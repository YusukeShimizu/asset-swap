# ExecPlan: LN→Liquid Swap (gRPC / P2WSH HTLC)

## Goal

- Implement a minimal swap where a buyer pays over Lightning and receives a Liquid asset, using a
  gRPC-only API.
- On Liquid, lock the asset in a P2WSH HTLC (`OP_SHA256` + `OP_CHECKLOCKTIMEVERIFY`). The buyer
  claims it with the preimage obtained after a successful LN payment.
- Add an integration test (no mocks) that reproduces: create swap → LN payment → Liquid claim →
  balance increase.

### Non-goals

- Do not design production operations (key management, SLA, monitoring, scaling) for mainnet/testnet.
- Do not implement confidential transaction blinding as the general solution (use explicit HTLC
  outputs for the minimal build).
- Do not require HODL invoices or custom LN-side HTLC conditions (use a standard BOLT11 payment).

## Scope

### In scope

- Add a swap gRPC schema under `proto/`.
- Add a gRPC server (seller) and a CLI (buyer) under `src/bin/`.
- Add implementations under `src/` for:
  - the HTLC script, and
  - claim/refund transaction construction.
- Add an E2E test under `tests/` that starts LN (regtest) + Liquid (liquidregtest) processes.
- If needed, add a `justfile` recipe to run the swap E2E.
- If needed, add representative E2E concept/actions to `spec.md`.

### Out of scope

- Do not delete or break existing template features (the `hello` CLI, existing E2E tests, etc.).
- Do not implement pricing/market APIs (fixed parameters are fine for the minimal build).

## Milestones

### M1: Protobuf and gRPC server skeleton

#### Observable outcomes

- `buf lint` succeeds.
- `cargo test --all` compiles through build time (including gRPC codegen).

#### Work

- Add `proto/ln_liquid_swap/v1/swap.proto` (minimal Create/Get).
- Introduce `tonic-build` and generate code for the swap proto only.

### M2: Liquid HTLC and funding

#### Observable outcomes

- When the seller receives `CreateSwap`, it can generate an HTLC witness script.
- The seller can build/sign/broadcast the funding transaction.

#### Work

- Implement `LiquidHtlc` (witness script, P2WSH address, spend conditions).
- Using LWK, fund the HTLC with explicit outputs:
  - asset output + fee subsidy (LBTC) output.
- Persist the minimal data required to recover swaps in `SwapStore`.

### M3: LN payment and Liquid claim (buyer)

#### Observable outcomes

- The buyer can pay the invoice and obtain the `preimage`.
- The buyer can build and broadcast the claim transaction that spends the HTLC with the preimage.

#### Work

- Use `ldk-server` `Bolt11Send` and `ListPayments` to observe the payment result.
- Compute `SHA256(preimage)` and verify it matches `payment_hash`.
- Build the claim transaction and attach the P2WSH (claim) witness per input.

### M4: Swap E2E integration test

#### Observable outcomes

- `cargo test --test ln_liquid_swap_e2e -- --ignored --nocapture` succeeds.
- The test observes:
  - LN payment status is `Succeeded`.
  - The buyer's Liquid-side asset balance increases.

#### Work

- Start `bitcoind` + `ldk-server` (2 nodes) and open a channel.
- Start `elementsd` + `electrs-liquid`, issue an asset, and prepare seller inventory.
- Start the gRPC server and run: `CreateSwap` → pay → claim.

### M5: Minimal refund handling

#### Observable outcomes

- After `refund_lock_height`, the seller can broadcast the refund transaction.

#### Work

- Build the refund transaction and set `nLockTime` and `sequence` to satisfy CLTV.
- Make it observable when refund fails because the HTLC was already claimed.

## Tests

- `tests/ln_liquid_swap_e2e.rs`
  - Dependencies:
    - `tests/support/bitcoind.rs`
    - `tests/support/ldk_server.rs`
    - `tests/support/lwk_env.rs`
    - `tests/support/lwk_wallet.rs`
  - No mocks.
  - Mark as `#[ignore]` and run inside `nix develop`.

## Decisions / Risks

### Key decisions

- External API is gRPC only.
  - Rationale: avoid dual implementations for JSON/HTTP and keep the interface stable.
- HTLC outputs are explicit (unblinded).
  - Rationale: minimize blinding-factor persistence/recovery work and get claim/refund working first.

### Known risks and mitigations

- Risk: gRPC codegen breaks due to `googleapis` dependency drift.
  - Mitigation: keep swap proto self-contained (do not import `google.api.http`, etc.).
- Risk: LN + Liquid E2E tests are flaky due to many processes.
  - Mitigation: make all waits deadline-based and print log paths on failure.
- Risk: refund/claim tx construction fails due to Elements specifics.
  - Mitigation: use Elements `SighashCache` and `TxOut::new_fee`, and validate with minimal explicit
    transactions.

## Progress

- 2026-01-13: Created this ExecPlan.
