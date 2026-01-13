# ExecPlan: LDK Server regtest E2E integration test

## Goal

- LDK Server（`ldk-server` デーモン）をテスト内で実際に起動する。regtest 上で「チャネル作成 → 請求書（BOLT11） → 支払い（送金）」までを自動で通す Integration Test を追加する。
- 失敗時に原因調査ができるように、外部プロセスのログとテストの観測点を明確にする。

### 非目的

- mainnet/testnet を対象にしない。
- LSPS/JIT などの高度なフローを対象にしない（まずは通常のチャネル + BOLT11 を通す）。
- ルーティング（複数ホップ）を対象にしない（まずは 1 hop で通す）。

## Scope

### 変更対象

- `tests/` に E2E シナリオを追加する。
- `Cargo.toml` の `dev-dependencies` に、テスト用の最小限の依存を追加する。
- `flake.nix` に `bitcoind` と `ldk-server` 実行環境（バイナリ）を提供するための定義を追加する。
- （必要なら）`justfile` に E2E テスト実行用のレシピを追加する。
- （必要なら）`spec.md` に新しい代表操作（E2E）の concept/action を追加する。

### 変更しない

- 本体のプロダクションコード（`src/`）に Lightning 実装を追加しない（テストハーネス中心）。
- Docker 前提の手順は追加しない（Nix Flakes の再現性を優先する）。

## Milestones

### M1: 実行環境（Nix）を確立する

#### 観測可能な成果

- `nix develop -c bitcoind --version` が成功する。
- `nix develop -c ldk-server --help`（または同等）が成功する。

#### 作業内容

- `flake.nix` に Bitcoin Core（`bitcoind`）を追加する。
- `flake.nix` に LDK Server（`ldk-server`）バイナリを追加する。
  - 推奨: `pkgs.rustPlatform.buildRustPackage` で upstream の `lightningdevkit/ldk-server` を `rev` 固定でビルドする。
  - 代替: `cargo install --git ... --rev ...` を `just` で管理する（ただし再現性は Nix ビルドより落ちる）。
- CI での実行時間が重い場合に備え、E2E テストを `#[ignore]` で段階導入できるように設計する（M4 で決める）。

### M2: bitcoind（regtest）をテストから起動できるようにする

#### 観測可能な成果

- Integration Test から `bitcoind` を起動し、RPC が疎通できる。
- `generatetoaddress` でブロックを生成し、`getblockcount` が増える。

#### 作業内容（テストサポート）

- `tests/support/`（または `tests/support.rs`）に `BitcoindProcess` を実装する。
  - `tempfile` で `datadir` を作る。
  - `bitcoin.conf` を生成する。最低限の設定を入れる。
    - `regtest=1`。
    - `server=1`。
    - `rpcuser` と `rpcpassword`。
    - `rpcport` と `port`。
    - `fallbackfee`。
  - `std::process::Command` で起動し、`bitcoincore_rpc` で readiness をポーリングする。
  - Drop で `kill` し、必要なら `wait` する。
- `bitcoincore-rpc` を `dev-dependencies` に追加する。

#### 設計メモ

- コイン生成は「新規ウォレット作成 → 101 ブロック採掘 → 成熟後に送金」を基本形にする。
- ポート衝突のリスクがあるため、ランダムポートを選び、起動失敗時はリトライする。

### M3: ldk-server（Alice/Bob）をテストから起動できるようにする

#### 観測可能な成果

- 2 つの `ldk-server` インスタンスを別ポートで起動できる。
- 両方に対して `GetNodeInfo` が成功し、`node_id` を取得できる。

#### 作業内容（テストサポート）

- `tests/support/` に `LdkServerProcess` を実装する。
  - 各ノードごとに `storage.disk.dir_path` とログファイルを分ける。
  - `node.listening_address` と `node.rest_service_address` は衝突しないポートを割り当てる。
  - `bitcoind` RPC 設定は M2 の `BitcoindProcess` と一致させる。
  - readiness は `ldk-server-client`（または `reqwest` + `prost`）で `GetNodeInfo` を繰り返し叩いて判定する。
- Rust 側の API クライアントは、可能なら upstream の `ldk-server-client` を `dev-dependency`（git + `rev` 固定）で取り込む。
  - 代替: `ldk-server-protos` を取り込み、`reqwest` で `application/octet-stream` の Protobuf POST を直接実装する。

