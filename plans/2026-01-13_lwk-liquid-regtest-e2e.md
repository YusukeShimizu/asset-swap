# ExecPlan: LWK Liquid regtest E2E integration test

## Goal

- Add an integration test on Liquid regtest using LWK.
- The test should cover: create wallet → receive/sync LBTC → issue asset → send asset → verify
  balances.
- Make external process logs and test observation points explicit so failures can be diagnosed.

### Non-goals

- Do not target mainnet/testnet.
- Do not require pegin.
- Do not integrate with an asset registry.

## Scope

### In scope

- Add a Liquid regtest E2E scenario under `tests/`.
- Add LWK-related crates under `dev-dependencies` in `Cargo.toml`.
- Add definitions to `flake.nix` to provide `elementsd` and liquid-enabled `electrs`.
- Add `justfile` recipes to run the E2E tests.

### Out of scope

- Do not add Liquid features to production code under `src/`.
- Do not add these E2E tests to `just ci`.

## Milestones

### M1: Establish the runtime environment (Nix)

#### Observable outcomes

- `nix develop -c elementsd --version` succeeds.
- `nix develop -c electrs --help` succeeds.

#### Work

- Add `elementsd` to `flake.nix`.
- Add liquid-enabled `electrs` to `flake.nix`.
  - Prefer using LWK's `electrs-flake` `blockstream-electrs-liquid`.
  - Alternatively, build a liquid-enabled `electrs` pinned to a `rev`.
- If using `lwk_test_util::TestEnvBuilder`, set the following env vars in the dev shell:
  - `ELEMENTSD_EXEC`
  - `ELECTRS_LIQUID_EXEC`

### M2: Implement wallet creation and sync in tests

#### Observable outcomes

- Create an issuer wallet and a receiver wallet.
- After `Sync`, observe the tip height.

#### Work

- Implement `LwkWalletFixture` under `tests/support/`.
- Centralize `SwSigner` and descriptor creation in one place.

### M3: Receive LBTC and sync

#### Observable outcomes

- The issuer wallet balance for the policy asset increases.

#### Work

- Implement `LiquidRegtestEnv` under `tests/support/`.
  - Start `elementsd` and `electrs`.
  - Provide block generation and sends.
- Prepare LBTC by sweeping the initial coins from `elementsd`.

### M4: Exercise asset issuance and send

#### Observable outcomes

- Obtain `asset_id` and `reissuance_token_id`.
- The issuer wallet balances for the asset and token increase.
- The receiver wallet balance for the issued asset increases.

#### Work

- Build/sign/broadcast the issuance transaction from the issuer wallet.
- Send the issued asset from the issuer wallet to the receiver wallet address.

### M5: Add balance assertions

#### Observable outcomes

- Verify balances on both issuer and receiver wallets.

#### Work

- Verify policy asset, issued asset, and reissuance token separately.
- Verify via before/after deltas.

### M6: Make the execution path ergonomic

#### Observable outcomes

- Run tests via `nix develop -c just lwk_e2e`.
- On failure, preserve logs and rerun.

#### Work

- Mark the test `#[ignore]` and run via a dedicated `just` recipe.
- Keep the working directory when `KEEP_LWK_E2E_ARTIFACTS=1` is set.

## Tests

### Integration test to add (proposal)

- `tests/lwk_liquid_regtest_e2e.rs`
  - Single responsibility: create wallet → receive/sync → issuance → send → balance verification.

### Test prerequisites

- `elementsd` and liquid-enabled `electrs` are available.
- Use Electrum as the backend.

## Decisions / Risks

### Key decisions

- Avoid pegin.
  - Rationale: setup becomes heavy.
- Use `electrs` as the indexer.
  - Rationale: LWK's Electrum backend is mature.

### Known risks and mitigations

- Risk: waiting for indexer sync is flaky.
  - Mitigation: poll tip height with deadlines.
- Risk: confidential transactions are harder to diagnose with external tools.
  - Mitigation: print transfer amounts and balance deltas inside the test.

## Progress

- 2026-01-13: Created this ExecPlan (implementation not started at that time).
- 2026-01-13: Added `elementsd` and liquid-enabled `electrs` to `flake.nix`.
- 2026-01-13: Added `LiquidRegtestEnv` and `LwkWalletFixture` under `tests/support/`.
- 2026-01-13: Added `tests/lwk_liquid_regtest_e2e.rs` and `just lwk_e2e`.
