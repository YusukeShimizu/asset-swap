use anyhow::{Context as _, Result};
use clap::{Parser as _, Subcommand};
use ln_liquid_swap::proto::v1::swap_service_client::SwapServiceClient;
use ln_liquid_swap::proto::v1::{
    ClaimAssetRequest, CreateQuoteRequest, CreateSwapRequest, GetQuoteRequest, GetSwapRequest,
    PayLightningRequest, SwapStatus,
};
use serde_json::json;

#[derive(Debug, clap::Parser)]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:50051")]
    grpc_url: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    CreateQuote {
        #[arg(long)]
        asset_id: String,

        #[arg(long)]
        asset_amount: u64,

        #[arg(long, default_value_t = 1)]
        min_funding_confs: u32,
    },
    GetQuote {
        #[arg(long)]
        quote_id: String,
    },
    CreateSwap {
        #[arg(long)]
        quote_id: String,
    },
    GetSwap {
        #[arg(long)]
        swap_id: String,
    },
    PayLightning {
        #[arg(long)]
        swap_id: String,

        #[arg(long, default_value_t = 60)]
        payment_timeout_secs: u32,
    },
    ClaimAsset {
        #[arg(long)]
        swap_id: String,

        #[arg(long, default_value_t = 500)]
        claim_fee_sats: u64,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    ln_liquid_swap::logging::init().ok();
    let args = Args::parse();

    let mut client = SwapServiceClient::connect(args.grpc_url)
        .await
        .context("connect gRPC")?;

    let out = match args.command {
        Command::CreateQuote {
            asset_id,
            asset_amount,
            min_funding_confs,
        } => {
            let quote = client
                .create_quote(CreateQuoteRequest {
                    asset_id,
                    asset_amount,
                    min_funding_confs,
                })
                .await
                .context("CreateQuote")?
                .into_inner();

            json!({
              "quote_id": quote.quote_id,
              "offer_id": quote.offer_id,
              "asset_id": quote.asset_id,
              "asset_amount": quote.asset_amount,
              "buyer_claim_address": quote.buyer_claim_address,
              "min_funding_confs": quote.min_funding_confs,
              "total_price_msat": quote.total_price_msat,
              "offer": quote.offer.map(|o| json!({
                "asset_id": o.asset_id,
                "price_msat_per_asset_unit": o.price_msat_per_asset_unit,
                "fee_subsidy_sats": o.fee_subsidy_sats,
                "refund_delta_blocks": o.refund_delta_blocks,
                "invoice_expiry_secs": o.invoice_expiry_secs,
                "max_min_funding_confs": o.max_min_funding_confs,
              })),
            })
        }
        Command::GetQuote { quote_id } => {
            let quote = client
                .get_quote(GetQuoteRequest { quote_id })
                .await
                .context("GetQuote")?
                .into_inner();

            json!({
              "quote_id": quote.quote_id,
              "offer_id": quote.offer_id,
              "asset_id": quote.asset_id,
              "asset_amount": quote.asset_amount,
              "buyer_claim_address": quote.buyer_claim_address,
              "min_funding_confs": quote.min_funding_confs,
              "total_price_msat": quote.total_price_msat,
              "offer": quote.offer.map(|o| json!({
                "asset_id": o.asset_id,
                "price_msat_per_asset_unit": o.price_msat_per_asset_unit,
                "fee_subsidy_sats": o.fee_subsidy_sats,
                "refund_delta_blocks": o.refund_delta_blocks,
                "invoice_expiry_secs": o.invoice_expiry_secs,
                "max_min_funding_confs": o.max_min_funding_confs,
              })),
            })
        }
        Command::CreateSwap { quote_id } => {
            let swap = client
                .create_swap(CreateSwapRequest { quote_id })
                .await
                .context("CreateSwap")?
                .into_inner();

            swap_json(swap)
        }
        Command::GetSwap { swap_id } => {
            let swap = client
                .get_swap(GetSwapRequest { swap_id })
                .await
                .context("GetSwap")?
                .into_inner();

            swap_json(swap)
        }
        Command::PayLightning {
            swap_id,
            payment_timeout_secs,
        } => {
            let resp = client
                .pay_lightning(PayLightningRequest {
                    swap_id,
                    payment_timeout_secs,
                })
                .await
                .context("PayLightning")?
                .into_inner();

            json!({
              "payment_id": resp.payment_id,
              "preimage_hex": hex::encode(resp.preimage),
            })
        }
        Command::ClaimAsset {
            swap_id,
            claim_fee_sats,
        } => {
            let resp = client
                .claim_asset(ClaimAssetRequest {
                    swap_id,
                    claim_fee_sats,
                })
                .await
                .context("ClaimAsset")?
                .into_inner();

            json!({
              "claim_txid": resp.claim_txid,
            })
        }
    };

    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn swap_json(swap: ln_liquid_swap::proto::v1::Swap) -> serde_json::Value {
    let status_str = SwapStatus::try_from(swap.status)
        .ok()
        .map(|s| format!("{s:?}"))
        .unwrap_or_else(|| format!("UNKNOWN({})", swap.status));

    json!({
      "swap_id": swap.swap_id,
      "quote_id": swap.quote_id,
      "status": status_str,
      "bolt11_invoice": swap.bolt11_invoice,
      "payment_hash": swap.payment_hash,
      "liquid": swap.liquid.map(|l| json!({
        "asset_id": l.asset_id,
        "asset_amount": l.asset_amount,
        "fee_subsidy_sats": l.fee_subsidy_sats,
        "refund_lock_height": l.refund_lock_height,
        "p2wsh_address": l.p2wsh_address,
        "funding_txid": l.funding_txid,
        "asset_vout": l.asset_vout,
        "lbtc_vout": l.lbtc_vout,
        "min_funding_confs": l.min_funding_confs,
      })),
    })
}
