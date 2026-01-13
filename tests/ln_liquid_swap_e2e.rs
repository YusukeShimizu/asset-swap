mod support;

use std::net::SocketAddr;
use std::str::FromStr as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use bitcoincore_rpc::RpcApi as _;
use bitcoincore_rpc::bitcoin::address::NetworkUnchecked;
use bitcoincore_rpc::bitcoin::{Address as BtcAddress, Network};
use ldk_server_protos::api::{
    GetBalancesRequest, GetNodeInfoRequest, ListChannelsRequest, ListPaymentsRequest,
    OnchainReceiveRequest, OpenChannelRequest,
};
use ldk_server_protos::types::{PaymentDirection, PaymentStatus, payment_kind};
use lwk_wollet::ElementsNetwork;
use tonic::transport::Server;

use support::bitcoind::BitcoindProcess;
use support::ldk_server::LdkServerProcess;
use support::lwk_env::LiquidRegtestEnv;
use support::lwk_wallet::LwkWalletFixture;
use support::port::get_available_port;
use support::wait::wait_for;

use ln_liquid_swap::lightning::invoice::payment_hash_from_bolt11;
use ln_liquid_swap::lightning::ldk::LdkLightningClient;
use ln_liquid_swap::liquid::htlc::{HtlcFunding, claim_tx_from_witness_script, sha256_preimage};
use ln_liquid_swap::liquid::keys::derive_secret_key;
use ln_liquid_swap::liquid::wallet::LiquidWallet;
use ln_liquid_swap::proto::v1::CreateSwapRequest;
use ln_liquid_swap::proto::v1::swap_service_client::SwapServiceClient;
use ln_liquid_swap::proto::v1::swap_service_server::SwapServiceServer;
use ln_liquid_swap::swap::service::{SellerConfig, SwapSellerService};
use ln_liquid_swap::swap::store::SqliteSwapStore;

const ISSUER_MNEMONIC: &str = lwk_test_util::TEST_MNEMONIC;
const ISSUER_SLIP77: &str = lwk_test_util::TEST_MNEMONIC_SLIP77;

const SELLER_MNEMONIC: &str =
    "legal winner thank year wave sausage worth useful legal winner thank yellow";
const SELLER_SLIP77: &str = "0000000000000000000000000000000000000000000000000000000000000002";

