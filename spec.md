# LN→Liquid Swap Specification

このリポジトリは、LN（BOLT11）支払いと Liquid HTLC（P2WSH）を結合した
LN→Liquid swap の最小実装である。
本書（`spec.md`）は、このリポジトリが満たすべき機能と不変条件を定義する。

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
operational principle
    after request [ command: "just ci" ]
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
operational principle
    after init [ ]
        => [ ]
    then Shell/request [ command: "RUST_LOG=debug cargo run --bin swap_seller -- --help" ]
        => [ ]
```

```text
concept LnLiquidSwap
purpose
    LN の invoice 支払いと Liquid HTLC（P2WSH）を結合した swap を提供する。
    本実装は完全な原子性を提供しない。
state
    proto_file: string
actions
    get_offer [ asset_id: string ]
        => [ price_msat_per_asset_unit: uint64; fee_subsidy_sats: uint64; refund_delta_blocks: uint32; invoice_expiry_secs: uint32; max_min_funding_confs: uint32 ]
        buyer SHOULD call `get_offer` before `create_swap` to discover pricing without funding an HTLC
    create_swap [ asset_id: string; asset_amount: uint64; buyer_claim_address: string; min_funding_confs: uint32; max_total_price_msat: uint64 ]
        => [ swap_id: string; bolt11_invoice: string; payment_hash: string; funding_txid: string; p2wsh_address: string ]
        seller MUST fund the Liquid HTLC before returning `bolt11_invoice`
        seller MUST compute invoice amount as `asset_amount * price_msat_per_asset_unit`
        seller MUST reject if `max_total_price_msat != 0` and invoice amount exceeds `max_total_price_msat`
        buyer MUST verify funding and hash matches before paying `bolt11_invoice`
    get_swap [ swap_id: string ]
        => [ found: boolean ]
operational principle
    after get_offer [ asset_id: "<ASSET_ID>" ]
        => [ price_msat_per_asset_unit: 1000 ]
    then create_swap [ asset_id: "<ASSET_ID>"; asset_amount: 1000; buyer_claim_address: "<ADDR>"; min_funding_confs: 1; max_total_price_msat: 1000000 ]
        => [ swap_id: "<UUID>" ]
    then get_swap [ swap_id: "<UUID>" ]
        => [ found: true ]
```

```text
concept SqliteSwapStore
purpose
    swap の復旧に必要な最小データを SQLite へ永続化する。
state
    store_path: string
actions
    open [ store_path: string ]
        => [ ok: boolean ]
    insert_swap [ swap_id: string ]
        => [ ok: boolean ]
    get_swap [ swap_id: string ]
        => [ found: boolean ]
    update_status [ swap_id: string; status: string ]
        => [ ok: boolean ]
    list_swaps [ ]
        => [ count: uint32 ]
operational principle
    after open [ store_path: "<TMP>/swap_store.sqlite3" ]
        => [ ok: true ]
    then insert_swap [ swap_id: "swap-a" ]
        => [ ok: true ]
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
operational principle
    after fmt_check [ ]
        => [ ok: true ]
    then clippy [ ]
        => [ ok: true ]
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
operational principle
    after run [ ]
        => [ ok: true ]
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
concept LnLiquidSwapE2E
purpose
    LN（bitcoind + ldk-server）と Liquid（elementsd + electrs）を実際に起動し、
    swap の作成・支払い・claim を検証する。
state
    bitcoind: string
    ldk_server: string
    elementsd: string
    electrs_liquid: string
    test_file: string
actions
    run [ ]
        => [ ok: boolean ]
        run `cargo test --test ln_liquid_swap_e2e -- --ignored --nocapture`
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
    swap_proto: string
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
operational principle
    after format_check [ ]
        => [ ok: true ]
    then lint [ ]
        => [ ok: true ]
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
operational principle
    after vale [ ]
        => [ ok: true ]
    then broken_links [ ]
        => [ ok: true ]
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
operational principle
    after lint_markdown [ ]
        => [ ok: true ]
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

```text
sync SWAP_E2E
when {
    Shell/request: [ command: "just swap_e2e" ]
        => [] }
then {
    LnLiquidSwapE2E/run: [ ] }
```
