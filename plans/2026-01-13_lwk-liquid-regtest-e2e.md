# ExecPlan: LWK Liquid regtest E2E integration test

## Goal

- LWK を使い、Liquid regtest 上で Integration Test を追加する。
- テストは「wallet 作成 → LBTC 受領と同期 → asset issuance → asset 送信 → 残高検証」を通す。
- 失敗時に原因調査ができるように、外部プロセスのログとテストの観測点を明確にする。

### 非目的

- mainnet/testnet を対象にしない。
- pegin を前提にしない。
- asset registry 連携は対象にしない。

## Scope

### 変更対象

- `tests/` に Liquid regtest の E2E シナリオを追加する。
- `Cargo.toml` の `dev-dependencies` に LWK 関連 crate を追加する。
- `flake.nix` に `elementsd` と Liquid 対応 `electrs` を提供する定義を追加する。
- `justfile` に E2E テスト実行用のレシピを追加する。

### 変更しない

- `src/` のプロダクションコードに Liquid 機能を追加しない。
- `just ci` に E2E テストを追加しない。

## Milestones

### M1: 実行環境（Nix）を確立する

#### 観測可能な成果

- `nix develop -c elementsd --version` が成功する。
- `nix develop -c electrs --help` が成功する。

#### 作業内容

- `flake.nix` に `elementsd` を追加する。
- `flake.nix` に Liquid 対応 `electrs` を追加する。
  - LWK の `electrs-flake` の `blockstream-electrs-liquid` を使う案を優先する。
  - 代替として `electrs` の Liquid 対応ビルドを `rev` 固定でビルドする。
- LWK の `lwk_test_util::TestEnvBuilder` を使う場合は、次の環境変数を dev shell で設定する。
  - `ELEMENTSD_EXEC`。
  - `ELECTRS_LIQUID_EXEC`。

### M2: Wallet 作成と同期をテストに落とす

#### 観測可能な成果

- Issuer wallet と Receiver wallet を生成できる。
- `Sync` 後に tip height を観測できる。

#### 作業内容

- `tests/support/` に `LwkWalletFixture` を実装する。
- `SwSigner` と Descriptor 生成を 1 箇所に集約する。

### M3: LBTC 受領と同期を通す

#### 観測可能な成果

- Issuer wallet の policy asset 残高が増える。

#### 作業内容

- `tests/support/` に `LiquidTestEnv` を実装する。
  - `elementsd` と `electrs` を起動する。
  - ブロック生成と送金を提供する。
- LBTC は `elementsd` の初期コインを sweep して用意する。

### M4: Asset issuance と送信を通す

#### 観測可能な成果

- `asset_id` と `reissuance_token_id` を取得できる。
- Issuer 側で asset と token の残高が増える。
- Receiver 側で asset の残高が増える。

#### 作業内容

- Issuer wallet で issuance transaction を構築して署名し、ブロードキャストする。
- Issuer wallet で issued asset を Receiver wallet の address に送る。

### M5: 残高検証を追加する

#### 観測可能な成果

- Issuer 側と Receiver 側の両方で残高を検証できる。

#### 作業内容

- 検証対象は policy asset、issued asset、reissuance token に分ける。
- 送金前後の差分で検証する。

### M6: 実行導線を整える

#### 観測可能な成果

- `nix develop -c just lwk_e2e` でテストを実行できる。
- 失敗時にログを残して再実行できる。

#### 作業内容

- テストは `#[ignore]` とし、`just` の専用レシピで実行する。
- `KEEP_LWK_E2E_ARTIFACTS=1` の時に作業ディレクトリを保持する。

## Tests

### 追加する Integration Test（案）

- `tests/lwk_liquid_regtest_e2e.rs`
  - 単一責任: 「wallet 作成 → 受領と同期 → issuance → 送信 → 残高検証」を通す。

### テストの前提

- `elementsd` と Liquid 対応 `electrs` が利用できる。
- バックエンドは Electrum を使う。

## Decisions / Risks

### 重要な判断

- pegin を避ける。
  - 理由: セットアップが重くなる。
- インデクサは `electrs` を使う。
  - 理由: LWK の Electrum backend が成熟している。

### 既知のリスクと緩和策

- リスク: インデクサの同期待ちでフレークする。
  - 緩和: tip height を期限付きでポーリングする。
- リスク: confidential transaction のため、外部ツールで調査しにくい。
  - 緩和: 送金額と残高差分をテスト内で出力する。

## Progress

- 2026-01-13: ExecPlan を作成した。実装は未着手である。
- 2026-01-13: `flake.nix` に `elementsd` と `electrs`（Liquid 対応）を追加した。
- 2026-01-13: `tests/support/` に `LiquidRegtestEnv` と `LwkWalletFixture` を追加した。
- 2026-01-13: `tests/lwk_liquid_regtest_e2e.rs` と `just lwk_e2e` を追加した。
