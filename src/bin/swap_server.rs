use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr as _;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context as _, Result};
use clap::Parser as _;
use ln_liquid_swap::lightning::ldk::LdkLightningClient;
use ln_liquid_swap::liquid::htlc::{HtlcFunding, refund_tx_from_witness_script};
use ln_liquid_swap::liquid::keys::derive_secret_key;
use ln_liquid_swap::liquid::wallet::LiquidWallet;
use ln_liquid_swap::proto::v1::swap_service_server::SwapServiceServer;
use ln_liquid_swap::swap::SwapStatus;
use ln_liquid_swap::swap::service::{SwapServiceConfig, SwapServiceImpl};
use ln_liquid_swap::swap::store::SqliteStore;
use lwk_wollet::ElementsNetwork;
use tonic::transport::Server;

#[derive(Debug, clap::Parser)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:50051")]
    listen_addr: String,

    #[arg(long)]
    seller_ldk_rest_addr: String,

    #[arg(long)]
    buyer_ldk_rest_addr: String,

    #[arg(long)]
    liquid_electrum_url: String,

    #[arg(long)]
    seller_wallet_dir: PathBuf,

    #[arg(long)]
    buyer_wallet_dir: PathBuf,

    #[arg(long)]
    store_path: PathBuf,

    #[arg(long)]
    seller_mnemonic: String,

    #[arg(long)]
    seller_slip77: String,

    #[arg(long)]
    buyer_mnemonic: String,

    #[arg(long)]
    buyer_slip77: String,

    #[arg(long)]
    sell_asset_id: String,

    #[arg(long, default_value_t = 1)]
    price_msat_per_asset_unit: u64,

    #[arg(long, default_value_t = 10_000)]
    fee_subsidy_sats: u64,

    #[arg(long, default_value_t = 144)]
    refund_delta_blocks: u32,

    #[arg(long, default_value_t = 3600)]
    invoice_expiry_secs: u32,

    #[arg(long, default_value_t = 0)]
    seller_key_index: u32,

    #[arg(long, default_value_t = 0)]
    buyer_key_index: u32,

    #[arg(long, default_value_t = 5)]
    refund_poll_interval_secs: u64,

    #[arg(long, default_value_t = 500)]
    refund_fee_sats: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    ln_liquid_swap::logging::init().ok();

    let args = Args::parse();
    let listen_addr: SocketAddr = args.listen_addr.parse().context("parse listen_addr")?;

    std::fs::create_dir_all(&args.seller_wallet_dir).context("create seller_wallet_dir")?;
    std::fs::create_dir_all(&args.buyer_wallet_dir).context("create buyer_wallet_dir")?;
    if let Some(parent) = args.store_path.parent() {
        std::fs::create_dir_all(parent).context("create store parent dir")?;
    }

    let sell_asset_id = lwk_wollet::elements::AssetId::from_str(&args.sell_asset_id)
        .context("parse sell_asset_id")?;

    let network = ElementsNetwork::default_regtest();
    let seller_wallet = LiquidWallet::new(
        &args.seller_mnemonic,
        &args.seller_slip77,
        &args.liquid_electrum_url,
        &args.seller_wallet_dir,
        network,
    )
    .context("create seller liquid wallet")?;

    let seller_receive_address = seller_wallet
        .address_at(args.seller_key_index)
        .context("get seller receive address")?;
    tracing::info!(
        seller_receive_address = %seller_receive_address,
        seller_key_index = args.seller_key_index,
        "seller wallet ready"
    );

    let buyer_wallet = LiquidWallet::new(
        &args.buyer_mnemonic,
        &args.buyer_slip77,
        &args.liquid_electrum_url,
        &args.buyer_wallet_dir,
        network,
    )
    .context("create buyer liquid wallet")?;
    let buyer_receive_address = buyer_wallet
        .address_at(args.buyer_key_index)
        .context("get buyer receive address")?;
    tracing::info!(
        buyer_receive_address = %buyer_receive_address,
        buyer_key_index = args.buyer_key_index,
        "buyer wallet ready"
    );

    let store = SqliteStore::open(args.store_path).context("open sqlite store")?;

    let seller_wallet = Arc::new(Mutex::new(seller_wallet));
    let buyer_wallet = Arc::new(Mutex::new(buyer_wallet));
    let store = Arc::new(Mutex::new(store));

    let cfg = SwapServiceConfig {
        sell_asset_id,
        price_msat_per_asset_unit: args.price_msat_per_asset_unit,
        fee_subsidy_sats: args.fee_subsidy_sats,
        refund_delta_blocks: args.refund_delta_blocks,
        invoice_expiry_secs: args.invoice_expiry_secs,
        seller_key_index: args.seller_key_index,
        buyer_key_index: args.buyer_key_index,
    };

    let seller_ln = LdkLightningClient::new(args.seller_ldk_rest_addr);
    let buyer_ln = LdkLightningClient::new(args.buyer_ldk_rest_addr);

    let svc = SwapServiceImpl::new(
        cfg.clone(),
        seller_ln,
        buyer_ln,
        seller_wallet.clone(),
        buyer_wallet,
        store.clone(),
    );

    spawn_refund_worker(
        seller_wallet.clone(),
        store.clone(),
        cfg.seller_key_index,
        Duration::from_secs(args.refund_poll_interval_secs),
        args.refund_fee_sats,
    );

    tracing::info!(%listen_addr, "starting swap gRPC server");

    Server::builder()
        .add_service(SwapServiceServer::new(svc))
        .serve(listen_addr)
        .await
        .context("serve gRPC")?;

    Ok(())
}

