# ExecPlan: Submarine Swap + Reverse Submarine Swap (LN⇄Liquid)

## Goal

- Implement both swap directions in `proto/ln_liquid_swap/v1/swap.proto`:
  - `LN_TO_LIQUID` (submarine swap): LN payment → Liquid HTLC claim.
  - `LIQUID_TO_LN` (reverse submarine swap): Liquid HTLC funding → LN payment.
- Enforce authentication and role-based authorization:
  - Seller: `CreateQuote`
  - Buyer: `CreateSwap`
  - Execution RPCs are authorized by `Swap.parties`.
- Make the implementation reproducible via `nix develop -c just ci` and at least one ignored (no
  mocks) integration test that exercises each direction end-to-end on regtest.

### Non-goals

- Do not maintain wire/backward compatibility with any previous v1 schema versions.
- Do not design production-grade operations (key management, TLS termination, and SLA).
- Do not cover production monitoring or scalability.
- Do not implement privacy hardening (explicit/unblinded HTLC outputs remain OK for this repo).
- Do not require HODL invoices or any custom LN-side HTLC conditions (keep standard BOLT11).

## Scope

### In scope

- Protobuf:
  - Keep `proto/ln_liquid_swap/v1/swap.proto` as the API surface.
  - Fully implement both `SwapDirection` values with correct `Swap.parties` semantics.
- Server implementation:
  - Persist `direction` and the minimum extra data needed to execute both directions.
  - Implement `CreateSwap` branching by direction:
    - Submarine: server-created invoice
    - Reverse: buyer-provided invoice
  - Ensure refund worker refunds to the correct party for each direction.
- CLI and docs:
  - Keep `swap_cli` aligned with the final API.
  - Update `docs/` + `spec.md` to remove “LN_TO_LIQUID only” limitations after implementation.
- Tests:
  - Add/extend integration tests (no mocks) to cover both directions.

### Out of scope

- Do not add HTTP JSON transcoding.
- Do not add a new API version (`v2/`).
- Do not support multiple assets/offers simultaneously (keep “one configured sell_asset_id” as the
  minimal starting point).

## Milestones

### M1: Persist swap direction and swap inputs

#### Observable outcomes

- `CreateQuote` returns the requested `direction` and derived `parties`.
- `CreateSwap` returns `Swap.direction` and `Swap.parties` based on the quote.
- `nix develop -c just ci` succeeds.

#### Work

- Update `QuoteRecord` / `SwapRecord` and SQLite schema:
  - Store `direction` in `quotes` and `swaps`.
  - Store `buyer_liquid_address` in `swaps` (currently tracked as an internal “buyer claim address”
    concept).
  - Remove any quote-time “buyer address” state from the quote record (buyer provides it at swap
    creation).
- Make `GetQuote` / `GetSwap` return consistent direction/parties.

### M2: Generalize Liquid HTLC script + spend builders

#### Observable outcomes

- The HTLC witness script is expressed in terms of:
  - `liquid_claimer` key hash for the preimage path, and
  - `liquid_refunder` key hash for the CLTV refund path.
- Unit/integration tests can build both:
  - a claim tx signed by the claimer, and
  - a refund tx signed by the refunder.

#### Work

- Refactor `src/liquid/htlc.rs`:
  - Rename “buyer/seller pubkey hash” terminology to “claimer/refunder pubkey hash”.
  - Make `claim_tx_from_witness_script` and `refund_tx_from_witness_script` accept the spender’s
    receive address + secret key (role-agnostic).
- Update call sites:
  - `CreateAssetClaim` should sign with `Swap.parties.liquid_claimer`.
  - Refund worker should sign with `Swap.parties.liquid_refunder`.

### M3: Complete `LN_TO_LIQUID` (submarine swap) implementation

#### Observable outcomes

- A buyer can run:
  - `CreateQuote(direction=LN_TO_LIQUID)` (seller token)
  - `CreateSwap(quote_id, buyer_liquid_address)` (buyer token)
  - `CreateLightningPayment` (buyer token)
  - `CreateAssetClaim` (buyer token)
