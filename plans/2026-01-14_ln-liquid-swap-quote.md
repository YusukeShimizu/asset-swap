# ExecPlan: LN→Liquid Swap（Quote → CreateSwap）

## Goal

- buyer が `CreateQuote` で見積もり（`Quote`）を取得し、`quote_id` を指定して `CreateSwap` を呼べるようにする。
- seller は `quote_id` が指す条件（price/policy）が **現時点でも成立する**場合のみ swap を作成して返す。
- buyer は返却された swap 情報を検証し、LN 支払いと Liquid claim を完了できる。

### 非目的

- 価格の算出に外部のマーケット API を必須化しない（まずは固定パラメータでよい）。
- gRPC/TLS、認証/認可、インターネット公開の安全設計は対象にしない（閉域運用前提）。
- mainnet/testnet 対応は対象にしない（regtest 前提を維持する）。

## Scope

### 変更対象

- Protobuf（gRPC）:
  - `proto/ln_liquid_swap/v1/swap.proto` に `CreateQuote` と `Quote` を追加する。
  - `CreateSwap` は `quote_id` を受け取れるようにする（新 RPC 追加、または既存 request の拡張）。
- seller 実装:
  - `src/swap/service.rs` に `CreateQuote` を実装する。
  - `CreateSwap` は `quote_id` を検証し、price/policy が一致しない場合は拒否する。
- buyer 実装:
  - `src/bin/swap_cli.rs` を quote フローへ更新する。
- Integration Test:
  - `tests/ln_liquid_swap_e2e.rs` を quote フローへ更新する。
- 仕様と設計:
  - `spec.md` に quote フローの action を追加し、E2E の代表シナリオを更新する。
  - `docs/swap/ln-liquid-swap.mdx` と runbook を更新する（手順が変わるため）。

### 変更しない

- HTLC の witness script 形式（`OP_SHA256 + OP_CHECKLOCKTIMEVERIFY`）は維持する。
- HTLC outputs は explicit（unblinded）を維持する。

## Milestones

### M1: Protobuf に Quote を追加

#### 観測可能な成果

- `nix develop -c just proto_fmt proto_lint` が成功する。
- `cargo test --all` がビルド段階まで通る（tonic codegen を含む）。

#### 作業内容

- `CreateQuoteRequest` / `Quote` を追加する。
- `CreateSwap` の入力へ `quote_id` を導入する。
- 代表的エラーパターン（quote 不一致、期限切れ等）をコメントへ明記する。

### M2: seller が Quote を発行・検証できる

#### 観測可能な成果

- `CreateQuote` が `total_price_msat` を返せる。
- seller が `CreateSwap(quote_id)` を受けたとき、現在の price/policy と一致しない場合に拒否できる。

#### 作業内容

- seller 側で `Offer` と `offer_id`（price/policy のスナップショット ID）を定義する。
- `CreateQuote` で `Quote(quote_id, offer_id, total_price_msat, …)` を生成する。
- `CreateSwap` の先頭で `quote_id` を解決し、`offer_id` の一致を検証する。

### M3: CLI を quote フローへ更新

#### 観測可能な成果

- buyer が `CreateQuote` → `CreateSwap` → `pay` → `claim` まで通せる。
- buyer が LN 支払い前に「invoice amount == quote.total_price_msat」を検証できる。

#### 作業内容

- `swap_cli` が `CreateQuote` を呼び、`Quote` を保持する。
- `CreateSwap` を `quote_id` で呼ぶ。
- 支払い前に quote と swap の整合（invoice amount、asset_amount 等）を検証する。

### M4: Swap E2E integration test を quote フローへ更新

#### 観測可能な成果

- `cargo test --test ln_liquid_swap_e2e -- --ignored --nocapture` が成功する。

#### 作業内容

- `tests/ln_liquid_swap_e2e.rs` の `CreateSwap` 呼び出しを `CreateQuote` 経由へ置換する。
- 可能なら「quote を取得後に seller の price/policy を変更すると `CreateSwap` が失敗する」ことも観測する。

### M5: spec と docs を同期

#### 観測可能な成果

- `nix develop -c just ci` が成功する。

#### 作業内容

- `spec.md` の `LnLiquidSwap` concept に `create_quote` を追加する。
- `docs/swap/ln-liquid-swap.mdx` と runbook の手順を更新する。

## Tests

- `nix develop -c just ci`
- `nix develop -c just swap_e2e`
  - `tests/ln_liquid_swap_e2e.rs`（ignored、mock なし）

## Decisions / Risks

### 重要な判断

- `quote_id` の実装方式を決める。
  - 例: SQLite 永続（quote テーブル）か、署名付きトークン（ステートレス）か。
  - 理由: 再起動時の扱いと、不要な永続化のバランスが変わるため。
- 後方互換の扱いを決める。
  - 例: 既存の `GetOffer` / `max_total_price_msat` を残すか、破壊的変更とするか。

### 既知のリスクと緩和策

- リスク: quote が無制限に発行され、メモリや DB を圧迫する。
  - 緩和: quote の TTL と上限（max inflight quotes）を導入する。
- リスク: quote と swap の整合チェックが不足し、buyer が過払いする。
  - 緩和: buyer 側で「invoice amount == quote.total_price_msat」を必須チェックにする。

## Progress

- 2026-01-14: Quote → CreateSwap フローの ExecPlan を作成した。
