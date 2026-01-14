# LN→Liquid Swap（gRPC）

本リポジトリは、Lightning の支払い（BOLT11）と Liquid の HTLC（P2WSH）を結合した
最小構成の LN→Liquid swap 実装である。

- 売り手は Liquid 側で HTLC を fund し、invoice を発行する。
- 買い手は funding を検証してから invoice を支払い、preimage で HTLC を claim する。
- 価格は seller が設定し、buyer は `CreateQuote` で見積もりを取得する。`CreateSwap` は `quote_id` を受け取り、見積もり取得後に条件が変化していれば拒否する。

完全な原子性は提供しない。想定は regtest / 検証環境である。

詳細は `docs/`（Mintlify）を参照する。

## Quick start

direnv を使う場合は、次を実行する。

```sh
direnv allow
just ci
```

direnv を使わない場合は、次を実行する。

```sh
nix develop -c just ci
```

## Binaries

- gRPC server: `swap_server`
- CLI: `swap_cli`

実行例は `docs/swap/ln-liquid-swap.mdx` を参照する。

## Logging

ログの詳細度は `RUST_LOG` で制御する。

```sh
echo 'export RUST_LOG=debug' > .envrc.local
direnv allow
nix develop -c cargo run --bin swap_server -- --help
```

## Protobuf（Buf）

スキーマは `proto/` 配下で管理する。

- API: `proto/ln_liquid_swap/v1/swap.proto`
- Format/Lint:

```sh
buf format -w
buf lint
```

## E2E（regtest）

E2E テストは `#[ignore]` である。`nix develop` 経由で必要な外部プロセスを起動する。

- LDK Server（Bitcoin regtest）: `nix develop -c just e2e`
- LWK（Liquid regtest）: `nix develop -c just lwk_e2e`
- LN→Liquid swap: `nix develop -c just swap_e2e`

失敗時にログや作業ディレクトリを保持する場合は `just e2e_keep` / `just lwk_e2e_keep` を使う。

## ドキュメント（Mintlify）

ドキュメントは `docs/` 配下で管理する。

- 設定: `docs/docs.json`
- Vale: `docs/.vale.ini`

CI は `nix develop -c just ci` で品質ゲートを実行する。