fn spawn_refund_worker(
    wallet: Arc<Mutex<LiquidWallet>>,
    store: Arc<Mutex<SqliteStore>>,
    seller_key_index: u32,
    poll_interval: Duration,
    fee_sats: u64,
) {
    tokio::spawn(async move {
        loop {
            match tokio::task::spawn_blocking({
                let wallet = wallet.clone();
                let store = store.clone();
                move || refund_once(wallet, store, seller_key_index, fee_sats)
            })
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    tracing::warn!(error = %err, "refund worker error");
                }
                Err(err) => {
                    tracing::warn!(error = %err, "refund worker join error");
                }
            }

            tokio::time::sleep(poll_interval).await;
        }
    });
}

fn refund_once(
    wallet: Arc<Mutex<LiquidWallet>>,
    store: Arc<Mutex<SqliteStore>>,
    seller_key_index: u32,
    fee_sats: u64,
) -> Result<()> {
    let mut wallet = wallet.lock().expect("wallet mutex poisoned");
    wallet.sync().context("sync wallet")?;
    let tip_height = wallet.tip_height();

    let swaps = store
        .lock()
        .expect("store mutex poisoned")
        .list_swaps()
        .context("list swaps")?;
    for s in swaps {
        if !matches!(s.status, SwapStatus::Created | SwapStatus::Funded) {
            continue;
        }
        if tip_height < s.refund_lock_height {
            continue;
        }

        let funding_txid =
            lwk_wollet::elements::Txid::from_str(&s.funding_txid).context("parse funding_txid")?;
        let asset_id =
            lwk_wollet::elements::AssetId::from_str(&s.asset_id).context("parse asset_id")?;
        let policy_asset = wallet.policy_asset();
        let witness_script: lwk_wollet::elements::Script = s
            .witness_script_hex
            .parse()
            .map_err(|e| anyhow::anyhow!("parse witness_script: {e:?}"))?;

        let funding = HtlcFunding {
            funding_txid,
            asset_vout: s.asset_vout,
            lbtc_vout: s.lbtc_vout,
            asset_id,
            asset_amount: s.asset_amount,
            policy_asset,
            fee_subsidy_sats: s.fee_subsidy_sats,
        };

        let seller_receive = wallet
            .address_at(seller_key_index)
            .context("get seller receive address")?;

        let seller_secret_key = derive_secret_key(wallet.signer(), seller_key_index)
            .context("derive seller secret key")?;

        let tx = refund_tx_from_witness_script(
            &witness_script,
            s.refund_lock_height,
            &funding,
            &seller_receive,
            &seller_secret_key,
            fee_sats,
        )
        .context("build refund tx")?;

        match wallet.broadcast_transaction(&tx) {
            Ok(txid) => {
                tracing::info!(swap_id = %s.swap_id, refund_txid = %txid, "broadcast refund tx");
                let mut store = store.lock().expect("store mutex poisoned");
                store
                    .update_swap_status(&s.swap_id, SwapStatus::Refunded)
                    .context("update swap status (refunded)")?;
            }
            Err(err) => {
                tracing::warn!(swap_id = %s.swap_id, error = %err, "refund broadcast failed");
            }
        }
    }

    Ok(())
}
