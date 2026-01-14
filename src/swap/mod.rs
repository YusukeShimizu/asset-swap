pub mod service;
pub mod store;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwapStatus {
    Created,
    Funded,
    Paid,
    Claimed,
    Refunded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapRecord {
    pub swap_id: String,
    pub quote_id: String,
    pub bolt11_invoice: String,
    pub payment_hash: String,

    pub asset_id: String,
    pub asset_amount: u64,
    pub total_price_msat: u64,
    pub buyer_claim_address: String,
    pub fee_subsidy_sats: u64,
    pub refund_lock_height: u32,

    pub p2wsh_address: String,
    pub witness_script_hex: String,

    pub funding_txid: String,
    pub asset_vout: u32,
    pub lbtc_vout: u32,
    pub min_funding_confs: u32,

    pub ln_payment_id: Option<String>,
    pub ln_preimage_hex: Option<String>,
    pub claim_txid: Option<String>,

    pub status: SwapStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteRecord {
    pub quote_id: String,
    pub offer_id: String,

    pub asset_id: String,
    pub asset_amount: u64,
    pub buyer_claim_address: String,
    pub min_funding_confs: u32,
    pub total_price_msat: u64,

    pub price_msat_per_asset_unit: u64,
    pub fee_subsidy_sats: u64,
    pub refund_delta_blocks: u32,
    pub invoice_expiry_secs: u32,
    pub max_min_funding_confs: u32,

    pub swap_id: Option<String>,
}
