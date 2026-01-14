# ExecPlan: LN→Liquid Swap (Quote → CreateSwap)

## Goal

- Let the buyer request a quote via `CreateQuote` and then call `CreateSwap` by specifying the
  returned `quote_id`.
- Let the seller create and return a swap only when the quoted conditions (price/policy) are still
  valid at the time of `CreateSwap`.
- Let the buyer verify the returned swap info and complete the LN payment and Liquid claim.

### Non-goals

- Do not require an external market API for price computation (fixed parameters are fine at first).
- Do not implement gRPC/TLS, authentication/authorization, or safe internet exposure (closed-network
  assumption).
- Do not add mainnet/testnet support (keep regtest assumptions).

## Scope

### In scope

- Protobuf (gRPC):
  - Add `CreateQuote` and `Quote` to `proto/ln_liquid_swap/v1/swap.proto`.
  - Make `CreateSwap` accept `quote_id` (new RPC or request extension).
- Seller implementation:
  - Implement `CreateQuote` in `src/swap/service.rs`.
  - Validate `quote_id` in `CreateSwap` and reject when price/policy differs from the current
    offer.
- Buyer implementation:
  - Update `src/bin/swap_cli.rs` for the quote flow.
- Integration test:
  - Update `tests/ln_liquid_swap_e2e.rs` for the quote flow.
- Spec and docs:
  - Add quote-flow actions to `spec.md` and update the representative E2E scenario.
  - Update `docs/swap/ln-liquid-swap.mdx` and the runbook (because steps change).

### Out of scope

- Keep the HTLC witness script format (`OP_SHA256 + OP_CHECKLOCKTIMEVERIFY`).
- Keep explicit (unblinded) HTLC outputs.

## Milestones

### M1: Add Quote to Protobuf

#### Observable outcomes

- `nix develop -c just proto_fmt proto_lint` succeeds.
- `cargo test --all` compiles through build time (including tonic codegen).

#### Work

- Add `CreateQuoteRequest` / `Quote`.
- Add `quote_id` to `CreateSwap` input.
- Document representative error patterns (quote mismatch, expiry, etc.) in `.proto` comments.

### M2: Seller can issue and validate quotes

#### Observable outcomes

- `CreateQuote` returns `total_price_msat`.
- When the seller receives `CreateSwap(quote_id)`, it can reject if current price/policy does not
  match the quote.

#### Work

- Define `Offer` and `offer_id` (snapshot id of price/policy) on the seller side.
- In `CreateQuote`, generate `Quote(quote_id, offer_id, total_price_msat, ...)`.
- At the beginning of `CreateSwap`, resolve `quote_id` and validate `offer_id` match.

### M3: Update the CLI for the quote flow

#### Observable outcomes

- The buyer can complete `CreateQuote` → `CreateSwap` → pay → claim.
- Before paying on LN, the buyer can verify "invoice amount == quote.total_price_msat".

#### Work

- Make `swap_cli` call `CreateQuote` and keep the `Quote`.
- Call `CreateSwap` using `quote_id`.
- Before payment, validate quote/swap consistency (invoice amount, asset_amount, etc.).

### M4: Update Swap E2E integration test for the quote flow

#### Observable outcomes

- `cargo test --test ln_liquid_swap_e2e -- --ignored --nocapture` succeeds.

#### Work

- Replace `CreateSwap` calls in `tests/ln_liquid_swap_e2e.rs` with the `CreateQuote` flow.
- If possible, also observe "if seller changes price/policy after quoting, `CreateSwap` fails".

### M5: Sync spec and docs

#### Observable outcomes

- `nix develop -c just ci` succeeds.

#### Work

- Add `create_quote` to the `LnLiquidSwap` concept in `spec.md`.
- Update `docs/swap/ln-liquid-swap.mdx` and the runbook steps.

## Tests

- `nix develop -c just ci`
- `nix develop -c just swap_e2e`
  - `tests/ln_liquid_swap_e2e.rs` (ignored, no mocks)

## Decisions / Risks

### Key decisions

- Decide how to implement `quote_id`.
  - Examples: persist in SQLite (quotes table) vs signed tokens (stateless).
  - Rationale: affects restart behavior and the persistence vs stateless trade-off.
- Decide how to handle backwards compatibility.
  - Examples: keep `GetOffer` / `max_total_price_msat` or make a breaking change.

### Known risks and mitigations

- Risk: unlimited quote issuance exhausts memory/DB.
  - Mitigation: add quote TTL and caps (max inflight quotes).
- Risk: insufficient consistency checks cause buyer overpayment.
  - Mitigation: require buyer-side validation "invoice amount == quote.total_price_msat".

## Progress

- 2026-01-14: Created this ExecPlan for the Quote → CreateSwap flow.
