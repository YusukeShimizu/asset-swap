# LN⇄Liquid Swap (gRPC)

This repository contains a minimal LN⇄Liquid swap implementation that combines Lightning payments
(BOLT11) and a Liquid HTLC (P2WSH).

- `LN_TO_LIQUID` (submarine swap): the seller funds a Liquid HTLC and creates an invoice; the buyer pays and claims.
- `LIQUID_TO_LN` (reverse submarine swap): the buyer funds a Liquid HTLC and creates an invoice; the seller pays and claims.
- The current `swap_server` implementation supports `LN_TO_LIQUID` (submarine swap) only.
- Pricing is decided by the seller; the seller creates a quote via `CreateQuote` and shares `quote_id`.
  The buyer calls `CreateSwap(quote_id)`. The server rejects if the conditions changed after quoting.

This project does not provide full atomicity. It is intended for regtest / validation
environments.

See `docs/` (Mintlify) for details.

## Quick start

If you use direnv:

```sh
direnv allow
just ci
```

If you do not use direnv:

```sh
nix develop -c just ci
```

## Binaries

- gRPC server: `swap_server`
- CLI: `swap_cli`

See `docs/swap/ln-liquid-swap.mdx` for examples.

## Logging

Control log verbosity with `RUST_LOG`.

```sh
echo 'export RUST_LOG=debug' > .envrc.local
direnv allow
nix develop -c cargo run --bin swap_server -- --help
```

## Protobuf (Buf)

Schemas live under `proto/`.

- API: `proto/ln_liquid_swap/v1/swap.proto`
- Format/Lint:

```sh
buf format -w
buf lint
```

## E2E (regtest)

E2E tests are `#[ignore]` and require external processes (run via `nix develop`).

- LDK Server (Bitcoin regtest): `nix develop -c just e2e`
- LWK (Liquid regtest): `nix develop -c just lwk_e2e`
- LN→Liquid swap: `nix develop -c just swap_e2e`

To keep logs and working directories on failure, use `just e2e_keep` / `just lwk_e2e_keep`.

## Documentation (Mintlify)

Documentation lives under `docs/`.

- Config: `docs/docs.json`
- Vale: `docs/.vale.ini`

CI runs the quality gate via `nix develop -c just ci`.
