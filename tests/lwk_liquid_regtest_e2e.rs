mod support {
    pub mod lwk_env;
    pub mod lwk_wallet;
}

use anyhow::{Context as _, Result};

use support::lwk_env::LiquidRegtestEnv;
use support::lwk_wallet::LwkWalletFixture;

const ISSUER_MNEMONIC: &str = lwk_test_util::TEST_MNEMONIC;
const ISSUER_SLIP77: &str = lwk_test_util::TEST_MNEMONIC_SLIP77;

const RECEIVER_MNEMONIC: &str =
    "legal winner thank year wave sausage worth useful legal winner thank yellow";
const RECEIVER_SLIP77: &str = "0000000000000000000000000000000000000000000000000000000000000002";

#[test]
#[ignore = "requires `elementsd` and liquid-enabled `electrs` binaries (run via `nix develop`)"]
fn lwk_liquid_regtest_wallet_issue_send_balance() -> Result<()> {
    let _ = template::logging::init();

    let env = LiquidRegtestEnv::start().context("start liquid regtest env")?;
    let electrum_url = env.electrum_url();
    tracing::info!(electrum_url = %electrum_url, "liquid regtest env started");

    let mut issuer = LwkWalletFixture::new("issuer", ISSUER_MNEMONIC, ISSUER_SLIP77, &electrum_url)
        .context("create issuer wallet fixture")?;
    let mut receiver = LwkWalletFixture::new(
        "receiver",
        RECEIVER_MNEMONIC,
        RECEIVER_SLIP77,
        &electrum_url,
    )
    .context("create receiver wallet fixture")?;

    let policy_asset = issuer.policy_asset();

    let issuer_lbtc_before = issuer
        .balance(&policy_asset)
        .context("get issuer policy asset balance (before funding)")?;

    issuer
        .fund_lbtc(&env, 2_000_000)
        .context("fund issuer with lbtc")?;
    issuer.sync().context("sync issuer after funding")?;

    let issuer_lbtc_after = issuer
        .balance(&policy_asset)
        .context("get issuer policy asset balance (after funding)")?;
    anyhow::ensure!(
        issuer_lbtc_after >= issuer_lbtc_before + 2_000_000,
        "issuer lbtc did not increase: before={} after={}",
        issuer_lbtc_before,
        issuer_lbtc_after
    );

    let issuer_lbtc_before_issuance = issuer
        .balance(&policy_asset)
        .context("get issuer policy asset balance (before issuance)")?;

    let (_issuance_txid, asset_id, token_id) =
        issuer.issue_asset(&env, 10_000, 1).context("issue asset")?;
    issuer.sync().context("sync issuer after issuance")?;

    let issuer_lbtc_after_issuance = issuer
        .balance(&policy_asset)
        .context("get issuer policy asset balance (after issuance)")?;
    anyhow::ensure!(
        issuer_lbtc_after_issuance < issuer_lbtc_before_issuance,
        "issuer lbtc did not decrease after issuance (fee expected): before={} after={}",
        issuer_lbtc_before_issuance,
        issuer_lbtc_after_issuance
    );

    anyhow::ensure!(
        issuer.balance(&asset_id)? == 10_000,
        "issuer asset balance mismatch"
    );
    anyhow::ensure!(
        issuer.balance(&token_id)? == 1,
        "issuer reissuance token balance mismatch"
    );

    let receiver_address = receiver.address().context("get receiver address")?;

    let issuer_asset_before_send = issuer
        .balance(&asset_id)
        .context("get issuer asset balance (before send)")?;
    let receiver_asset_before_send = receiver
        .balance(&asset_id)
        .context("get receiver asset balance (before send)")?;

    issuer
        .send_asset(&env, &receiver_address, &asset_id, 1_234)
        .context("send asset to receiver")?;
    issuer.sync().context("sync issuer after send")?;
    receiver.sync().context("sync receiver after receive")?;

    let issuer_asset_after_send = issuer
        .balance(&asset_id)
        .context("get issuer asset balance (after send)")?;
    let receiver_asset_after_send = receiver
        .balance(&asset_id)
        .context("get receiver asset balance (after send)")?;

    anyhow::ensure!(
        issuer_asset_after_send == issuer_asset_before_send - 1_234,
        "issuer asset balance did not decrease correctly: before={} after={}",
        issuer_asset_before_send,
        issuer_asset_after_send
    );
    anyhow::ensure!(
        receiver_asset_after_send == receiver_asset_before_send + 1_234,
        "receiver asset balance did not increase correctly: before={} after={}",
        receiver_asset_before_send,
        receiver_asset_after_send
    );

    anyhow::ensure!(
        issuer.balance(&token_id)? == 1,
        "issuer reissuance token balance changed unexpectedly"
    );

    Ok(())
}
