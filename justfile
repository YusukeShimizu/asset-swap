#!/usr/bin/env bash

fmt:
    cargo fmt --all -- --check

proto_fmt:
    buf format -d --exit-code

proto_lint:
    buf lint

clippy:
    cargo clippy --all-targets --all-features -- -D warnings

test:
    cargo test --all

textlint:
    textlint $(git ls-files '*.md' | grep -v '^\.codex/')

docs_links:
    cd docs && PUPPETEER_SKIP_DOWNLOAD=1 PUPPETEER_SKIP_CHROMIUM_DOWNLOAD=1 npx --yes mint@4.2.269 broken-links

docs_vale:
    cd docs && vale sync --config .vale.ini
    cd docs && vale --config .vale.ini --glob='*.mdx' .

ci: fmt proto_fmt proto_lint clippy test textlint docs_vale docs_links

e2e:
    cargo test --test ldk_server_regtest_e2e -- --ignored --nocapture

e2e_keep:
    KEEP_LDK_E2E_ARTIFACTS=1 cargo test --test ldk_server_regtest_e2e -- --ignored --nocapture

lwk_e2e:
    cargo test --test lwk_liquid_regtest_e2e -- --ignored --nocapture

lwk_e2e_keep:
    KEEP_LWK_E2E_ARTIFACTS=1 cargo test --test lwk_liquid_regtest_e2e -- --ignored --nocapture

swap_e2e:
    cargo test --test ln_liquid_swap_e2e -- --ignored --nocapture
