# Rust Template Specification

このリポジトリは、Rust プロジェクトを開始するための汎用テンプレートである。
本書（`spec.md`）は、テンプレートに含める機能と開発上の不変条件を定義する。

本書は、arXiv:2508.14511v2 に準拠する。
対象論文のタイトルは「What You See Is What It Does」である。
本書は Concept Specifications と Synchronizations で構造化して記述する。
参照: https://arxiv.org/html/2508.14511v2

## Concepts

```text
concept Shell
purpose
    外部（開発者・CI）からのコマンド実行を表現する。
state
    env: string -> string
actions
    request [ command: string ]
        => [ ]
```

```text
concept DevEnvironment
purpose
    開発環境を Nix Flakes で再現する。
    環境変数を direnv（`.envrc`）で管理する。
state
    flake_nix: string
    flake_lock: string
    envrc: string
    envrc_local: string
actions
    enter [ tool: "direnv" ]
        => [ ]
        load the dev shell via `use flake` in `.envrc`
        load local-only overrides from `.envrc.local` when present
    enter [ tool: "nix" ]
        => [ ]
        enter the dev shell via `nix develop`
operational principle
    after enter [ tool: "direnv" ]
        => [ ]
    then Shell/request [ command: "just ci" ]
        => [ ]
```

```text
concept RustCLI
purpose
    Rust のツールチェーンと CI 配線を検証するための最小 CLI を提供する。
state
    binary_name: string
actions
    hello [ ]
        => [ exit_code: 0 ; stdout: "Hello, world!\n" ]
        CLI 引数の parse 後に debug ログ `"parsed cli"` を出力する。
    hello [ name: string ]
        => [ exit_code: 0 ; stdout: "Hello, <NAME>!\n" ]
        CLI 引数の parse 後に debug ログ `"parsed cli"` を出力する。
operational principle
    after hello [ ]
        => [ exit_code: 0 ; stdout: "Hello, world!\n" ]
    then hello [ name: "Alice" ]
        => [ exit_code: 0 ; stdout: "Hello, Alice!\n" ]
```

```text
concept Logging
purpose
    tracing による構造化ログを提供する。
    ログ詳細度を `RUST_LOG` で制御できるようにする。
state
    rust_log: string
actions
    init [ ]
        => [ ]
        configure `tracing_subscriber::EnvFilter` from `RUST_LOG`
        default to `info` when `RUST_LOG` is not set
        write logs to stderr
        when the effective filter enables debug, emit debug logs (e.g. `"parsed cli"`)
operational principle
    after init [ ]
        => [ ]
    then RustCLI/hello [ ]
        => [ exit_code: 0 ]
```

```text
concept RustToolchain
purpose
    Rust コードの代表的な品質ゲートを提供する。
state
    src_dir: string
    tests_dir: string
actions
    fmt_check [ ]
        => [ ok: boolean ]
        run `cargo fmt --all -- --check`
    clippy [ ]
        => [ ok: boolean ]
        run `cargo clippy --all-targets --all-features -- -D warnings`
    test [ ]
        => [ ok: boolean ]
        run `cargo test --all`
```

```text
concept IntegrationTests
purpose
    代表的な操作を Integration Test（`tests/`）で表現する。
    Integration Test では mock を使わない。
state
    tests_dir: string
actions
    run [ ]
        => [ ok: boolean ]
        run `cargo test --all`
```

```text
concept LdkServerRegtestE2E
purpose
    LDK Server と bitcoind を実際に起動し、regtest 上でチャネル作成と BOLT11 支払いを検証する。
state
    bitcoind: string
    ldk_server: string
    test_file: string
actions
    run [ ]
        => [ ok: boolean ]
        run `cargo test --test ldk_server_regtest_e2e -- --ignored --nocapture`
operational principle
    after run [ ]
        => [ ok: true ]
```

```text
concept LwkLiquidRegtestE2E
purpose
    LWK と elementsd/electrs を実際に起動し、Liquid regtest 上でアセット発行と送受信を検証する。
state
    elementsd: string
    electrs_liquid: string
    test_file: string
actions
    run [ ]
        => [ ok: boolean ]
        run `cargo test --test lwk_liquid_regtest_e2e -- --ignored --nocapture`
operational principle
    after run [ ]
        => [ ok: true ]
```

```text
concept Protobuf
purpose
    Protobuf スキーマを Buf で管理する。
state
    proto_dir: string
    buf_yaml: string
    buf_lock: string
actions
    format_check [ ]
        => [ ok: boolean ]
        run `buf format -d --exit-code`
    lint [ ]
        => [ ok: boolean ]
        run `buf lint`
    dep_update [ ]
        => [ ok: boolean ]
        run `buf dep update` (writes `buf.lock`)
```

```text
concept Documentation
purpose
    ドキュメントを `docs/` 配下で管理し、文章とリンクの検査を行う。
state
    docs_dir: string
    docs_json: string
    vale_ini: string
actions
    vale [ ]
        => [ ok: boolean ]
        run `vale --config docs/.vale.ini --glob='*.mdx' docs`
    broken_links [ ]
        => [ ok: boolean ]
        run `mint broken-links` for `docs/`
```

```text
concept Textlint
purpose
    Markdown 文章を textlint で検査する。
state
    config: string
    prh: string
actions
    lint_markdown [ ]
        => [ ok: boolean ]
        run `textlint` for tracked `*.md` files (excluding `.codex/`)
```

## Synchronizations

```text
sync CI
when {
    Shell/request: [ command: "just ci" ]
        => [] }
then {
    RustToolchain/fmt_check: [ ]
    Protobuf/format_check: [ ]
    Protobuf/lint: [ ]
    RustToolchain/clippy: [ ]
    RustToolchain/test: [ ]
    Textlint/lint_markdown: [ ]
    Documentation/vale: [ ]
    Documentation/broken_links: [ ] }
```

```text
sync E2E
when {
    Shell/request: [ command: "just e2e" ]
        => [] }
then {
    LdkServerRegtestE2E/run: [ ] }
```

```text
sync LWK_E2E
when {
    Shell/request: [ command: "just lwk_e2e" ]
        => [] }
then {
    LwkLiquidRegtestE2E/run: [ ] }
```
