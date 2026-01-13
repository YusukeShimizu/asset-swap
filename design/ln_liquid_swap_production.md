# Design: LN→Liquid Swap（Production Readiness / gRPC）

本書は `design/ln_liquid_swap_minimal.md` の最小 swap を、production で運用可能な水準へ引き上げるための設計である。
外部 API は **gRPC のみ**とする。

## Security & Architectural Constraints

- 外部 API は gRPC のみで提供しなければならない（MUST）。
  - 理由: API 面を単純にし、二重実装を避けるため。
- 本設計は「閉域・信頼ネットワークでの運用」を前提とし、gRPC の TLS と認証/認可は必須としない（MAY）。
  - 理由: 運用要件によっては L4/L7 の外側で保護でき、アプリに実装しない選択があるため。
  - 注意: インターネット露出する場合は TLS と認証/認可を追加する。
- buyer が価格を事前に知れるよう、`GetOffer` で price/policy を提示しなければならない（MUST）。
  - 理由: `CreateSwap` は資産を lock する副作用があるため、価格提示と分離する必要がある。
- `CreateSwap` は buyer の過払い防止のために `max_total_price_msat` を受け取り、上限を超える場合は拒否しなければならない（MUST）。
  - 理由: `GetOffer` と `CreateSwap` の間の価格変更や再試行で、想定外の支払いを防ぐため。
- `CreateSwap` は冪等でなければならない（MUST）。
  - 例: `request_id` により重複作成を抑止する。
  - 理由: クライアント再試行で二重 funding を起こさないため。
- swap 状態は軽量でトランザクション性のある永続ストアへ保存しなければならない（MUST）。
  - 例: SQLite（WAL）。
  - 理由: 再起動後に refund と監視を再開するため。
- 在庫管理は UTXO 単位で予約しなければならない（MUST）。
  - 理由: 同一 UTXO を複数 swap に割り当てないため。
- buyer は LN 支払い前に、funding の十分性を検証しなければならない（MUST）。
  - 理由: 未 fund の swap へ先払いしないため。
- buyer は LN 支払い前に、invoice の `payment_hash` と HTLC の hashlock を照合しなければならない（MUST）。
  - 理由: 不一致なロックへ先払いしないため。
- HTLC claim パスは buyer 署名を要求しなければならない（MUST）。
  - 理由: 経路上ノードが preimage を知り得るため。
- refund パスは seller 署名と CLTV を要求しなければならない（MUST）。
  - 理由: 早期 refund を防ぐため。
- `refund_lock_height` は invoice の有効期限と運用遅延を考慮して決定しなければならない（MUST）。
  - 理由: buyer が支払い後に claim する時間を確保するため。
- funding tx の確認条件は server policy として固定しなければならない（MUST）。
  - 理由: buyer の想定とズレると安全性が低下するため。
- チェーン再編（reorg）を考慮して confirmations を扱わなければならない（MUST）。
  - 理由: 取り消された confirmation を信頼すると資産ロックが崩れるため。
- 観測性を備えなければならない（MUST）。
  - 例: `tracing` の相関 ID、メトリクス、アラート。
  - 理由: LN/Liquid の境界は障害解析が難しいため。

## Concepts

### `SwapSellerDaemon`（server）

責務は次のとおりである。

- gRPC で swap を作成し、状態を返す。
- `GetOffer` で価格とポリシーを提示する。
- 在庫 UTXO を予約し、funding tx を構築する。
- LN invoice を作成し、`payment_hash` を抽出する。
- `min_funding_confs` を満たすまで監視し、状態を更新する。
- HTLC outpoint の spend を監視し、claim/refund を判定する。
- `refund_lock_height` 到来後に refund を試行する。

対応する実装の起点は次のとおりである。

- gRPC: `proto/ln_liquid_swap/v1/swap.proto`
- server: `src/swap/service.rs`, `src/bin/swap_seller.rs`

### `SwapStore`（永続ストア）

目的は「再起動しても swap を復元できる」ことである。
最低限、次の情報を保存する。

- `swap_id`
- `payment_hash`
- `bolt11_invoice`（または invoice の参照）
- HTLC: `witness_script`, `p2wsh_address`, `refund_lock_height`
- funding: `funding_txid`, `asset_vout`, `lbtc_vout`
- policy: `min_funding_confs`, `fee_subsidy_sats`
- status: `Created/Funded/Claimed/Refunded/Failed`
- reservation: 使用した UTXO（asset と LBTC）

### `InventoryManager`（在庫管理）

目的は「同一 UTXO の二重割当を防ぐ」ことである。

- asset と LBTC を別々に選択する。
- swap 作成時に予約し、確定後に消費済みへ遷移する。
- swap 失敗時に予約を解放する。

### `LiquidWatcher`（Liquid 監視）

目的は「funding の確定と spend を追跡する」ことである。

- funding tx の confirmations を監視する。
- HTLC outpoint の spend を監視する。
- spend が claim/refund のどちらかを推定し、状態を更新する。
- reorg を扱う。

### `LnWatcher`（LN 監視）

目的は「invoice の支払い状態を追跡する」ことである。

- invoice が `Succeeded` になったことを観測する。
- 支払い完了後も claim が観測できない場合に警告する。

## Flows

### Flow: CreateSwap（production）

1. server は入力を検証する。
2. server は `request_id` により冪等性を確保する。
3. server は `price_msat = asset_amount * price_msat_per_asset_unit` を計算する。
4. server は `max_total_price_msat` を超える場合は拒否する。
5. server は在庫 UTXO を予約する。
6. server は LN invoice を作成する。
7. server は invoice から `payment_hash` を抽出する。
8. server は HTLC witness script を生成する。
9. server は funding tx（PSET）を作成し、署名して放流する。
10. server は swap を `Created` として保存する。
11. server は `min_funding_confs` を待つ。
12. server は `Funded` を保存し、client へ応答する。

### Flow: Buyer pay & claim（production）

1. buyer は `GetOffer` で価格とポリシーを得る。
2. buyer は `max_total_price_msat` を設定して `CreateSwap`（または `GetSwap`）で swap を得る。
3. buyer は invoice と HTLC を検証する。
4. buyer は funding confirmations を検証する。
5. buyer は LN 支払いを実行する。
6. buyer は preimage を得る。
7. buyer は preimage と署名で HTLC を claim する。

### Flow: Refund（production）

1. server は `refund_lock_height` 到来を検知する。
2. server は未 claim の swap を抽出する。
3. server は refund tx を構築して放流する。
4. server は refund 確定を監視する。
5. server は status を `Refunded` に更新する。

## Production Gaps（最小実装との差分）

次は production で必須になりやすい。
ただし、実装順は運用要件で変わる。

- 冪等性キーと重複排除。
- DB 化とマイグレーション。
- UTXO 予約と二重割当防止。
- claim 検知と状態遷移の自動化。
- reorg を含む監視の堅牢化。
- メトリクスとアラート。

## Notes

- Confidential transaction を production で要求する場合がある。
  - その場合、HTLC output の blinding に必要な情報を永続化する。
  - 本スコープでは blinding は扱わない（explicit output を継続する）。
