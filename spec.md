# LN⇄Liquid Swap Specification

This repository is a minimal LN⇄Liquid swap implementation that combines LN (BOLT11) payments and
a Liquid HTLC (P2WSH).
This document (`spec.md`) defines the required behavior and invariants for this repository.

This document follows https://arxiv.org/html/2508.14511v2.
The paper title is "What You See Is What It Does".
This document is structured as Concept Specifications and Synchronizations.

## Concept Specifications

```text
concept Shell
purpose
    Represent command execution by external actors (developers and CI).
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
    Reproduce the development environment with Nix Flakes.
    Manage environment variables with direnv (`.envrc`).
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
    Provide structured logging via `tracing`.
    Allow controlling log verbosity with `RUST_LOG`.
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
    then Shell/request [ command: "RUST_LOG=debug cargo run --bin swap_server -- --help" ]
        => [ ]
```

```text
concept SwapCli
purpose
    Provide a CLI interface to operate the LN⇄Liquid swap.
    The CLI calls the gRPC server.
state
    grpc_url: string
    auth_token: string
actions
    create_quote [ auth_token: string; direction: string; asset_id: string; asset_amount: uint64; min_funding_confs: uint32 ]
        => [ quote_id: string; offer_id: string; total_price_msat: uint64 ]
    get_quote [ auth_token: string; quote_id: string ]
        => [ quote_id: string; offer_id: string; total_price_msat: uint64 ]
    create_swap [ auth_token: string; quote_id: string; buyer_liquid_address: string; buyer_bolt11_invoice: string ]
        => [ swap_id: string; bolt11_invoice: string; payment_hash: string ]
    get_swap [ auth_token: string; swap_id: string ]
        => [ swap_id: string; status: string ]
    create_lightning_payment [ auth_token: string; swap_id: string ]
        => [ payment_id: string; preimage_hex: string ]
    create_asset_claim [ auth_token: string; swap_id: string ]
        => [ claim_txid: string ]
operational principle
    after create_quote [ auth_token: "<SELLER_TOKEN>"; direction: "LN_TO_LIQUID"; asset_id: "<ASSET_ID>"; asset_amount: 1000; min_funding_confs: 1 ]
        => [ quote_id: "<UUID>"; total_price_msat: 1000000 ]
    then create_swap [ auth_token: "<BUYER_TOKEN>"; quote_id: "<UUID>"; buyer_liquid_address: "<BUYER_LIQUID_ADDRESS>"; buyer_bolt11_invoice: "" ]
        => [ swap_id: "<UUID>" ]
    then create_lightning_payment [ auth_token: "<BUYER_TOKEN>"; swap_id: "<UUID>" ]
        => [ payment_id: "<UUID>" ]
    then create_asset_claim [ auth_token: "<BUYER_TOKEN>"; swap_id: "<UUID>" ]
        => [ claim_txid: "<TXID>" ]
```