- An ignored E2E test exists (no mocks) and succeeds when run with required binaries:
  - `nix develop -c just swap_e2e`

#### Work

- In `CreateSwap` for `LN_TO_LIQUID`:
  - Require `buyer_bolt11_invoice` to be empty.
  - Create the BOLT11 invoice server-side and use its payment hash for the HTLC script.
  - Build the HTLC script with:
    - claimer = buyer (buyer_liquid_address),
    - refunder = seller (server-configured seller key).
- Ensure safety checks remain:
  - offer snapshot consistency (`offer_id`)
  - funding confirmation waiting

### M4: Implement `LIQUID_TO_LN` (reverse submarine swap)

#### Observable outcomes

- A buyer can create a reverse swap by supplying:
  - `buyer_liquid_address` (refund key), and
  - `buyer_bolt11_invoice` (to be paid by the seller).
- Seller can complete the swap by calling:
  - `CreateLightningPayment` (seller token), then
  - `CreateAssetClaim` (seller token).
- Refund worker refunds to the buyer after expiry when the invoice is not paid.

#### Work

- In `CreateSwap` for `LIQUID_TO_LN`:
  - Require `buyer_bolt11_invoice` to be non-empty.
  - Parse the invoice:
    - extract `payment_hash`,
    - validate invoice amount is specified and equals `Quote.total_price_msat`,
    - validate the invoice is not already expired (best-effort).
  - Build the HTLC script with:
    - claimer = seller (server-configured seller key),
    - refunder = buyer (buyer_liquid_address).
- Update auth checks to use the persisted direction:
  - `CreateLightningPayment` requires `Swap.parties.ln_payer`.
  - `CreateAssetClaim` requires `Swap.parties.liquid_claimer`.
- Add an ignored E2E test for reverse swaps (no mocks).

### M5: Update spec/docs for the fully implemented protocol

#### Observable outcomes

- `spec.md` describes both directions without “not implemented” notes.
- `docs/swap/ln-liquid-swap.mdx` includes example sequences for both directions.
- `nix develop -c just ci` succeeds (textlint/vale/broken-links included).

#### Work

- Update `spec.md`:
  - For submarine swaps, server creates the invoice.
  - For reverse swaps, buyer provides the invoice and the server validates it.
- Update docs to include:
  - who calls which RPC per direction (based on `Swap.parties`),
  - CLI/grpcurl examples for reverse swaps.

## Tests

- Primary gate: `nix develop -c just ci`
- E2E (ignored, no mocks):
  - `nix develop -c just swap_e2e` (submarine swap)
  - Add a new ignored test (or extend existing) for reverse submarine swaps and document how to run
    it.

## Decisions / Risks

### Key decisions

- **LN backend topology**
  - Option A (minimal): keep a single configured LN backend and accept limitations (e.g. “paying
    your own invoice” can fail depending on the backend).
  - Option B (protocol-faithful): configure separate LN backends (buyer vs seller) or change the
    API so the payer submits payment proofs/preimages instead of the server paying.
- **Key custody**
  - Current minimal server holds Liquid signing keys for both roles (by key index).
  - A more faithful submarine swap design would have the external party sign and broadcast claim /
    refund transactions themselves (requires API redesign).
- **Storage compatibility**
  - We intentionally allow breaking SQLite schema and require wiping `store.sqlite3` for major
    changes.

### Risks and mitigations

- Invoice parsing/validation edge cases (amountless invoices, expiry):
  - Mitigation: reject amountless invoices for reverse swaps; validate amount and expiry.
- Refund worker correctness across directions:
  - Mitigation: derive refund role from persisted direction and test both refund paths.

## Progress

- 2026-01-15: Created plan. Current repo state: v1 schema models both submarine + reverse submarine,
  but server implementation supports `LN_TO_LIQUID` only (reverse returns `UNIMPLEMENTED`).
