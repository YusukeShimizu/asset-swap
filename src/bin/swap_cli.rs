use anyhow::{Context as _, Result};
use clap::{Parser as _, Subcommand};
use ln_liquid_swap::proto::v1::swap_service_client::SwapServiceClient;
use ln_liquid_swap::proto::v1::{
    CreateAssetClaimRequest, CreateLightningPaymentRequest, CreateQuoteRequest, CreateSwapRequest,
    GetQuoteRequest, GetSwapRequest, SwapDirection, SwapRole, SwapStatus,
};
use serde_json::json;
use tonic::Request;
use tonic::metadata::MetadataValue;

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum DirectionArg {
    LnToLiquid,
    LiquidToLn,
}

impl DirectionArg {
    fn to_proto(self) -> SwapDirection {
        match self {
            DirectionArg::LnToLiquid => SwapDirection::LnToLiquid,
            DirectionArg::LiquidToLn => SwapDirection::LiquidToLn,
        }
    }
}

#[derive(Debug, clap::Parser)]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:50051")]
    grpc_url: String,

    #[arg(long)]
    auth_token: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    CreateQuote {
        #[arg(long)]
        direction: DirectionArg,

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

        #[arg(long)]
        buyer_liquid_address: String,

        #[arg(long, default_value = "")]
        buyer_bolt11_invoice: String,
    },
    GetSwap {
        #[arg(long)]
        swap_id: String,
    },
    CreateLightningPayment {
        #[arg(long)]
        swap_id: String,

        #[arg(long, default_value_t = 60)]
        payment_timeout_secs: u32,
    },
    CreateAssetClaim {
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
            direction,
            asset_id,
            asset_amount,
            min_funding_confs,
        } => {
            let direction = direction.to_proto();
            let quote = client
                .create_quote(with_auth(
                    &args.auth_token,
                    CreateQuoteRequest {
                        direction: direction as i32,
                        asset_id,
                        asset_amount,
                        min_funding_confs,
                    },
                ))
                .await
                .context("CreateQuote")?
                .into_inner();

            json!({
              "quote_id": quote.quote_id,
              "offer_id": quote.offer_id,
              "asset_id": quote.asset_id,
              "asset_amount": quote.asset_amount,
              "min_funding_confs": quote.min_funding_confs,
              "total_price_msat": quote.total_price_msat,
              "direction": format!("{direction:?}"),
              "parties": quote.parties.map(|p| json!({
                "ln_payer": SwapRole::try_from(p.ln_payer).ok().map(|r| format!("{r:?}")),
                "ln_payee": SwapRole::try_from(p.ln_payee).ok().map(|r| format!("{r:?}")),
                "liquid_funder": SwapRole::try_from(p.liquid_funder).ok().map(|r| format!("{r:?}")),
                "liquid_claimer": SwapRole::try_from(p.liquid_claimer).ok().map(|r| format!("{r:?}")),
                "liquid_refunder": SwapRole::try_from(p.liquid_refunder).ok().map(|r| format!("{r:?}")),
              })),
              "offer": quote.offer.map(|o| json!({
                "asset_id": o.asset_id,
                "supported_directions": o.supported_directions.iter().filter_map(|d| SwapDirection::try_from(*d).ok()).map(|d| format!("{d:?}")).collect::<Vec<_>>(),
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
                .get_quote(with_auth(&args.auth_token, GetQuoteRequest { quote_id }))
                .await
                .context("GetQuote")?
                .into_inner();

            json!({
              "quote_id": quote.quote_id,
              "offer_id": quote.offer_id,
              "asset_id": quote.asset_id,
              "asset_amount": quote.asset_amount,
              "min_funding_confs": quote.min_funding_confs,
              "total_price_msat": quote.total_price_msat,
              "direction": SwapDirection::try_from(quote.direction).ok().map(|d| format!("{d:?}")),
              "parties": quote.parties.map(|p| json!({
                "ln_payer": SwapRole::try_from(p.ln_payer).ok().map(|r| format!("{r:?}")),
                "ln_payee": SwapRole::try_from(p.ln_payee).ok().map(|r| format!("{r:?}")),
                "liquid_funder": SwapRole::try_from(p.liquid_funder).ok().map(|r| format!("{r:?}")),
                "liquid_claimer": SwapRole::try_from(p.liquid_claimer).ok().map(|r| format!("{r:?}")),
                "liquid_refunder": SwapRole::try_from(p.liquid_refunder).ok().map(|r| format!("{r:?}")),
              })),
              "offer": quote.offer.map(|o| json!({
                "asset_id": o.asset_id,
                "supported_directions": o.supported_directions.iter().filter_map(|d| SwapDirection::try_from(*d).ok()).map(|d| format!("{d:?}")).collect::<Vec<_>>(),
                "price_msat_per_asset_unit": o.price_msat_per_asset_unit,
                "fee_subsidy_sats": o.fee_subsidy_sats,
                "refund_delta_blocks": o.refund_delta_blocks,
                "invoice_expiry_secs": o.invoice_expiry_secs,
                "max_min_funding_confs": o.max_min_funding_confs,
              })),
            })
        }
        Command::CreateSwap {
            quote_id,
            buyer_liquid_address,
            buyer_bolt11_invoice,
        } => {
            let swap = client
                .create_swap(with_auth(
                    &args.auth_token,
                    CreateSwapRequest {
                        quote_id,
                        buyer_liquid_address,
                        buyer_bolt11_invoice,
                    },
                ))
                .await
                .context("CreateSwap")?
                .into_inner();

            swap_json(swap)
        }
        Command::GetSwap { swap_id } => {
            let swap = client
                .get_swap(with_auth(&args.auth_token, GetSwapRequest { swap_id }))
                .await
                .context("GetSwap")?
                .into_inner();

            swap_json(swap)
        }
        Command::CreateLightningPayment {
            swap_id,
            payment_timeout_secs,
        } => {
            let resp = client
                .create_lightning_payment(with_auth(
                    &args.auth_token,
                    CreateLightningPaymentRequest {
                        swap_id,
                        payment_timeout_secs,
                    },
                ))
                .await
                .context("CreateLightningPayment")?
                .into_inner();

            json!({
              "payment_id": resp.payment_id,
              "preimage_hex": hex::encode(resp.preimage),
            })
        }
        Command::CreateAssetClaim {
            swap_id,
            claim_fee_sats,
        } => {
            let resp = client
                .create_asset_claim(with_auth(
                    &args.auth_token,
                    CreateAssetClaimRequest {
                        swap_id,
                        claim_fee_sats,
                    },
                ))
                .await
                .context("CreateAssetClaim")?
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
    let direction_str = SwapDirection::try_from(swap.direction)
        .ok()
        .map(|d| format!("{d:?}"))
        .unwrap_or_else(|| format!("UNKNOWN({})", swap.direction));

    json!({
      "swap_id": swap.swap_id,
      "quote_id": swap.quote_id,
      "status": status_str,
      "direction": direction_str,
      "parties": swap.parties.map(|p| json!({
        "ln_payer": SwapRole::try_from(p.ln_payer).ok().map(|r| format!("{r:?}")),
        "ln_payee": SwapRole::try_from(p.ln_payee).ok().map(|r| format!("{r:?}")),
        "liquid_funder": SwapRole::try_from(p.liquid_funder).ok().map(|r| format!("{r:?}")),
        "liquid_claimer": SwapRole::try_from(p.liquid_claimer).ok().map(|r| format!("{r:?}")),
        "liquid_refunder": SwapRole::try_from(p.liquid_refunder).ok().map(|r| format!("{r:?}")),
      })),
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

fn with_auth<T>(auth_token: &str, msg: T) -> Request<T> {
    let mut req = Request::new(msg);
    let header_value = format!("Bearer {auth_token}");
    let meta =
        MetadataValue::try_from(header_value).expect("authorization metadata must be valid ASCII");
    req.metadata_mut().insert("authorization", meta);
    req
}