```text
concept LnLiquidSwap
purpose
    Provide a swap that combines LN invoice payments and a Liquid HTLC (P2WSH).
    This implementation does not provide full atomicity.
    Expose two directions in the API: LN_TO_LIQUID and LIQUID_TO_LN.
    LN_TO_LIQUID is a submarine swap.
    LIQUID_TO_LN is a reverse submarine swap.
    Require authentication and authorization via bearer token.
    Seller can create quotes.
    Buyer can create swaps from quotes.
    Swap execution actions depend on swap direction.
    Buyer and seller actions are represented as a single gRPC server (a single Protobuf service).
    In this minimal setup, the buyer and seller may share the same LN node and Liquid wallet.
state
    proto_file: string
    seller_token: string
    buyer_token: string
actions
    create_quote [ auth_token: string; direction: string; asset_id: string; asset_amount: uint64; min_funding_confs: uint32 ]
        => [ quote_id: string; offer_id: string; total_price_msat: uint64 ]
        server MUST reject if `auth_token` is not `seller_token`
        server MUST reject if `direction` is not supported by the current offer
        seller MUST compute `total_price_msat = asset_amount * price_msat_per_asset_unit`
        seller MUST persist the quote so it can be resolved by `quote_id`
    get_quote [ auth_token: string; quote_id: string ]
        => [ found: boolean ]
        server MUST reject if `auth_token` is not `buyer_token` and not `seller_token`
    create_swap [ auth_token: string; quote_id: string; buyer_liquid_address: string; buyer_bolt11_invoice: string ]
        => [ swap_id: string; bolt11_invoice: string; payment_hash: string; funding_txid: string; p2wsh_address: string ]
        server MUST reject if `auth_token` is not `buyer_token`
        seller MUST resolve `quote_id` to a persisted quote
        seller MUST reject if the current offer differs from the quote (`offer_id` mismatch)
        server MUST set swap direction from the quote direction
        server MUST set swap parties from the swap direction
        server MUST lock the asset output and the LBTC fee subsidy output into the same P2WSH HTLC
        HTLC outputs MUST be explicit (unblinded) in this minimal design
        server MUST build the HTLC witness script so that:
            - claim path requires `Swap.parties.liquid_claimer` signature, and
            - refund path requires `Swap.parties.liquid_refunder` signature.
        server MUST fund the Liquid HTLC before returning `bolt11_invoice`
        for LN_TO_LIQUID:
            server MUST reject if `buyer_bolt11_invoice` is set
            server MUST create a BOLT11 invoice for `Swap.parties.ln_payee`
            server MUST set invoice amount to `Quote.total_price_msat`
        for LIQUID_TO_LN:
            server MUST reject if `buyer_bolt11_invoice` is empty
            server MUST reject if `buyer_bolt11_invoice` has no amount
            server MUST reject if `buyer_bolt11_invoice` amount differs from `Quote.total_price_msat`
            server MUST reject if `buyer_bolt11_invoice` is expired (best-effort)
            server MUST use `buyer_bolt11_invoice` as `Swap.bolt11_invoice`
    get_swap [ auth_token: string; swap_id: string ]
        => [ found: boolean ]
        server MUST reject if `auth_token` is not `buyer_token` and not `seller_token`
    create_lightning_payment [ auth_token: string; swap_id: string ]
        => [ payment_id: string ]
        server MUST reject if `auth_token` is not the token for `Swap.parties.ln_payer`
        ln_payer MUST pay `Swap.bolt11_invoice` via the configured LN node
        ln_payer MUST verify `SHA256(preimage) == Swap.payment_hash` before persisting the payment result
    create_asset_claim [ auth_token: string; swap_id: string ]
        => [ claim_txid: string ]
        server MUST reject if `auth_token` is not the token for `Swap.parties.liquid_claimer`
        liquid_claimer MUST build a claim tx that spends the HTLC with:
            - the preimage, and
            - the liquid_claimer signature (preimage-only spend MUST NOT be allowed)
        liquid_claimer MUST broadcast the claim tx on Liquid
operational principle
    after create_quote [ auth_token: "<SELLER_TOKEN>"; direction: "LN_TO_LIQUID"; asset_id: "<ASSET_ID>"; asset_amount: 1000; min_funding_confs: 1 ]
        => [ quote_id: "<UUID>"; total_price_msat: 1000000 ]
    then create_swap [ auth_token: "<BUYER_TOKEN>"; quote_id: "<UUID>"; buyer_liquid_address: "<BUYER_LIQUID_ADDRESS>"; buyer_bolt11_invoice: "" ]
        => [ swap_id: "<UUID>" ]
    then create_lightning_payment [ auth_token: "<BUYER_TOKEN>"; swap_id: "<UUID>" ]
        => [ payment_id: "<UUID>" ]
    then create_asset_claim [ auth_token: "<BUYER_TOKEN>"; swap_id: "<UUID>" ]
        => [ claim_txid: "<TXID>" ]
    then get_swap [ auth_token: "<BUYER_TOKEN>"; swap_id: "<UUID>" ]
        => [ found: true ]
    ---
    after create_quote [ auth_token: "<SELLER_TOKEN>"; direction: "LIQUID_TO_LN"; asset_id: "<ASSET_ID>"; asset_amount: 1000; min_funding_confs: 1 ]
        => [ quote_id: "<UUID>"; total_price_msat: 1000000 ]
    then create_swap [ auth_token: "<BUYER_TOKEN>"; quote_id: "<UUID>"; buyer_liquid_address: "<BUYER_LIQUID_ADDRESS>"; buyer_bolt11_invoice: "<BUYER_BOLT11_INVOICE>" ]
        => [ swap_id: "<UUID>" ]
    then create_lightning_payment [ auth_token: "<SELLER_TOKEN>"; swap_id: "<UUID>" ]
        => [ payment_id: "<UUID>" ]
    then create_asset_claim [ auth_token: "<SELLER_TOKEN>"; swap_id: "<UUID>" ]
        => [ claim_txid: "<TXID>" ]
    then get_swap [ auth_token: "<SELLER_TOKEN>"; swap_id: "<UUID>" ]
        => [ found: true ]
```

