# ExecPlan: LN→Liquid Swap (production-lite / gRPC)

## Goal

- Harden the minimal swap described in `spec.md` into a production-lite design for **closed-network,
  single-process operation**.
- Concretely, make the following hold:
  - idempotent `CreateSwap`
  - lightweight DB persistence
  - prevent double-allocation of inventory
  - track claim/refund state transitions

### Non-goals

- Do not implement gRPC/TLS or authentication/authorization (assume closed-network operation).
- Do not handle confidential transaction blinding (keep explicit HTLC outputs).
- Do not make horizontal scale work (no shared inventory across multiple instances).

## Scope

### In scope

- Protobuf (gRPC):
  - extend `proto/ln_liquid_swap/v1/swap.proto` request/response messages (idempotency keys, etc.).
- Persistence:
  - replace `src/swap/store.rs` with a lightweight DB (for example: SQLite WAL).
  - add schema for swaps, request ids, and inventory reservation.
- Seller server:
  - make `CreateSwap` in `src/swap/service.rs` idempotent and clarify state transitions.
  - add watcher workers (claim/refund/funding) to `src/bin/swap_server.rs`.
- Liquid:
  - retrieve the required information from Electrum to confirm funding and track spends (add
    dependencies if needed).
- Documentation:
  - update `docs/swap/ln-liquid-swap.mdx` with production-lite notes and new fields.

### Out of scope

- Do not provide external APIs other than gRPC (do not add HTTP JSON).
- Keep the HTLC script format (`OP_SHA256 + OP_CHECKLOCKTIMEVERIFY`).
- Do not weaken the buyer-side safety requirements for verification before paying on LN.

## Milestones

### M1: Introduce SQLite persistence (replace SwapStore)

#### Observable outcomes

- `cargo test --all` passes.
- After process restart, `GetSwap` can return the same `swap_id` (persistent store works).

#### Work

- Replace `src/swap/store.rs` with `SqliteSwapStore`.
  - swaps table (status, funding outpoint, witness script, etc.)
  - request_id table (idempotency key → swap_id)
  - inventory reservation table (reserved utxo / amount)
- Add migrations (auto-apply on startup).
- Add store integration tests (use SQLite as a real DB).

### M2: Make `CreateSwap` idempotent (request_id)

#### Observable outcomes

- Calling `CreateSwap` multiple times with the same `request_id` returns the same `swap_id` and the
  same `funding_txid`.
- Retries do not double-lock inventory.

#### Work

- Add `request_id` (UUID string) to `proto/ln_liquid_swap/v1/swap.proto`.
  - Add Protovalidate constraints.
  - Document idempotency semantics and representative errors in comments.
- In `src/swap/service.rs`, implement:
  - if `request_id` exists, return the existing swap (idempotent).
  - otherwise create a new swap and persist both request_id and swap in one transaction.
- Add an idempotency observation to the swap E2E test (ignored) in `tests/ln_liquid_swap_e2e.rs`.

### M3: Prevent double allocation of inventory (single-process assumption)

#### Observable outcomes

- Even with concurrent `CreateSwap` calls, insufficient inventory fails safely and does not produce
  double funding.

#### Work

- Serialize wallet operations on the server (keep the existing mutex and also rely on DB
  consistency).
- Record reservations in the DB and always release them on creation failure.
- If UTXO-level reservation is difficult, start with a conservative safe approach:
  - single-process + wallet serialization + a cap on reserved amounts
  - optionally introduce additional constraints (for example: max inflight swaps) as config.

### M4: Track claim/refund state (watcher)

#### Observable outcomes

- A claimed swap transitions to `Claimed` (detect that the HTLC outpoint was spent by the buyer).
- A successfully refunded swap transitions to `Refunded`.

#### Work

- Add a Liquid watcher:
  - check HTLC scriptPubKey unspent status and detect when the outpoint is spent.
  - if needed, use `electrum-client` directly and implement `scripthash.listunspent` equivalent.
- Upgrade the refund worker to manage:
  - timelock reached → still unspent → refund attempt → confirmation.
- Restrict state transitions to `Created → Funded → Claimed/Refunded` and reject invalid transitions.

### M5: Documentation and operational knobs

#### Observable outcomes

- Users can follow the docs to create swaps and understand the buyer verification checklist.

#### Work

- Add to `docs/swap/ln-liquid-swap.mdx`:
  - how to use `request_id` for retries
  - `CreateSwap` long-blocking behavior (`min_funding_confs`) and recommended values
  - production-lite assumptions (closed network, single instance, explicit outputs)
- Organize server configuration (timeouts, max inflight, poll intervals).

## Tests

- Keep `just ci` passing.
- Add store integration tests using SQLite (no mocks).
- For swaps, add to `tests/ln_liquid_swap_e2e.rs` (ignored):
  - `CreateSwap` idempotency observation for the same `request_id`
  - if a watcher is added, observe that seller-side `GetSwap` transitions to `Claimed` after claim.

## Decisions / Risks

### Key decisions

- Do not implement TLS/authn/authz.
  - Rationale: assume closed, trusted networks.
  - Risk: if exposed, assets can be drained or tampered with.
- Use a lightweight DB (SQLite WAL).
  - Rationale: fits single-process operation and is easy to adopt.
- Keep explicit (unblinded) HTLC outputs (do not handle blinding).
  - Rationale: postpone persistence/recovery complexity.

### Known risks and mitigations

- Risk: spend tracking may be difficult with only an Electrum backend.
  - Mitigation: assume a backend that provides `listunspent`-equivalent APIs.
- Risk: waiting for confirmations makes `CreateSwap` long-running.
  - Mitigation: cap `min_funding_confs`, add server-side timeouts, and ensure clients can re-fetch
    via `GetSwap`.
- Risk: reorgs can reduce confirmations.
  - Mitigation: make state transitions re-evaluable and define a fixed operational policy for what
    "Funded" means.

## Progress

- 2026-01-13: Created this ExecPlan with production-lite assumptions (no TLS/auth, SQLite, no
  blinding).
- 2026-01-14: Introduced `SqliteSwapStore` and replaced usage in the seller/server. Added an
  integration test for the SQLite store.
