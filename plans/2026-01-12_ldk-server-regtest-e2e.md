# ExecPlan: LDK Server regtest E2E integration test

## Goal

- Start LDK Server (`ldk-server` daemon) as real processes inside the test. On regtest, add an
  integration test that automatically completes "open channel → BOLT11 invoice → payment"
  end-to-end.
- Make external process logs and test observation points explicit so failures can be diagnosed.

### Non-goals

- Do not target mainnet/testnet.
- Do not cover advanced flows like LSPS/JIT. Start with a normal channel + BOLT11.
- Do not cover multi-hop routing. Start with a single hop.

## Scope

### In scope

- Add an E2E scenario under `tests/`.
- Add the minimum test dependencies under `dev-dependencies` in `Cargo.toml`.
- Add definitions to `flake.nix` to provide `bitcoind` and `ldk-server` binaries in the dev shell.
- If needed, add recipes to `justfile` to run the E2E tests.
- If needed, add new representative E2E concept/actions to `spec.md`.

### Out of scope

- Do not add Lightning implementation to production code under `src/` (focus on the test harness).
- Do not add Docker-based instructions (prefer Nix Flakes reproducibility).

## Milestones

### M1: Establish the runtime environment (Nix)

#### Observable outcomes

- `nix develop -c bitcoind --version` succeeds.
- `nix develop -c ldk-server --help` (or equivalent) succeeds.

#### Work

- Add Bitcoin Core (`bitcoind`) to `flake.nix`.
- Add LDK Server (`ldk-server`) to `flake.nix`.
  - Preferred: build upstream `lightningdevkit/ldk-server` pinned to a `rev` via
    `pkgs.rustPlatform.buildRustPackage`.
  - Alternative: manage `cargo install --git ... --rev ...` via `just` (less reproducible than a
    Nix build).
- If CI runtime is too heavy, design the test so it can be introduced gradually via `#[ignore]`
  (decide in M4).

### M2: Start bitcoind (regtest) from tests

#### Observable outcomes

- Start `bitcoind` from an integration test and communicate with its RPC.
- Generate blocks with `generatetoaddress` and observe `getblockcount` increasing.

#### Work (test support)

- Implement `BitcoindProcess` under `tests/support/` (or `tests/support.rs`).
  - Create a `datadir` with `tempfile`.
  - Generate `bitcoin.conf` with the minimal required settings:
    - `regtest=1`
    - `server=1`
    - `rpcuser` and `rpcpassword`
    - `rpcport` and `port`
    - `fallbackfee`
  - Start via `std::process::Command` and poll readiness via `bitcoincore_rpc`.
  - Kill on `Drop`, and `wait` if needed.
- Add `bitcoincore-rpc` to `dev-dependencies`.

#### Design notes

- Use a basic coin generation pattern: create wallet → mine 101 blocks → after maturity, send
  funds.
- Port collisions are possible. Choose random ports and retry on startup failure.

### M3: Start ldk-server (Alice/Bob) from tests

#### Observable outcomes

- Start two `ldk-server` instances on separate ports.
- `GetNodeInfo` succeeds for both and `node_id` can be obtained.

#### Work (test support)

- Implement `LdkServerProcess` under `tests/support/`.
  - Separate `storage.disk.dir_path` and log files per node.
  - Allocate non-conflicting ports for `node.listening_address` and `node.rest_service_address`.
  - Align bitcoind RPC config with the `BitcoindProcess` from M2.
  - For readiness, repeatedly call `GetNodeInfo` via `ldk-server-client` (or `reqwest` + `prost`)
    until the node becomes ready.
- For the Rust API client, prefer upstream `ldk-server-client` as a `dev-dependency`.
  - Pin the git `rev`.
  - Alternative: depend on `ldk-server-protos` and implement Protobuf-over-HTTP POST directly with
    `reqwest` using `application/octet-stream`.

### M4: E2E scenario (open channel → invoice → payment) as a single integration test

#### Observable outcomes

- `cargo test --test ldk_server_regtest_e2e` (tentative) succeeds.
- The test observes all of the following:
  - The channel becomes `is_usable=true` on both nodes.
  - The payment is observed on the sender as `OUTBOUND + SUCCEEDED`.
  - The payment is observed on the receiver as `INBOUND + SUCCEEDED`.

#### Minimal scenario

1. Start bitcoind regtest (M2).
2. Start two `ldk-server` instances (Alice/Bob, M3).
3. Get Alice's onchain address via `OnchainReceive`.
4. Send funds from bitcoind to Alice and mine 1 block to confirm.
5. Use Bob's `node_id` and `listening_address` to call `OpenChannel` from Alice to Bob.
6. Poll `ListChannels` while mining confirmations until both sides show `is_usable=true`.
7. Bob creates an invoice via `Bolt11Receive`.
8. Alice pays via `Bolt11Send`.
9. Poll `ListPayments` until `SUCCEEDED` is observed on both nodes.

#### Stabilization points

- All waits should be deadline-based polling (for example: 60–120 seconds) and print log paths on
  failure.
- The test must own regtest mining (do not rely on external chain state).

### M5: Make the execution path ergonomic (CI/local)

#### Observable outcomes

- `nix develop -c cargo test --test ldk_server_regtest_e2e -- --nocapture` runs reproducibly.
- On failure, you can inspect `bitcoind` / `ldk-server` logs.

#### Work

- If runtime is acceptable, include it in `just ci`.
- If heavy, choose one:
  - Keep the test as `#[ignore]` and run via `just e2e` (new recipe).
  - Run only when an env var (for example: `RUN_LDK_E2E=1`) is set.

## Tests

### Integration test to add (proposal)

- `tests/ldk_server_regtest_e2e.rs`
  - Single responsibility: start two ldk-server nodes and complete open channel → invoice →
    payment.
  - Dependencies: `BitcoindProcess`, `LdkServerProcess`, `wait_for` utility.

### Support code to implement (proposal)

- `tests/support/bitcoind.rs`: start bitcoind and provide RPC operations (mining, sending).
- `tests/support/ldk_server.rs`: start ldk-server and provide an API client.
- `tests/support/wait.rs`: deadline-based polling (exponential backoff + observation logs).

## Decisions / Risks

### Key decisions

- Prefer `ldk-server-client` for API operations.
  - Rationale: it already implements Protobuf-over-HTTP POST and error decoding, reducing test-side
    complexity.
- Use only two ldk-server nodes (Alice/Bob).
  - Rationale: to close "open channel → payment" with minimum moving parts, avoid introducing other
    LN implementations (LND/CLN).

### Known risks and mitigations

- Risk: sync/channel confirmation waits are flaky.
  - Mitigation: observe `GetNodeInfo.current_best_block` and `ListChannels.is_usable` with
    deadlines.
- Risk: port collisions cause startup failures.
  - Mitigation: random ports + retry.
- Risk: bitcoind fee estimation fails and blocks sends/channel opens.
  - Mitigation: set `fallbackfee`.
- Risk: upstream `ldk-server` API introduces breaking changes.
  - Mitigation: pin `rev` in both Nix and Cargo.

## Progress

- 2026-01-12: Created this ExecPlan.
- 2026-01-12: Added `bitcoin` and `ldk-server` to `flake.nix`.
- 2026-01-12: Implemented `tests/ldk_server_regtest_e2e.rs` and `tests/support/`.
- 2026-01-12: Added `just e2e` and `just e2e_keep`.
