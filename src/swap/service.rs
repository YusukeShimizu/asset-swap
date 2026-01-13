use std::str::FromStr as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::lightning::invoice::payment_hash_from_bolt11;
use crate::lightning::ldk::LdkLightningClient;
use crate::liquid::htlc::{HtlcSpec, pubkey_hash160_from_p2wpkh_address};
use crate::liquid::wallet::LiquidWallet;
use crate::proto::v1 as pb;
use crate::swap::store::SqliteSwapStore;
use crate::swap::{SwapRecord, SwapStatus};
use lwk_wollet::elements::{Address, AssetId, Script, Txid};

const MAX_MIN_FUNDING_CONFS: u32 = 6;

#[derive(Debug, Clone)]
pub struct SellerConfig {
    pub sell_asset_id: AssetId,
    pub price_msat_per_asset_unit: u64,
    pub fee_subsidy_sats: u64,
    pub refund_delta_blocks: u32,
    pub invoice_expiry_secs: u32,
    pub seller_key_index: u32,
}

#[derive(Clone)]
pub struct SwapSellerService {
    cfg: SellerConfig,
    ln: LdkLightningClient,
    wallet: Arc<Mutex<LiquidWallet>>,
    store: Arc<Mutex<SqliteSwapStore>>,
}

impl SwapSellerService {
    pub fn new(
        cfg: SellerConfig,
        ln: LdkLightningClient,
        wallet: Arc<Mutex<LiquidWallet>>,
        store: Arc<Mutex<SqliteSwapStore>>,
    ) -> Self {
        Self {
            cfg,
            ln,
            wallet,
            store,
        }
    }

    fn record_to_proto(record: &SwapRecord) -> Result<pb::Swap> {
        let status = match record.status {
            SwapStatus::Created => pb::SwapStatus::Created as i32,
            SwapStatus::Funded => pb::SwapStatus::Funded as i32,
            SwapStatus::Claimed => pb::SwapStatus::Claimed as i32,
            SwapStatus::Refunded => pb::SwapStatus::Refunded as i32,
            SwapStatus::Failed => pb::SwapStatus::Failed as i32,
        };

        let witness_script =
            hex::decode(&record.witness_script_hex).context("decode witness_script_hex")?;

        Ok(pb::Swap {
            swap_id: record.swap_id.clone(),
            bolt11_invoice: record.bolt11_invoice.clone(),
            payment_hash: record.payment_hash.clone(),
            status,
            liquid: Some(pb::LiquidHtlc {
                asset_id: record.asset_id.clone(),
                asset_amount: record.asset_amount,
                fee_subsidy_sats: record.fee_subsidy_sats,
                refund_lock_height: record.refund_lock_height,
                p2wsh_address: record.p2wsh_address.clone(),
                witness_script,
                funding_txid: record.funding_txid.clone(),
                asset_vout: record.asset_vout,
                lbtc_vout: record.lbtc_vout,
                min_funding_confs: record.min_funding_confs,
            }),
        })
    }