```text
concept LiquidHtlc
purpose
    Provide a Liquid P2WSH HTLC (hashlock + CLTV).
    Embed a hashlock that matches the LN invoice payment hash.
state
    payment_hash: bytes
    claimer_pubkey_hash160: bytes
    refunder_pubkey_hash160: bytes
    refund_lock_height: uint32
actions
    build [ payment_hash: bytes; claimer_pubkey_hash160: bytes; refunder_pubkey_hash160: bytes; refund_lock_height: uint32 ]
        => [ witness_script: bytes; p2wsh_address: string ]
        witness_script MUST use `OP_SHA256` for hashlock
        witness_script MUST use `OP_CHECKLOCKTIMEVERIFY` for timelock (CLTV)
        claim path MUST require liquid_claimer signature (preimage-only spend MUST NOT be allowed)
        refund path MUST require liquid_refunder signature and CLTV
operational principle
    after build [ payment_hash: "<HASH>"; claimer_pubkey_hash160: "<PKH>"; refunder_pubkey_hash160: "<PKH>"; refund_lock_height: 1000 ]
        => [ p2wsh_address: "<ADDR>" ]
```

```text
concept SqliteStore
purpose
    Persist the minimum data required to recover quotes and swaps into SQLite.
state
    store_path: string
actions
    open [ store_path: string ]
        => [ ok: boolean ]
    insert_quote [ quote_id: string ]
        => [ ok: boolean ]
    get_quote [ quote_id: string ]
        => [ found: boolean ]
    insert_swap [ swap_id: string ]
        => [ ok: boolean ]
    get_swap [ swap_id: string ]
        => [ found: boolean ]
    update_swap_status [ swap_id: string; status: string ]
        => [ ok: boolean ]
    upsert_swap_payment [ swap_id: string ]
        => [ ok: boolean ]
    upsert_swap_claim [ swap_id: string ]
        => [ ok: boolean ]
    list_swaps [ ]
        => [ count: uint32 ]
operational principle
    after open [ store_path: "<TMP>/swap_store.sqlite3" ]
        => [ ok: true ]
    then insert_quote [ quote_id: "quote-a" ]
        => [ ok: true ]
    then insert_swap [ swap_id: "swap-a" ]
        => [ ok: true ]
```

```text
concept RustToolchain
purpose
    Provide the primary quality gates for Rust code.
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
    Express representative operations as integration tests (`tests/`).
    Integration tests must not use mocks.
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
    Start LDK Server and bitcoind for real and verify channel creation and BOLT11 payments on
    regtest.
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
    Start LWK and elementsd/electrs for real and verify asset issuance and transfers on Liquid
    regtest.
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
    Start LN (bitcoind + ldk-server) and Liquid (elementsd + electrs) for real and verify swap
    creation, payment, and claim.
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
    Manage Protobuf schemas with Buf.
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
    Manage documentation under `docs/` and validate content and links.
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
    Lint Markdown content with textlint.
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
