use std::path::PathBuf;
use std::str::FromStr as _;
use std::time::Duration;

use anyhow::{Context as _, Result};
use clap::Parser as _;
use ln_liquid_swap::lightning::invoice::payment_hash_from_bolt11;
use ln_liquid_swap::lightning::ldk::LdkLightningClient;
use ln_liquid_swap::liquid::htlc::{
    HtlcFunding, HtlcSpec, claim_tx_from_witness_script, pubkey_hash160_from_p2wpkh_address,
    sha256_preimage,
};
use ln_liquid_swap::liquid::keys::derive_secret_key;
use ln_liquid_swap::liquid::wallet::LiquidWallet;
use ln_liquid_swap::proto::v1::swap_service_client::SwapServiceClient;
use ln_liquid_swap::proto::v1::{CreateSwapRequest, GetOfferRequest, Swap};
use lwk_wollet::ElementsNetwork;

#[derive(Debug, clap::Parser)]
struct Args {
    #[arg(long)]
    seller_grpc_url: String,

    #[arg(long)]
    ldk_rest_addr: String,

    #[arg(long)]
    liquid_electrum_url: String,

    #[arg(long)]
    wallet_dir: PathBuf,

    #[arg(long)]
    buyer_mnemonic: String,

    #[arg(long)]
    buyer_slip77: String,

    #[arg(long)]
    asset_id: String,

    #[arg(long)]
    asset_amount: u64,

    #[arg(long, default_value_t = 1)]
    min_funding_confs: u32,

    #[arg(long, default_value_t = 500)]
    claim_fee_sats: u64,

    #[arg(long, default_value_t = 300)]
    funding_timeout_secs: u64,

    #[arg(long, default_value_t = 60)]
    payment_timeout_secs: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    ln_liquid_swap::logging::init().ok();
    let args = Args::parse();

    std::fs::create_dir_all(&args.wallet_dir).context("create wallet_dir")?;

    let network = ElementsNetwork::default_regtest();
    let mut buyer_wallet = LiquidWallet::new(
        &args.buyer_mnemonic,
        &args.buyer_slip77,
        &args.liquid_electrum_url,
        &args.wallet_dir,
        network,
    )
    .context("create buyer liquid wallet")?;

    let buyer_key_index = 0u32;
    let buyer_receive = buyer_wallet
        .address_at(buyer_key_index)
        .context("get buyer receive address")?;
    let buyer_secret_key = derive_secret_key(buyer_wallet.signer(), buyer_key_index)
        .context("derive buyer secret key")?;
    tracing::info!(buyer_receive_address = %buyer_receive, "buyer wallet ready");

    let mut seller = SwapServiceClient::connect(args.seller_grpc_url.clone())
        .await
        .context("connect seller gRPC")?;

    let offer = seller
        .get_offer(GetOfferRequest {
            asset_id: args.asset_id.clone(),
        })
        .await
        .context("GetOffer")?
        .into_inner();

    anyhow::ensure!(
        args.min_funding_confs <= offer.max_min_funding_confs,
        "min_funding_confs must be <= {}",
        offer.max_min_funding_confs
    );

    let max_total_price_msat = args
        .asset_amount
        .checked_mul(offer.price_msat_per_asset_unit)
        .ok_or_else(|| anyhow::anyhow!("max_total_price_msat overflow"))?;
    tracing::info!(
        asset_id = %offer.asset_id,
        asset_amount = args.asset_amount,
        price_msat_per_asset_unit = offer.price_msat_per_asset_unit,
        max_total_price_msat,
        "received offer"
    );

    let resp = seller
        .create_swap(CreateSwapRequest {
            asset_id: args.asset_id.clone(),
            asset_amount: args.asset_amount,
            buyer_claim_address: buyer_receive.to_string(),
            min_funding_confs: args.min_funding_confs,
            max_total_price_msat,
        })
        .await
        .context("CreateSwap")?;

    let swap = resp.into_inner().swap.context("missing swap in response")?;

    let invoice_amount_msat =
        ln_liquid_swap::lightning::invoice::amount_msat_from_bolt11(&swap.bolt11_invoice)
            .context("parse invoice amount")?
            .context("invoice amount is required")?;
    anyhow::ensure!(
        invoice_amount_msat <= max_total_price_msat,
        "invoice amount exceeds max_total_price_msat: invoice_amount_msat={invoice_amount_msat} max_total_price_msat={max_total_price_msat}"
    );

    verify_swap_before_payment(
        &mut buyer_wallet,
        &buyer_receive,
        &swap,
        Duration::from_secs(args.funding_timeout_secs),
    )
    .context("verify swap")?;

    let ln = LdkLightningClient::new(args.ldk_rest_addr);
    let payment_id = ln
        .pay_invoice(swap.bolt11_invoice.clone())
        .await
        .context("pay invoice")?;
    let preimage = ln
        .wait_preimage(&payment_id, Duration::from_secs(args.payment_timeout_secs))
        .await
        .context("wait preimage")?;

    let expected_payment_hash = hex::decode(&swap.payment_hash).context("decode payment_hash")?;
    let expected_payment_hash: [u8; 32] = expected_payment_hash
        .try_into()
        .map_err(|_| anyhow::anyhow!("payment_hash must be 32 bytes"))?;
    let got_payment_hash = sha256_preimage(&preimage);
    anyhow::ensure!(
        got_payment_hash == expected_payment_hash,
        "preimage hash mismatch"
    );

    let liquid = swap.liquid.context("missing liquid details")?;