### M4: E2E シナリオ（チャネル作成 → 請求書 → 支払い）を 1 本の Integration Test にする

#### 観測可能な成果

- `cargo test --test ldk_server_regtest_e2e`（仮）が成功する。
- テストが以下の観測点を満たす。
  - チャネルが両ノードで `is_usable=true` になる。
  - 支払いが送金側で `OUTBOUND + SUCCEEDED` として観測できる。
  - 支払いが受領側で `INBOUND + SUCCEEDED` として観測できる。

#### シナリオ案（最小）

1. bitcoind regtest 起動（M2）。
2. `ldk-server` を 2 台起動（Alice/Bob, M3）。
3. Alice のオンチェーンアドレスを `OnchainReceive` で取得する。
4. bitcoind から Alice に送金し、1 ブロック採掘して入金を確定する。
5. Bob の `node_id` と `listening_address` を使い、Alice から `OpenChannel` を実行する。
6. `ListChannels` をポーリングしつつ必要数のブロックを採掘し、両者で `is_usable=true` を待つ。
7. Bob が `Bolt11Receive` で請求書を発行する。
8. Alice が `Bolt11Send` で支払う。
9. `ListPayments` をポーリングして SUCCEEDED を両者で確認する。

#### 安定化ポイント

- 全ての待機は「期限付きポーリング（例: 60–120 秒）+ 失敗時のログ出力」を標準にする。
- regtest のブロック生成はテストが責任を持つ（外部状態に依存しない）。

### M5: 実行導線（CI/ローカル）を整える

#### 観測可能な成果

- `nix develop -c cargo test --test ldk_server_regtest_e2e -- --nocapture` が再現可能に動く。
- 失敗時に `bitcoind` / `ldk-server` のログを参照できる。

#### 作業内容

- 実行時間が許容できる場合は `just ci` に含める。
- 重い場合は次のどちらかを選ぶ:
  - `#[ignore]` で `just e2e`（新設）に分離する。
  - 環境変数（例: `RUN_LDK_E2E=1`）がある時だけ実行する。

## Tests

### 追加する Integration Test（案）

- `tests/ldk_server_regtest_e2e.rs`
  - 単一責任: 「ldk-server を 2 台起動し、チャネル作成 → 請求書 → 支払い」を通す。
  - 依存: `BitcoindProcess`, `LdkServerProcess`, `wait_for` ユーティリティ。

### 実装するサポート（案）

- `tests/support/bitcoind.rs`: bitcoind 起動と RPC 操作（採掘・送金）。
- `tests/support/ldk_server.rs`: ldk-server 起動と API クライアント。
- `tests/support/wait.rs`: 期限付きポーリング（指数バックオフ + 観測ログ）。

## Decisions / Risks

### 重要な判断

- API 操作は `ldk-server-client` を優先する。
  - 理由: Protobuf の HTTP POST とエラーデコードを既に実装しているため、テスト側の責務を減らせる。
- ノードは 2 台（Alice/Bob）の `ldk-server` だけで完結させる。
  - 理由: 「チャネル作成 → 支払い」を最短で閉じるには、外部 LN 実装（LND/CLN）を入れない方が安定する。

### 既知のリスクと緩和策

- リスク: 同期・チャネル確定待ちが不安定でフレークする。
  - 緩和: `GetNodeInfo.current_best_block` と `ListChannels.is_usable` を観測し、期限付きで待つ。
- リスク: ポート衝突で起動に失敗する。
  - 緩和: ランダムポート + 起動失敗リトライ。
- リスク: bitcoind の fee 推定が失敗して送金・チャネル作成が止まる。
  - 緩和: `fallbackfee` を設定する。
- リスク: upstream の `ldk-server` API が breaking change する。
  - 緩和: Nix と Cargo の双方で `rev` 固定する。

## Progress

- 2026-01-12: ExecPlan を作成した。
- 2026-01-12: `flake.nix` に `bitcoin` と `ldk-server` を追加した。
- 2026-01-12: `tests/ldk_server_regtest_e2e.rs` と `tests/support/` を実装した。
- 2026-01-12: `just e2e` と `just e2e_keep` を追加した。
