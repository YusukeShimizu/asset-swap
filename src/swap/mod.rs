pub mod service;
pub mod store;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwapStatus {
    Created,
    Funded,
    Claimed,
    Refunded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapRecord {
    pub swap_id: String,
    pub bolt11_invoice: String,
    pub payment_hash: String,

    pub asset_id: String,
    pub asset_amount: u64,
    pub fee_subsidy_sats: u64,
    pub refund_lock_height: u32,

    pub p2wsh_address: String,
    pub witness_script_hex: String,

    pub funding_txid: String,
    pub asset_vout: u32,
    pub lbtc_vout: u32,
    pub min_funding_confs: u32,

    pub status: SwapStatus,
}
