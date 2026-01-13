# Rust Template

このリポジトリは、Rust プロジェクトを開始するためのテンプレートである。
開発環境は Nix Flakes を正とし、ローカルの環境変数は direnv（`.envrc`）で管理する。

## Quick start

```sh
direnv allow
just ci
```

direnv を使わない場合は、次を実行する。

```sh
nix develop -c just ci
```

## 実行例

```sh
cargo run -- hello
cargo run -- hello --name Alice
```

## Logging

`RUST_LOG` でログの詳細度を切り替える。

```sh
echo 'export RUST_LOG=debug' > .envrc.local
direnv allow
cargo run -- hello
```

## E2E（LDK Server regtest）

`bitcoind` と `ldk-server` を起動する。
`tests/ldk_server_regtest_e2e.rs` で「チャネル作成 → 請求書（BOLT11） → 支払い」を検証する。
このテストは `#[ignore]` である。

次で実行する。

```sh
nix develop -c just e2e
```

失敗時にログを残す場合は、次を実行する。

```sh
nix develop -c just e2e_keep
```

## Protobuf（Buf）

Protobuf スキーマは `proto/` 配下で管理し、Buf で lint/format する。

```sh
buf lint
buf format -w
```

例として、AIP 準拠の Resource Oriented API と Protovalidate を使ったスキーマを
`proto/template/v1/greetings.proto` に含める。

## ドキュメント（Mintlify）

Mintlify で動かすことを前提に、ドキュメントは `docs/` 配下に置く。

- 設定: `docs/docs.json`
- Vale: `docs/.vale.ini`

CI では `just ci` がドキュメントの検査も実行する。

## テンプレートの置換

- `Cargo.toml` の `name` をプロジェクト名に変更する。
- `tests/` のバイナリ名（`cargo_bin("template")`）を変更後の名前に合わせる。