const BUYER_MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const BUYER_SLIP77: &str = "0000000000000000000000000000000000000000000000000000000000000003";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires `bitcoind`, `ldk-server`, `elementsd`, and liquid-enabled `electrs` (run via `nix develop`)"]
async fn ln_to_liquid_swap_create_pay_claim() -> Result<()> {
    let _ = ln_liquid_swap::logging::init();

    // --- LN: bitcoind + ldk-server ---
    let bitcoind = BitcoindProcess::start().context("start bitcoind")?;
    bitcoind
        .create_wallet_if_missing("miner")
        .context("create miner wallet")?;
    bitcoind
        .mine_blocks("miner", 101)
        .context("mine initial blocks")?;

    let mut alice =
        LdkServerProcess::start("alice", &bitcoind).context("start ldk-server alice")?;
    let mut bob = LdkServerProcess::start("bob", &bitcoind).context("start ldk-server bob")?;

    alice
        .wait_ready(Duration::from_secs(60))
        .await
        .context("wait alice ready")?;
    bob.wait_ready(Duration::from_secs(60))
        .await
        .context("wait bob ready")?;

    let bitcoind_height = bitcoind
        .client()?
        .get_block_count()
        .context("get bitcoind block count")?;

    wait_for("alice synced to chain", Duration::from_secs(60), || {
        let client = alice.client();
        async move {
            let info = match client.get_node_info(GetNodeInfoRequest {}).await {
                Ok(info) => info,
                Err(_) => return Ok(None),
            };
            let Some(best_block) = info.current_best_block else {
                return Ok(None);
            };
            Ok((best_block.height >= bitcoind_height as u32).then_some(()))
        }
    })
    .await?;

    wait_for("bob synced to chain", Duration::from_secs(60), || {
        let client = bob.client();
        async move {
            let info = match client.get_node_info(GetNodeInfoRequest {}).await {
                Ok(info) => info,
                Err(_) => return Ok(None),
            };
            let Some(best_block) = info.current_best_block else {
                return Ok(None);
            };
            Ok((best_block.height >= bitcoind_height as u32).then_some(()))
        }
    })
    .await?;

    let alice_address_str = alice
        .client()
        .onchain_receive(OnchainReceiveRequest {})
        .await
        .context("alice OnchainReceive")?
        .address;
    let alice_address_unchecked: BtcAddress<NetworkUnchecked> = alice_address_str
        .parse()
        .with_context(|| format!("parse alice address {alice_address_str}"))?;
    let alice_address = alice_address_unchecked
        .require_network(Network::Regtest)
        .context("check alice address network")?;

    let bob_address_str = bob
        .client()
        .onchain_receive(OnchainReceiveRequest {})
        .await
        .context("bob OnchainReceive")?
        .address;
    let bob_address_unchecked: BtcAddress<NetworkUnchecked> = bob_address_str
        .parse()
        .with_context(|| format!("parse bob address {bob_address_str}"))?;
    let bob_address = bob_address_unchecked
        .require_network(Network::Regtest)
        .context("check bob address network")?;

    bitcoind
        .send_to_address("miner", &alice_address, 200_000)
        .context("fund alice")?;
    bitcoind
        .send_to_address("miner", &bob_address, 100_000)
        .context("fund bob")?;
    bitcoind
        .mine_blocks("miner", 1)
        .context("confirm funding tx")?;

    wait_for(
        "alice onchain balance available",
        Duration::from_secs(60),
        || {
            let client = alice.client();
            async move {
                let balances = match client.get_balances(GetBalancesRequest {}).await {
                    Ok(balances) => balances,
                    Err(_) => return Ok(None),
                };
                Ok((balances.spendable_onchain_balance_sats >= 200_000).then_some(()))
            }
        },
    )
    .await?;

    wait_for(
        "bob onchain balance available",
        Duration::from_secs(60),
        || {
            let client = bob.client();
            async move {
                let balances = match client.get_balances(GetBalancesRequest {}).await {
                    Ok(balances) => balances,
                    Err(_) => return Ok(None),
                };
                Ok((balances.spendable_onchain_balance_sats >= 25_000).then_some(()))
            }
        },
    )
    .await?;

    let bob_info = bob
        .client()
        .get_node_info(GetNodeInfoRequest {})
        .await
        .context("bob GetNodeInfo")?;

    let open_resp = alice
        .client()
        .open_channel(OpenChannelRequest {
            node_pubkey: bob_info.node_id,
            address: bob.listen_addr().to_string(),
            channel_amount_sats: 100_000,
            push_to_counterparty_msat: None,
            channel_config: None,
            announce_channel: false,
        })
        .await
        .context("alice OpenChannel")?;

    let alice_user_channel_id = open_resp.user_channel_id;
    let channel_deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let alice_channels = alice
            .client()
            .list_channels(ListChannelsRequest {})
            .await
            .context("alice ListChannels")?
            .channels;
        let bob_channels = bob
            .client()
            .list_channels(ListChannelsRequest {})
            .await
            .context("bob ListChannels")?
            .channels;

        let alice_channel = alice_channels
            .iter()
            .find(|c| c.user_channel_id == alice_user_channel_id);
        let alice_channel_id = alice_channel.map(|c| c.channel_id.clone());
        let alice_usable = alice_channel.map(|c| c.is_usable).unwrap_or(false);

        let bob_usable = alice_channel_id
            .map(|channel_id| {
                bob_channels
                    .iter()
                    .any(|c| c.channel_id == channel_id && c.is_usable)
            })
            .unwrap_or(false);

        if alice_usable && bob_usable {
            break;
        }

        if Instant::now() >= channel_deadline {
            anyhow::bail!(
                "channel did not become usable: alice_stdout_log={} alice_file_log={} bob_stdout_log={} bob_file_log={} bitcoind_log={}",
                alice.stdout_log_path().display(),
                alice.file_log_path().display(),
                bob.stdout_log_path().display(),
                bob.file_log_path().display(),
                bitcoind.log_path().display()
            );
        }

        bitcoind
            .mine_blocks("miner", 1)
            .context("mine for channel confirmations")?;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // --- Liquid: elementsd + electrs + wallets ---
    let env = LiquidRegtestEnv::start().context("start liquid regtest env")?;
    let electrum_url = env.electrum_url();

    let mut issuer = LwkWalletFixture::new("issuer", ISSUER_MNEMONIC, ISSUER_SLIP77, &electrum_url)
        .context("create issuer wallet")?;

    issuer.fund_lbtc(&env, 3_000_000).context("fund issuer")?;
    issuer.sync().context("sync issuer after funding")?;

    let (_issuance_txid, asset_id, _token_id) =
        issuer.issue_asset(&env, 50_000, 1).context("issue asset")?;
    issuer.sync().context("sync issuer after issuance")?;

    let seller_dir = tempfile::tempdir().context("create seller wallet dir")?;
    let mut seller_wallet = LiquidWallet::new(
        SELLER_MNEMONIC,
        SELLER_SLIP77,
        &electrum_url,
        seller_dir.path(),
        ElementsNetwork::default_regtest(),
    )
    .context("create seller wallet")?;

    let seller_receive = seller_wallet.address_at(0).context("get seller address")?;
    env.elementsd_sendtoaddress(&seller_receive, 2_000_000, None);
    env.elementsd_generate(1);
    seller_wallet.sync().context("sync seller after funding")?;

    issuer
        .send_asset(&env, &seller_receive, &asset_id, 10_000)
        .context("send asset to seller")?;
    seller_wallet
        .sync()
        .context("sync seller after receiving asset")?;

    let buyer_dir = tempfile::tempdir().context("create buyer wallet dir")?;
    let mut buyer_wallet = LiquidWallet::new(
        BUYER_MNEMONIC,
        BUYER_SLIP77,
        &electrum_url,
        buyer_dir.path(),
        ElementsNetwork::default_regtest(),
    )
    .context("create buyer wallet")?;

    let buyer_receive = buyer_wallet.address_at(0).context("get buyer address")?;
    let buyer_asset_before = buyer_wallet
        .balance(&asset_id)
        .context("buyer asset balance before")?;

    // --- Swap seller gRPC server (LN receiver = bob) ---
    let store_path = seller_dir.path().join("swap_store.sqlite3");
    let store = SqliteSwapStore::open(store_path).context("create swap store")?;
    let store = Arc::new(std::sync::Mutex::new(store));
    let wallet = Arc::new(std::sync::Mutex::new(seller_wallet));

    let cfg = SellerConfig {
        sell_asset_id: asset_id,
        price_msat_per_asset_unit: 1_000,
        fee_subsidy_sats: 10_000,
        refund_delta_blocks: 20,
        invoice_expiry_secs: 3600,
        seller_key_index: 0,
    };
    let seller_ln = LdkLightningClient::new(bob.rest_service_address().to_string());
    let svc = SwapSellerService::new(cfg, seller_ln, wallet.clone(), store);

    let port = get_available_port().context("select gRPC port")?;
    let listen_addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = Server::builder()
            .add_service(SwapServiceServer::new(svc))
            .serve_with_shutdown(listen_addr, async move {
                let _ = shutdown_rx.await;
            })
            .await;
    });

    // --- Buyer: CreateSwap -> verify -> pay -> claim ---
    let mut swap_client = SwapServiceClient::connect(format!("http://{listen_addr}"))
        .await
        .context("connect swap seller")?;

    let asset_amount = 1_000u64;
    let mut create_fut = Box::pin(swap_client.create_swap(CreateSwapRequest {
        asset_id: asset_id.to_string(),
        asset_amount,
        buyer_claim_address: buyer_receive.to_string(),
        min_funding_confs: 1,
        max_total_price_msat: 0,
    }));

    let create_deadline = Instant::now() + Duration::from_secs(60);
    let mut mine_interval = tokio::time::interval(Duration::from_secs(1));
    let swap_resp = loop {
        if Instant::now() >= create_deadline {
            anyhow::bail!("timeout waiting for CreateSwap to return");
        }

        tokio::select! {
            resp = &mut create_fut => {
                break resp.context("CreateSwap")?.into_inner();
            }
            _ = mine_interval.tick() => {
                env.elementsd_generate(1);
            }
        }
    };

    let swap = swap_resp.swap.context("missing swap")?;
    let liquid = swap.liquid.clone().context("missing liquid details")?;

    let invoice_hash = payment_hash_from_bolt11(&swap.bolt11_invoice).context("parse invoice")?;
    let resp_hash = hex::decode(&swap.payment_hash).context("decode payment_hash")?;
    let resp_hash: [u8; 32] = resp_hash
        .try_into()
        .map_err(|_| anyhow::anyhow!("payment_hash must be 32 bytes"))?;
    anyhow::ensure!(invoice_hash == resp_hash, "payment_hash mismatch");

    let witness_script = lwk_wollet::elements::Script::from(liquid.witness_script.clone());
    let spec = ln_liquid_swap::liquid::htlc::HtlcSpec::parse_witness_script(&witness_script)
        .context("parse witness script")?;
    anyhow::ensure!(
        spec.payment_hash == invoice_hash,
        "witness script hash mismatch"
    );
    anyhow::ensure!(
        spec.buyer_pubkey_hash160
            == ln_liquid_swap::liquid::htlc::pubkey_hash160_from_p2wpkh_address(&buyer_receive)
                .context("extract buyer pubkey hash")?,
        "witness script buyer key mismatch"
    );
    anyhow::ensure!(
        spec.refund_lock_height == liquid.refund_lock_height,
        "witness script refund lock mismatch"
    );

    let expected_p2wsh = lwk_wollet::elements::Address::p2wsh(
        &witness_script,
        None,
        ElementsNetwork::default_regtest().address_params(),
    );

    let funding_txid =
        lwk_wollet::elements::Txid::from_str(&liquid.funding_txid).context("parse funding_txid")?;
    let funding_tx = buyer_wallet
        .get_transaction(&funding_txid)
        .context("fetch funding tx")?;

    let asset_out = funding_tx
        .output
        .get(liquid.asset_vout as usize)
        .context("asset_vout out of range")?;
    anyhow::ensure!(
        asset_out.script_pubkey == expected_p2wsh.script_pubkey(),
        "asset output script mismatch"
    );

    let lbtc_out = funding_tx
        .output
        .get(liquid.lbtc_vout as usize)
        .context("lbtc_vout out of range")?;
    anyhow::ensure!(
        lbtc_out.script_pubkey == expected_p2wsh.script_pubkey(),
        "lbtc output script mismatch"
    );

    match asset_out.asset {
        lwk_wollet::elements::confidential::Asset::Explicit(a) if a == asset_id => {}
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

    // Pay invoice from alice.
    let buyer_ln = LdkLightningClient::new(alice.rest_service_address().to_string());
    let payment_id = buyer_ln
        .pay_invoice(swap.bolt11_invoice.clone())
        .await
        .context("pay invoice")?;
    let preimage = buyer_ln
        .wait_preimage(&payment_id, Duration::from_secs(60))
        .await
        .context("wait preimage")?;

    anyhow::ensure!(sha256_preimage(&preimage) == resp_hash, "preimage mismatch");

    let funding = HtlcFunding {
        funding_txid,
        asset_vout: liquid.asset_vout,
        lbtc_vout: liquid.lbtc_vout,
        asset_id,
        asset_amount: liquid.asset_amount,
        policy_asset: buyer_wallet.policy_asset(),
        fee_subsidy_sats: liquid.fee_subsidy_sats,
    };

    let buyer_secret_key =
        derive_secret_key(buyer_wallet.signer(), 0).context("derive buyer key")?;
    let claim_tx = claim_tx_from_witness_script(
        &witness_script,
        &funding,
        &buyer_receive,
        &buyer_secret_key,
        preimage,
        500,
    )
    .context("build claim tx")?;

    let claim_txid = buyer_wallet
        .broadcast_transaction(&claim_tx)
        .context("broadcast claim tx")?;
    env.elementsd_generate(1);
    buyer_wallet.sync().context("sync buyer after claim")?;

    let buyer_asset_after = buyer_wallet
        .balance(&asset_id)
        .context("buyer asset balance after")?;
    anyhow::ensure!(
        buyer_asset_after == buyer_asset_before + asset_amount,
        "buyer asset did not increase: before={} after={}",
        buyer_asset_before,
        buyer_asset_after
    );

    // Cleanup gRPC server.
    let _ = shutdown_tx.send(());

    // Extra observation: alice outbound payment succeeded.
    wait_for(
        "alice outbound payment succeeded",
        Duration::from_secs(30),
        || {
            let client = alice.client();
            async move {
                let payments = match client
                    .list_payments(ListPaymentsRequest { page_token: None })
                    .await
                {
                    Ok(r) => r.payments,
                    Err(_) => return Ok(None),
                };
                let has_succeeded = payments.into_iter().any(|p| {
                    p.direction == PaymentDirection::Outbound as i32
                        && p.status == PaymentStatus::Succeeded as i32
                        && matches!(
                            p.kind.as_ref().and_then(|k| k.kind.as_ref()),
                            Some(payment_kind::Kind::Bolt11(_))
                        )
                });
                Ok(has_succeeded.then_some(()))
            }
        },
    )
    .await?;

    tracing::info!(%claim_txid, "swap e2e completed");
    Ok(())
}