    let witness_script = lwk_wollet::elements::Script::from(liquid.witness_script);

    let funding_txid =
        lwk_wollet::elements::Txid::from_str(&liquid.funding_txid).context("parse funding_txid")?;
    let asset_id =
        lwk_wollet::elements::AssetId::from_str(&liquid.asset_id).context("parse asset_id")?;
    let policy_asset = buyer_wallet.policy_asset();

    let funding = HtlcFunding {
        funding_txid,
        asset_vout: liquid.asset_vout,
        lbtc_vout: liquid.lbtc_vout,
        asset_id,
        asset_amount: liquid.asset_amount,
        policy_asset,
        fee_subsidy_sats: liquid.fee_subsidy_sats,
    };

    let claim_tx = claim_tx_from_witness_script(
        &witness_script,
        &funding,
        &buyer_receive,
        &buyer_secret_key,
        preimage,
        args.claim_fee_sats,
    )
    .context("build claim tx")?;

    let claim_txid = buyer_wallet
        .broadcast_transaction(&claim_tx)
        .context("broadcast claim tx")?;

    tracing::info!(%claim_txid, "broadcast claim tx");
    Ok(())
}

fn verify_swap_before_payment(
    buyer_wallet: &mut LiquidWallet,
    buyer_receive: &lwk_wollet::elements::Address,
    swap: &Swap,
    funding_timeout: Duration,
) -> Result<()> {
    let invoice_payment_hash =
        payment_hash_from_bolt11(&swap.bolt11_invoice).context("parse bolt11 invoice")?;
    let swap_payment_hash = hex::decode(&swap.payment_hash).context("decode payment_hash")?;
    let swap_payment_hash: [u8; 32] = swap_payment_hash
        .try_into()
        .map_err(|_| anyhow::anyhow!("payment_hash must be 32 bytes"))?;
    anyhow::ensure!(
        invoice_payment_hash == swap_payment_hash,
        "invoice payment_hash mismatch"
    );

    let liquid = swap.liquid.as_ref().context("missing liquid details")?;
    let witness_script = lwk_wollet::elements::Script::from(liquid.witness_script.clone());

    let htlc_spec =
        HtlcSpec::parse_witness_script(&witness_script).context("parse witness script")?;
    anyhow::ensure!(
        htlc_spec.payment_hash == invoice_payment_hash,
        "witness_script payment_hash mismatch"
    );

    let buyer_pubkey_hash160 =
        pubkey_hash160_from_p2wpkh_address(buyer_receive).context("extract buyer pubkey hash")?;
    anyhow::ensure!(
        htlc_spec.buyer_pubkey_hash160 == buyer_pubkey_hash160,
        "witness_script buyer pubkey hash mismatch"
    );
    anyhow::ensure!(
        htlc_spec.refund_lock_height == liquid.refund_lock_height,
        "witness_script refund_lock_height mismatch"
    );

    let expected_p2wsh = lwk_wollet::elements::Address::p2wsh(
        &witness_script,
        None,
        buyer_wallet.network().address_params(),
    );
    anyhow::ensure!(
        liquid.p2wsh_address == expected_p2wsh.to_string(),
        "p2wsh_address mismatch"
    );

    let funding_txid =
        lwk_wollet::elements::Txid::from_str(&liquid.funding_txid).context("parse funding_txid")?;
    let funding_tx = buyer_wallet
        .get_transaction(&funding_txid)
        .context("fetch funding tx")?;

    anyhow::ensure!(
        liquid.asset_vout != liquid.lbtc_vout,
        "asset_vout and lbtc_vout must be distinct"
    );

    let asset_out = funding_tx
        .output
        .get(liquid.asset_vout as usize)
        .context("asset_vout out of range")?;
    let lbtc_out = funding_tx
        .output
        .get(liquid.lbtc_vout as usize)
        .context("lbtc_vout out of range")?;

    anyhow::ensure!(
        asset_out.script_pubkey == expected_p2wsh.script_pubkey(),
        "asset output script mismatch"
    );
    anyhow::ensure!(
        lbtc_out.script_pubkey == expected_p2wsh.script_pubkey(),
        "lbtc output script mismatch"
    );

    let expected_asset_id =
        lwk_wollet::elements::AssetId::from_str(&liquid.asset_id).context("parse asset_id")?;
    match asset_out.asset {
        lwk_wollet::elements::confidential::Asset::Explicit(a) if a == expected_asset_id => {}
        other => anyhow::bail!("asset output must be explicit for asset_id, got {other:?}"),
    }
    match asset_out.value {
        lwk_wollet::elements::confidential::Value::Explicit(v) if v == liquid.asset_amount => {}
        other => anyhow::bail!("asset output must be explicit for asset_amount, got {other:?}"),
    }

    let policy_asset = buyer_wallet.policy_asset();
    match lbtc_out.asset {
        lwk_wollet::elements::confidential::Asset::Explicit(a) if a == policy_asset => {}
        other => anyhow::bail!("lbtc output must be explicit for policy asset, got {other:?}"),
    }
    match lbtc_out.value {
        lwk_wollet::elements::confidential::Value::Explicit(v) if v == liquid.fee_subsidy_sats => {}
        other => anyhow::bail!("lbtc output must be explicit for fee_subsidy_sats, got {other:?}"),
    }

    buyer_wallet
        .wait_for_tx_confirmations_for_script(
            &expected_p2wsh.script_pubkey(),
            &funding_txid,
            liquid.min_funding_confs,
            funding_timeout,
        )
        .context("wait funding confirmations")?;

    Ok(())
}
