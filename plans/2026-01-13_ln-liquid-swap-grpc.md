# ExecPlan: LN→Liquid Swap（gRPC / P2WSH HTLC）

## Goal

- gRPC API のみで「買い手が LN で支払うと、Liquid のアセットを受け取れる」最小 swap を実装する。
- Liquid 側は P2WSH HTLC（`OP_SHA256` + `OP_CHECKLOCKTIMEVERIFY`）でロックし、買い手は LN 支払い成功で得た preimage で claim する。
- Integration Test で「swap 作成 → LN 支払い → Liquid claim → 残高増加」を mock なしで再現する。

### 非目的

- mainnet/testnet の運用設計（鍵管理、SLA、監視、スケーリング）は対象にしない。
- Confidential transaction の blinding を一般解として実装しない（最小構成として HTLC output は explicit を採用する）。
- HODL invoice や LN 側のカスタム HTLC 条件を要求しない（通常の BOLT11 支払いで成立させる）。

## Scope

### 変更対象

- `proto/` に swap 用の gRPC schema を追加する。
- `src/bin/` に gRPC サーバ（売り手）と CLI（買い手）を追加する。
- `src/` に HTLC script と claim/refund tx 構築の実装を追加する。
- `tests/` に LN（regtest）+ Liquid（liquidregtest）を同時に起動する E2E test を追加する。
- `justfile` に swap E2E の実行レシピを追加する（必要なら）。
- （必要なら）`spec.md` に代表操作（E2E）の concept/action を追加する。

### 変更しない

- 既存のテンプレート機能（`hello` CLI、既存 E2E）の削除や破壊はしない。
- swap の価格決定やマーケット API は実装しない（固定パラメータでよい）。

## Milestones

### M1: Protobuf と gRPC サーバ骨格

#### 観測可能な成果

- `buf lint` が成功する。
- `cargo test --all` がビルド段階まで通る（gRPC のコード生成を含む）。

#### 作業内容

- `proto/ln_liquid_swap/v1/swap.proto` を追加する（Create/Get を最小で定義する）。
- `tonic-build` を導入し、swap の proto だけを codegen する。

### M2: Liquid HTLC と funding の実装

#### 観測可能な成果

- 売り手が `CreateSwap` を呼ばれたとき、HTLC witness script を生成できる。
- 売り手が funding tx を作成・署名・ブロードキャストできる。

#### 作業内容

- `LiquidHtlc`（witness script、P2WSH address、spend 条件）を実装する。
- LWK を使い、HTLC へ「アセット output + fee subsidy（LBTC）output」を explicit で送る。
- swap の復旧に必要な情報を `SwapStore` に永続化する。

### M3: LN 支払いと Liquid claim（買い手）

#### 観測可能な成果

- 買い手が invoice を支払い、`preimage` を取得できる。
- `preimage` で HTLC を spend する claim tx を構築し、ブロードキャストできる。

#### 作業内容

- `ldk-server` の `Bolt11Send` と `ListPayments` を使って支払い結果を観測する。
- `preimage` から `SHA256(preimage)` を計算し、`payment_hash` と一致することを検証する。
- claim tx を組み立て、P2WSH witness（claim）を入力ごとに付与する。

### M4: Swap E2E integration test

#### 観測可能な成果

- `cargo test --test ln_liquid_swap_e2e -- --ignored --nocapture` が成功する。
- テストが以下を観測する。
  - LN 支払いが `Succeeded` である。
  - Liquid 側で買い手のアセット残高が増える。

#### 作業内容

- `bitcoind` + `ldk-server`（2 ノード）を起動し、チャネルを開く。
- `elementsd` + `electrs-liquid` を起動し、売り手が asset を発行して在庫を用意する。
- gRPC サーバを起動し、買い手が `CreateSwap`→支払い→claim まで通す。

### M5: refund の最低限

#### 観測可能な成果

- `refund_lock_height` 到来後、売り手が refund tx をブロードキャストできる。

#### 作業内容

- refund tx を構築し、CLTV 条件を満たす `nLockTime` と `sequence` を設定する。
- refund が先に claim されている場合の失敗を観測可能にする。

## Tests

- `tests/ln_liquid_swap_e2e.rs`
  - 依存:
    - `tests/support/bitcoind.rs`
    - `tests/support/ldk_server.rs`
    - `tests/support/lwk_env.rs`
    - `tests/support/lwk_wallet.rs`
  - mock を使わない。
  - `#[ignore]` とし、`nix develop` 内で実行する。

## Decisions / Risks

### 重要な判断

- 外部 API は gRPC のみにする。
  - 理由: JSON/HTTP の二重実装を避け、インターフェイスを固定するため。
- HTLC output は explicit（unblinded）にする。
  - 理由: blinding factor の永続化と復旧を最小化し、claim/refund の実装を先に成立させるため。

### 既知のリスクと緩和策

- リスク: gRPC codegen が `googleapis` 依存で壊れる。
  - 緩和: swap の proto は `google.api.http` などを import せず、単独で codegen する。
- リスク: LN と Liquid の E2E は起動プロセス数が多くフレークする。
  - 緩和: 全待機を期限付きにし、失敗時にログパスを出す。
- リスク: refund/claim の tx 構築が Elements の仕様差分で失敗する。
  - 緩和: Elements の `SighashCache` と `TxOut::new_fee` を使い、最小の explicit tx で検証する。

## Progress

- 2026-01-13: ExecPlan を作成した。
