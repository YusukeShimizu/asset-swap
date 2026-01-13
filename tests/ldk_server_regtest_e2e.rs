mod support;

use std::time::{Duration, Instant};

use anyhow::Context as _;
use bitcoincore_rpc::RpcApi as _;
use bitcoincore_rpc::bitcoin::address::NetworkUnchecked;
use bitcoincore_rpc::bitcoin::{Address, Network};
use ldk_server_protos::api::{
    Bolt11ReceiveRequest, Bolt11SendRequest, GetBalancesRequest, GetNodeInfoRequest,
    ListChannelsRequest, ListPaymentsRequest, OnchainReceiveRequest, OpenChannelRequest,
};
use ldk_server_protos::types::{
    Bolt11InvoiceDescription, PaymentDirection, PaymentStatus, bolt11_invoice_description,
    payment_kind,
};

use support::bitcoind::BitcoindProcess;
use support::ldk_server::LdkServerProcess;
use support::wait::wait_for;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires `bitcoind` and `ldk-server` binaries (run via `nix develop`)"]
async fn ldk_server_regtest_channel_invoice_payment() -> anyhow::Result<()> {
    let _ = ln_liquid_swap::logging::init();

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
    let alice_address_unchecked: Address<NetworkUnchecked> = alice_address_str
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
    let bob_address_unchecked: Address<NetworkUnchecked> = bob_address_str
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

    let amount_msat = 50_000;
    let description = Bolt11InvoiceDescription {
        kind: Some(bolt11_invoice_description::Kind::Direct("e2e".to_string())),
    };

    let invoice = bob
        .client()
        .bolt11_receive(Bolt11ReceiveRequest {
            amount_msat: Some(amount_msat),
            description: Some(description),
            expiry_secs: 3600,
        })
        .await
        .context("bob Bolt11Receive")?
        .invoice;

    let payment_id = alice
        .client()
        .bolt11_send(Bolt11SendRequest {
            invoice,
            amount_msat: None,
            route_parameters: None,
        })
        .await
        .context("alice Bolt11Send")?
        .payment_id;

    wait_for(
        "alice outbound payment succeeded",
        Duration::from_secs(60),
        || {
            let client = alice.client();
            let payment_id = payment_id.clone();
            async move {
                let payments = match client
                    .list_payments(ListPaymentsRequest { page_token: None })
                    .await
                {
                    Ok(r) => r.payments,
                    Err(_) => return Ok(None),
                };
                let Some(payment) = payments.into_iter().find(|p| p.id == payment_id) else {
                    return Ok(None);
                };

                if payment.direction == PaymentDirection::Outbound as i32
                    && payment.status == PaymentStatus::Succeeded as i32
                    && payment.amount_msat == Some(amount_msat)
                {
                    return Ok(Some(()));
                }

                Ok(None)
            }
        },
    )
    .await?;

    wait_for(
        "bob inbound payment succeeded",
        Duration::from_secs(60),
        || {
            let client = bob.client();
            async move {
                let payments = match client
                    .list_payments(ListPaymentsRequest { page_token: None })
                    .await
                {
                    Ok(r) => r.payments,
                    Err(_) => return Ok(None),
                };

                let has_succeeded = payments.into_iter().any(|p| {
                    p.direction == PaymentDirection::Inbound as i32
                        && p.status == PaymentStatus::Succeeded as i32
                        && p.amount_msat == Some(amount_msat)
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

    Ok(())
}
