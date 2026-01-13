# ExecPlan: LN→Liquid Swap（Production Lite / gRPC）

## Goal

- `design/ln_liquid_swap_minimal.md` の最小 swap を、**閉域・単一プロセス運用**の production-lite として堅牢化する。
- 具体的には「冪等な `CreateSwap`」「軽量 DB 永続化」「在庫の二重割当防止」「claim/refund の状態追跡」を成立させる。

### 非目的

- gRPC/TLS、認証/認可の実装は対象にしない（閉域で運用する）。
- Confidential transaction（blinding）は扱わない（HTLC outputs は explicit を継続する）。
- 水平スケール（複数インスタンスでの同一在庫共有）を成立させない。

## Scope

### 変更対象

- Protobuf（gRPC）:
  - `proto/ln_liquid_swap/v1/swap.proto` の request/response を拡張する（冪等性キー等）。
- 永続化:
  - `src/swap/store.rs` を軽量 DB（例: SQLite WAL）ベースに置換する。
  - swap / request_id / 在庫予約のスキーマを追加する。
- 売り手サーバ:
  - `src/swap/service.rs` の `CreateSwap` を冪等化し、状態遷移を明確化する。
  - `src/bin/swap_seller.rs` に監視ワーカー（claim/refund/funding）を追加する。
- Liquid:
  - funding 確認と spend 追跡のために、Electrum から必要な情報を取得する（必要なら依存を追加する）。
- ドキュメント:
  - `docs/swap/ln-liquid-swap.mdx` に production-lite の利用上の注意と新フィールドを追記する。

### 変更しない

- 外部 API を gRPC 以外で提供しない（HTTP JSON は追加しない）。
- HTLC スクリプトの形式（`OP_SHA256 + OP_CHECKLOCKTIMEVERIFY`）は維持する。
- buyer 側の「LN 支払い前の安全検証」の要件は緩めない。

## Milestones

### M1: SQLite 永続化の導入（SwapStore 置換）

#### 観測可能な成果

- `cargo test --all` が通る。
- プロセス再起動後に `GetSwap` が同一 `swap_id` を返せる（store の永続性がある）。

#### 作業内容

- `src/swap/store.rs` を `SqliteSwapStore` に置換する。
  - swap テーブル（状態、funding outpoint、witness_script 等）。
  - request_id テーブル（冪等性キー→swap_id）。
  - 在庫予約テーブル（reserved utxo / amount）。
- マイグレーションを用意する（起動時に自動適用）。
- store の integration test を追加する（SQLite を実 DB として使う）。

### M2: `CreateSwap` の冪等化（request_id）

#### 観測可能な成果

- 同一 `request_id` の `CreateSwap` を複数回呼んでも、同じ `swap_id` と同じ `funding_txid` が返る。
- 再試行で在庫が二重に lock されない。

#### 作業内容

- `proto/ln_liquid_swap/v1/swap.proto` に `request_id`（UUID 文字列）を追加する。
  - Protovalidate で形式を制約する。
  - コメントに冪等性の意味と代表的エラーを追記する。
- `src/swap/service.rs` で次を実装する。
  - `request_id` が存在する場合は既存 swap を返す（冪等）。
  - 未存在なら新規作成し、request_id と swap を同一トランザクションで保存する。
- E2E（ignored）に idempotency の観測を追加する（`tests/ln_liquid_swap_e2e.rs`）。

### M3: 在庫の二重割当防止（単一プロセス前提）

#### 観測可能な成果

- 同時に複数の `CreateSwap` が来ても、在庫不足時に安全に失敗し、二重 funding を起こさない。

#### 作業内容

- server は wallet 操作を直列化する（既存の mutex を維持し、DB でも整合を担保する）。
- DB に「予約」を記録し、作成失敗時に必ず解放する。
- UTXO 単位の予約が困難な場合は、まず「単一プロセス + wallet 直列化 + 予約量の上限」で安全側に倒す。
  - 追加の制約（例: max inflight swaps）を config として導入する。

### M4: Claim/Refund の状態追跡（Watcher）

#### 観測可能な成果

- claim された swap が `Claimed` に遷移する（buyer により HTLC outpoint が消費されたことを検知できる）。
- refund が成功した swap が `Refunded` に遷移する。

#### 作業内容

- Liquid watcher を追加する。
  - HTLC scriptPubKey の unspent を確認し、outpoint が消費されたことを検知する。
  - 必要なら `electrum-client` を直接使い、`scripthash.listunspent` 相当を実装する。
- refund worker を「期限到来→未消費→refund 試行→確認」まで管理する。
- 状態遷移を `Created → Funded → Claimed/Refunded` に制限し、不正な遷移を拒否する。

### M5: ドキュメントと運用ノブ

#### 観測可能な成果

- 利用者が docs の手順で swap を作成し、buyer 側の検証ポイントを理解できる。

#### 作業内容

- `docs/swap/ln-liquid-swap.mdx` に次を追記する。
  - `request_id` の使い方（再試行の前提）。
  - `CreateSwap` が長時間ブロックし得る点（`min_funding_confs`）と推奨値。
  - production-lite 前提（閉域、単一インスタンス、explicit outputs）。
- server の設定項目を整理する（timeout、max inflight、poll interval）。

## Tests

- `just ci` を通す。
- store: SQLite を使う integration test を追加する（mock 不要）。
- swap: `tests/ln_liquid_swap_e2e.rs`（ignored）に次を追加する。
  - 同一 `request_id` の `CreateSwap` 冪等性の観測。
  - claim 後の seller 側 `GetSwap` が `Claimed` へ遷移する観測（Watcher が入った場合）。

## Decisions / Risks

### 重要な判断

- TLS/認証/認可は実装しない。
  - 理由: 閉域・信頼ネットワークでの運用を前提とするため。
  - リスク: 露出すると資産枯渇や改ざんのリスクがある。
- DB は軽量（SQLite WAL）を採用する。
  - 理由: 単一プロセス運用に適合し、導入が容易なため。
- blinding は扱わず、explicit HTLC outputs を継続する。
  - 理由: 永続化と復旧の複雑性を後回しにするため。

### 既知のリスクと緩和策

- リスク: Electrum backend だけでは spend の追跡が難しい。
  - 緩和: `listunspent` 相当の API を利用できる backend を前提にする。
- リスク: `CreateSwap` の confirm 待ちで RPC が長時間化する。
  - 緩和: `min_funding_confs` の上限、server 側タイムアウト、buyer 側は `GetSwap` で再取得できる設計にする。
- リスク: reorg により confirmations が巻き戻る。
  - 緩和: 状態遷移は再評価可能にし、`Funded` の確定条件を運用ポリシーで固定する。

## Progress

- 2026-01-13: production-lite 前提（no TLS/auth, SQLite, no blinding）で ExecPlan を作成した。
- 2026-01-14: `SqliteSwapStore` を導入し、seller/server から利用するように置換した。SQLite store の integration test も追加した。
