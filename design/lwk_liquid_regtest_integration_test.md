# Design: LWK Liquid regtest Integration Test

## Security & Architectural Constraints

- テストは `liquidregtest` のみで実行しなければならない（MUST）。
- テストは公開ネットワークへアクセスしてはならない（MUST NOT）。
  - 理由: テストを決定的かつ隔離された状態にするためである。
- テストは実プロセスを起動しなければならない（MUST）。
- テストはブロックチェーン状態を mock で代替してはならない（MUST NOT）。
  - 理由: end-to-end の信頼性を得るためである。
- テストはエフェメラルなポートと、テストごとのデータディレクトリを使わなければならない（MUST）。
  - 理由: 並列実行と再現性を両立するためである。
- テストは外部プロセスの stdout/stderr を収集しなければならない（MUST）。
  - 理由: 失敗時の原因調査を可能にするためである。
- テストで使う秘密情報はテスト専用とし、決定的に生成できることが望ましい（SHOULD）。
  - 理由: デバッグしやすくし、再利用を防ぐためである。
- 待機処理は全て期限付きでなければならない（MUST）。
- タイムアウト時は観測可能なログを残して失敗しなければならない（MUST）。
  - 理由: CI をハングさせないためである。

## Terms

- `Elements`: Elements Core のノードである。`elementsd` で起動する。
- `liquidregtest`: Elements の regtest チェーン名である。
- `Policy Asset`: ネットワークの基軸アセットである。LWK 上では LBTC として扱う。
- `Signer`: 署名するコンポーネントである。設計では `lwk_signer::SwSigner` を使う。
- `Descriptor`: ウォレット出力を表現する文字列である。Confidential address のために blinding key を含む。
- `Wollet`: LWK のウォレット実装である。設計では `lwk_wollet::Wollet` を使う。
- `Sync`: バックエンドからウォレット状態を更新する操作である。
- `Electrs (Liquid)`: Electrum プロトコルのサーバである。Liquid 対応ビルドを使う。

## Concepts

### `LiquidTestEnv`

目的は「`elementsd` とインデクサを起動し、同期可能な状態を提供する」ことである。

#### 責務

- `elementsd` を `liquidregtest` で起動する。
- 初期コインを sweep して採掘可能にする。
- `electrs`（Liquid 対応）を起動し、`ElectrumUrl` を提供する。
- ブロック生成と送金（policy asset / issued asset）を RPC で実行する。

#### 観測点

- `elementsd` RPC が疎通できる。
- `electrs` が `elementsd` の tip に追随する。

### `LwkWalletFixture`

目的は「Signer と Descriptor から Wollet を作り、同期と送金をテスト可能にする」ことである。

#### 責務

- `SwSigner` を生成またはロードする。
- Descriptor を生成する。
  - 例: `ct(slip77(<key>),elwpkh(<xpub>/*))#<checksum>`。
- `Wollet` を生成する。
  - 永続化の検証が必要なら `with_fs_persist` を使う。
  - そうでなければ `NoPersist` を使う。
- `Sync` を実行し、`balance(asset)` を返せるようにする。

## Flows

### Flow: Wallet 作成

1. テスト用の mnemonic を決める。
2. `SwSigner::new(mnemonic, is_mainnet=false)` で Signer を作る。
3. blinding key を決める。
   - `slip77(<key>)` を使う設計にする。
4. Descriptor を組み立て、checksum を付与する。
5. `Wollet` を生成し、初回 `Sync` を実行する。

#### 成功条件

- `Wollet::address()` が `liquidregtest` の address を返す。
- `Sync` 後に tip height を観測できる。

### Flow: LBTC 受領と同期

1. Issuer wallet の受領アドレスを取得する。
2. `elementsd` から policy asset（LBTC）を送金する。
3. `elementsd` でブロックを生成する。
4. `Issuer` の `Sync` を実行する。

#### 成功条件

- `Issuer` の policy asset 残高が増える。

### Flow: Asset issuance

1. Issuer wallet が issuance transaction を構築する。
   - 入力は policy asset の UTXO を使う。
   - `asset_amount` と `reissuance_token_amount` を指定する。
2. Issuer signer が署名する。
3. `elementsd` へブロードキャストする。
4. 採掘して確定させる。
5. Issuer wallet を `Sync` する。

#### 成功条件

- `asset_id` と `reissuance_token_id` を取得できる。
- Issuer の asset 残高と token 残高が指定値になる。
- Issuer の policy asset 残高が手数料分だけ減る。

### Flow: Asset 送信

Case A: Receiver wallet へ送る。

1. Receiver wallet の受領アドレスを取得する。
2. Issuer wallet が送金 transaction を構築する。
   - 宛先は Receiver の address と `asset_id` である。
3. Issuer signer が署名する。
4. `elementsd` へブロードキャストする。
5. 採掘して確定させる。
6. Issuer / Receiver を `Sync` する。

#### 成功条件

- Issuer の asset 残高が送金額だけ減る。
- Receiver の asset 残高が送金額だけ増える。

Case B: Node address へ送る。

1. `elementsd` の wallet で address を生成する。
2. Issuer からその address に `asset_id` を送る。
3. 採掘して確定させる。
4. Issuer を `Sync` する。

#### 成功条件

- Issuer の asset 残高が送金額だけ減る。

### Flow: 残高検証

目的は issuer 側と receiver 側の両方で「観測可能な数値」を検証することである。

#### 検証項目

- Issuer の policy asset 残高が `funding - fee - change` の関係を満たす。
- Issuer の issued asset 残高が `issued - sent` になる。
- Issuer の reissuance token 残高が初期発行量を保持する。
- Receiver の issued asset 残高が `received` になる。

## Observability

- 失敗時は `elementsd` と `electrs` のログを保存する。
- `KEEP_LWK_E2E_ARTIFACTS=1` の時は作業ディレクトリを保持する。
- `RUST_LOG=debug` の時はテストの観測点をログ出力する。