    fn wait_for_funding_confirmations(
        wallet: &Arc<Mutex<LiquidWallet>>,
        script_pubkey: &Script,
        txid: &Txid,
        min_confs: u32,
        timeout: Duration,
    ) -> Result<u32> {
        let deadline = Instant::now() + timeout;

        loop {
            let confs = {
                let mut wallet = wallet.lock().expect("wallet mutex poisoned");
                wallet.sync().context("sync wallet")?;
                wallet
                    .tx_confirmations_for_script(script_pubkey, txid)
                    .context("get funding tx confirmations")?
            };

            if let Some(confs) = confs
                && confs >= min_confs
            {
                return Ok(confs);
            }

            if Instant::now() >= deadline {
                anyhow::bail!(
                    "timeout waiting funding confirmations: txid={txid} min_confs={min_confs}"
                );
            }

            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

#[tonic::async_trait]
impl pb::swap_service_server::SwapService for SwapSellerService {
    async fn get_offer(
        &self,
        request: Request<pb::GetOfferRequest>,
    ) -> Result<Response<pb::Offer>, Status> {
        let req = request.into_inner();

        let asset_id = AssetId::from_str(&req.asset_id)
            .map_err(|e| Status::invalid_argument(format!("invalid asset_id: {e}")))?;
        if asset_id != self.cfg.sell_asset_id {
            return Err(Status::invalid_argument("unsupported asset_id"));
        }

        Ok(Response::new(pb::Offer {
            asset_id: self.cfg.sell_asset_id.to_string(),
            price_msat_per_asset_unit: self.cfg.price_msat_per_asset_unit,
            fee_subsidy_sats: self.cfg.fee_subsidy_sats,
            refund_delta_blocks: self.cfg.refund_delta_blocks,
            invoice_expiry_secs: self.cfg.invoice_expiry_secs,
            max_min_funding_confs: MAX_MIN_FUNDING_CONFS,
        }))
    }

    async fn create_swap(
        &self,
        request: Request<pb::CreateSwapRequest>,
    ) -> Result<Response<pb::CreateSwapResponse>, Status> {
        let req = request.into_inner();

        let asset_amount = req.asset_amount;
        if asset_amount == 0 {
            return Err(Status::invalid_argument("asset_amount must be > 0"));
        }

        let asset_id = AssetId::from_str(&req.asset_id)
            .map_err(|e| Status::invalid_argument(format!("invalid asset_id: {e}")))?;
        if asset_id != self.cfg.sell_asset_id {
            return Err(Status::invalid_argument("unsupported asset_id"));
        }

        let buyer_claim_address = Address::from_str(&req.buyer_claim_address)
            .map_err(|e| Status::invalid_argument(format!("invalid buyer_claim_address: {e}")))?;

        let min_funding_confs = req.min_funding_confs;
        if min_funding_confs > MAX_MIN_FUNDING_CONFS {
            return Err(Status::invalid_argument(format!(
                "min_funding_confs must be <= {MAX_MIN_FUNDING_CONFS}"
            )));
        }

        let buyer_pubkey_hash160 = pubkey_hash160_from_p2wpkh_address(&buyer_claim_address)
            .map_err(|e| {
                Status::invalid_argument(format!(
                    "buyer_claim_address must be a P2WPKH address: {e}"
                ))
            })?;

        let params = self
            .wallet
            .lock()
            .expect("wallet mutex poisoned")
            .network()
            .address_params();
        if buyer_claim_address.params != params {
            return Err(Status::invalid_argument(
                "buyer_claim_address network mismatch",
            ));
        }

        let price_msat = asset_amount
            .checked_mul(self.cfg.price_msat_per_asset_unit)
            .ok_or_else(|| Status::invalid_argument("price overflow"))?;

        let max_total_price_msat = req.max_total_price_msat;
        if max_total_price_msat != 0 && price_msat > max_total_price_msat {
            return Err(Status::invalid_argument(format!(
                "price exceeds max_total_price_msat: price_msat={price_msat} max_total_price_msat={max_total_price_msat}"
            )));
        }

        let swap_id = Uuid::new_v4().to_string();
        let invoice = self
            .ln
            .create_invoice(
                price_msat,
                format!("swap:{swap_id}"),
                self.cfg.invoice_expiry_secs,
            )
            .await
            .map_err(|e| Status::internal(format!("create invoice: {e:#}")))?;

        let payment_hash = payment_hash_from_bolt11(&invoice)
            .map_err(|e| Status::internal(format!("parse invoice: {e:#}")))?;
        let payment_hash_hex = hex::encode(payment_hash);

        let wallet = self.wallet.clone();
        let store = self.store.clone();
        let cfg = self.cfg.clone();

        let record = tokio::task::spawn_blocking(move || -> Result<SwapRecord> {
            let (mut record, htlc_script_pubkey, funding_txid) = {
                let mut wallet = wallet.lock().expect("wallet mutex poisoned");

                wallet.sync().context("sync wallet")?;

                let refund_lock_height =
                    wallet.tip_height().saturating_add(cfg.refund_delta_blocks);
                let seller_refund_address = wallet
                    .address_at(cfg.seller_key_index)
                    .context("get seller refund address")?;

                let seller_pubkey_hash160 =
                    pubkey_hash160_from_p2wpkh_address(&seller_refund_address)
                        .context("extract seller pubkey hash")?;

                let spec = HtlcSpec {
                    payment_hash,
                    buyer_pubkey_hash160,
                    seller_pubkey_hash160,
                    refund_lock_height,
                };

                let witness_script = spec.witness_script();
                let htlc_address = spec.p2wsh_address(params);
                let htlc_script_pubkey = htlc_address.script_pubkey();

                let (_tx, funding_txid, asset_vout, lbtc_vout) = wallet
                    .build_and_broadcast_funding(
                        &htlc_address,
                        cfg.sell_asset_id,
                        asset_amount,
                        cfg.fee_subsidy_sats,
                    )
                    .context("fund htlc")?;

                let record = SwapRecord {
                    swap_id: swap_id.clone(),
                    bolt11_invoice: invoice.clone(),
                    payment_hash: payment_hash_hex.clone(),
                    asset_id: cfg.sell_asset_id.to_string(),
                    asset_amount,
                    fee_subsidy_sats: cfg.fee_subsidy_sats,
                    refund_lock_height,
                    p2wsh_address: htlc_address.to_string(),
                    witness_script_hex: hex::encode(witness_script.to_bytes()),
                    funding_txid: funding_txid.to_string(),
                    asset_vout,
                    lbtc_vout,
                    min_funding_confs,
                    status: SwapStatus::Created,
                };

                let mut store = store.lock().expect("store mutex poisoned");
                store.insert_swap(&record).context("persist swap")?;

                Ok::<_, anyhow::Error>((record, htlc_script_pubkey, funding_txid))
            }?;

            let _confs = Self::wait_for_funding_confirmations(
                &wallet,
                &htlc_script_pubkey,
                &funding_txid,
                min_funding_confs,
                Duration::from_secs(300),
            )
            .context("wait funding confirmations")?;

            record.status = SwapStatus::Funded;
            let mut store = store.lock().expect("store mutex poisoned");
            store
                .update_status(&record.swap_id, SwapStatus::Funded)
                .context("update swap status")?;

            Ok(record)
        })
        .await
        .map_err(|e| Status::internal(format!("join: {e}")))?
        .map_err(|e| Status::internal(format!("create swap: {e:#}")))?;

        let swap = Self::record_to_proto(&record)
            .map_err(|e| Status::internal(format!("encode swap: {e:#}")))?;

        Ok(Response::new(pb::CreateSwapResponse { swap: Some(swap) }))
    }

    async fn get_swap(
        &self,
        request: Request<pb::GetSwapRequest>,
    ) -> Result<Response<pb::Swap>, Status> {
        let req = request.into_inner();
        if req.swap_id.trim().is_empty() {
            return Err(Status::invalid_argument("swap_id is required"));
        }

        let record = self
            .store
            .lock()
            .expect("store mutex poisoned")
            .get_swap(&req.swap_id)
            .map_err(|e| Status::internal(format!("get swap: {e:#}")))?
            .ok_or_else(|| Status::not_found("swap not found"))?;

        let swap = Self::record_to_proto(&record)
            .map_err(|e| Status::internal(format!("encode swap: {e:#}")))?;
        Ok(Response::new(swap))
    }
}
