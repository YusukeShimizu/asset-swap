use std::str::FromStr as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use lwk_wollet::elements::bitcoin::hashes::{Hash as _, sha256};
use lwk_wollet::elements::{Address, AssetId, Script, Txid};
use prost::Message as _;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::lightning::invoice::payment_hash_from_bolt11;
use crate::lightning::ldk::LdkLightningClient;
use crate::liquid::htlc::{
    HtlcFunding, HtlcSpec, claim_tx_from_witness_script, pubkey_hash160_from_p2wpkh_address,
    sha256_preimage,
};
use crate::liquid::keys::derive_secret_key;
use crate::liquid::wallet::LiquidWallet;
use crate::proto::v1 as pb;
use crate::swap::store::SqliteStore;
use crate::swap::{QuoteRecord, SwapRecord, SwapStatus};

const MAX_MIN_FUNDING_CONFS: u32 = 6;
const DEFAULT_PAYMENT_TIMEOUT_SECS: u64 = 60;
const DEFAULT_CLAIM_FEE_SATS: u64 = 500;

#[derive(Debug, Clone)]
pub struct SwapServiceConfig {
    pub sell_asset_id: AssetId,
    pub price_msat_per_asset_unit: u64,
    pub fee_subsidy_sats: u64,
    pub refund_delta_blocks: u32,
    pub invoice_expiry_secs: u32,
    pub seller_key_index: u32,
    pub buyer_key_index: u32,
}

#[derive(Clone)]
pub struct SwapServiceImpl {
    cfg: SwapServiceConfig,
    ln: LdkLightningClient,
    wallet: Arc<Mutex<LiquidWallet>>,
    store: Arc<Mutex<SqliteStore>>,
}

impl SwapServiceImpl {
    pub fn new(
        cfg: SwapServiceConfig,
        ln: LdkLightningClient,
        wallet: Arc<Mutex<LiquidWallet>>,
        store: Arc<Mutex<SqliteStore>>,
    ) -> Self {
        Self {
            cfg,
            ln,
            wallet,
            store,
        }
    }

    fn current_offer(&self) -> pb::Offer {
        pb::Offer {
            asset_id: self.cfg.sell_asset_id.to_string(),
            price_msat_per_asset_unit: self.cfg.price_msat_per_asset_unit,
            fee_subsidy_sats: self.cfg.fee_subsidy_sats,
            refund_delta_blocks: self.cfg.refund_delta_blocks,
            invoice_expiry_secs: self.cfg.invoice_expiry_secs,
            max_min_funding_confs: MAX_MIN_FUNDING_CONFS,
        }
    }

    fn offer_id(offer: &pb::Offer) -> String {
        let mut buf = Vec::new();
        offer
            .encode(&mut buf)
            .expect("prost Offer encoding must not fail");
        hex::encode(sha256::Hash::hash(&buf).to_byte_array())
    }

    fn quote_record_to_proto(record: &QuoteRecord) -> pb::Quote {
        pb::Quote {
            quote_id: record.quote_id.clone(),
            offer_id: record.offer_id.clone(),
            offer: Some(pb::Offer {
                asset_id: record.asset_id.clone(),
                price_msat_per_asset_unit: record.price_msat_per_asset_unit,
                fee_subsidy_sats: record.fee_subsidy_sats,
                refund_delta_blocks: record.refund_delta_blocks,
                invoice_expiry_secs: record.invoice_expiry_secs,
                max_min_funding_confs: record.max_min_funding_confs,
            }),
            buyer_claim_address: record.buyer_claim_address.clone(),
            asset_id: record.asset_id.clone(),
            asset_amount: record.asset_amount,
            min_funding_confs: record.min_funding_confs,
            total_price_msat: record.total_price_msat,
        }
    }

    fn swap_record_to_proto(record: &SwapRecord) -> Result<pb::Swap> {
        let status = match record.status {
            SwapStatus::Created => pb::SwapStatus::Created as i32,
            SwapStatus::Funded => pb::SwapStatus::Funded as i32,
            SwapStatus::Paid => pb::SwapStatus::Paid as i32,
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
            quote_id: record.quote_id.clone(),
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
impl pb::swap_service_server::SwapService for SwapServiceImpl {
    async fn create_quote(
        &self,
        request: Request<pb::CreateQuoteRequest>,
    ) -> Result<Response<pb::Quote>, Status> {
        let req = request.into_inner();

        let asset_amount = req.asset_amount;
        if asset_amount == 0 {
            return Err(Status::invalid_argument("asset_amount must be > 0"));
        }

        let asset_id = AssetId::from_str(&req.asset_id)
            .map_err(|e| Status::invalid_argument(format!("invalid asset_id: {e}")))?;
        if asset_id != self.cfg.sell_asset_id {
            return Err(Status::failed_precondition("unsupported asset_id"));
        }

        let min_funding_confs = req.min_funding_confs;
        if min_funding_confs > MAX_MIN_FUNDING_CONFS {
            return Err(Status::invalid_argument(format!(
                "min_funding_confs must be <= {MAX_MIN_FUNDING_CONFS}"
            )));
        }

        let buyer_claim_address = self
            .wallet
            .lock()
            .expect("wallet mutex poisoned")
            .address_at(self.cfg.buyer_key_index)
            .map_err(|e| Status::internal(format!("get buyer claim address: {e:#}")))?;

        let offer = self.current_offer();
        let offer_id = Self::offer_id(&offer);
        let total_price_msat = asset_amount
            .checked_mul(offer.price_msat_per_asset_unit)
            .ok_or_else(|| Status::invalid_argument("total_price_msat overflow"))?;

        let quote_id = Uuid::new_v4().to_string();
        let record = QuoteRecord {
            quote_id: quote_id.clone(),
            offer_id: offer_id.clone(),
            asset_id: offer.asset_id.clone(),
            asset_amount,
            buyer_claim_address: buyer_claim_address.to_string(),
            min_funding_confs,
            total_price_msat,
            price_msat_per_asset_unit: offer.price_msat_per_asset_unit,
            fee_subsidy_sats: offer.fee_subsidy_sats,
            refund_delta_blocks: offer.refund_delta_blocks,
            invoice_expiry_secs: offer.invoice_expiry_secs,
            max_min_funding_confs: offer.max_min_funding_confs,
            swap_id: None,
        };

        self.store
            .lock()
            .expect("store mutex poisoned")
            .insert_quote(&record)
            .map_err(|e| Status::internal(format!("persist quote: {e:#}")))?;

        Ok(Response::new(Self::quote_record_to_proto(&record)))
    }

    async fn get_quote(
        &self,
        request: Request<pb::GetQuoteRequest>,
    ) -> Result<Response<pb::Quote>, Status> {
        let req = request.into_inner();
        if req.quote_id.trim().is_empty() {
            return Err(Status::invalid_argument("quote_id is required"));
        }

        let record = self
            .store
            .lock()
            .expect("store mutex poisoned")
            .get_quote(&req.quote_id)
            .map_err(|e| Status::internal(format!("get quote: {e:#}")))?
            .ok_or_else(|| Status::not_found("quote not found"))?;

        Ok(Response::new(Self::quote_record_to_proto(&record)))
    }

    async fn create_swap(
        &self,
        request: Request<pb::CreateSwapRequest>,
    ) -> Result<Response<pb::Swap>, Status> {
        let req = request.into_inner();
        if req.quote_id.trim().is_empty() {
            return Err(Status::invalid_argument("quote_id is required"));
        }

        let quote = self
            .store
            .lock()
            .expect("store mutex poisoned")
            .get_quote(&req.quote_id)
            .map_err(|e| Status::internal(format!("get quote: {e:#}")))?
            .ok_or_else(|| Status::not_found("quote not found"))?;

        if let Some(existing_swap_id) = quote.swap_id.clone() {
            let record = self
                .store
                .lock()
                .expect("store mutex poisoned")
                .get_swap(&existing_swap_id)
                .map_err(|e| Status::internal(format!("get swap: {e:#}")))?
                .ok_or_else(|| Status::internal("quote refers to missing swap"))?;
            let swap = Self::swap_record_to_proto(&record)
                .map_err(|e| Status::internal(format!("encode swap: {e:#}")))?;
            return Ok(Response::new(swap));
        }

        let current_offer = self.current_offer();
        let current_offer_id = Self::offer_id(&current_offer);
        if current_offer_id != quote.offer_id {
            return Err(Status::failed_precondition(
                "offer changed since quoting; retry CreateQuote",
            ));
        }

        let asset_id = AssetId::from_str(&quote.asset_id)
            .map_err(|e| Status::invalid_argument(format!("invalid asset_id: {e}")))?;
        if asset_id != self.cfg.sell_asset_id {
            return Err(Status::failed_precondition("unsupported asset_id"));
        }

        let buyer_claim_address = Address::from_str(&quote.buyer_claim_address)
            .map_err(|e| Status::invalid_argument(format!("invalid buyer_claim_address: {e}")))?;

        let min_funding_confs = quote.min_funding_confs;
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

        let swap_id = Uuid::new_v4().to_string();
        let invoice = self
            .ln
            .create_invoice(
                quote.total_price_msat,
                format!("swap:{swap_id}"),
                quote.invoice_expiry_secs,
            )
            .await
            .map_err(|e| Status::internal(format!("create invoice: {e:#}")))?;

        let payment_hash = payment_hash_from_bolt11(&invoice)
            .map_err(|e| Status::internal(format!("parse invoice: {e:#}")))?;
        let payment_hash_hex = hex::encode(payment_hash);

        let wallet = self.wallet.clone();
        let store = self.store.clone();
        let cfg = self.cfg.clone();
        let quote_id = quote.quote_id.clone();

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
                        quote.asset_amount,
                        cfg.fee_subsidy_sats,
                    )
                    .context("fund htlc")?;

                let record = SwapRecord {
                    swap_id: swap_id.clone(),
                    quote_id: quote_id.clone(),
                    bolt11_invoice: invoice.clone(),
                    payment_hash: payment_hash_hex.clone(),
                    asset_id: cfg.sell_asset_id.to_string(),
                    asset_amount: quote.asset_amount,
                    total_price_msat: quote.total_price_msat,
                    buyer_claim_address: buyer_claim_address.to_string(),
                    fee_subsidy_sats: cfg.fee_subsidy_sats,
                    refund_lock_height,
                    p2wsh_address: htlc_address.to_string(),
                    witness_script_hex: hex::encode(witness_script.to_bytes()),
                    funding_txid: funding_txid.to_string(),
                    asset_vout,
                    lbtc_vout,
                    min_funding_confs,
                    ln_payment_id: None,
                    ln_preimage_hex: None,
                    claim_txid: None,
                    status: SwapStatus::Created,
                };

                let mut store = store.lock().expect("store mutex poisoned");
                store.insert_swap(&record).context("persist swap")?;
                store
                    .set_quote_swap_id(&quote_id, &swap_id)
                    .context("link quote to swap")?;

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
                .update_swap_status(&record.swap_id, SwapStatus::Funded)
                .context("update swap status")?;

            Ok(record)
        })
        .await
        .map_err(|e| Status::internal(format!("join: {e}")))?
        .map_err(|e| Status::internal(format!("create swap: {e:#}")))?;

        let swap = Self::swap_record_to_proto(&record)
            .map_err(|e| Status::internal(format!("encode swap: {e:#}")))?;

        Ok(Response::new(swap))
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

        let swap = Self::swap_record_to_proto(&record)
            .map_err(|e| Status::internal(format!("encode swap: {e:#}")))?;
        Ok(Response::new(swap))
    }

    async fn create_lightning_payment(
        &self,
        request: Request<pb::CreateLightningPaymentRequest>,
    ) -> Result<Response<pb::LightningPayment>, Status> {
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

        if let (Some(payment_id), Some(preimage_hex)) =
            (record.ln_payment_id.clone(), record.ln_preimage_hex.clone())
        {
            let preimage = hex::decode(preimage_hex)
                .map_err(|e| Status::internal(format!("decode preimage_hex: {e:#}")))?;
            return Ok(Response::new(pb::LightningPayment {
                payment_id,
                preimage,
            }));
        }

        if !matches!(record.status, SwapStatus::Funded) {
            return Err(Status::failed_precondition("swap is not funded"));
        }

        let payment_id = self
            .ln
            .pay_invoice(record.bolt11_invoice.clone())
            .await
            .map_err(|e| Status::internal(format!("pay invoice: {e:#}")))?;

        let timeout_secs = if req.payment_timeout_secs == 0 {
            DEFAULT_PAYMENT_TIMEOUT_SECS
        } else {
            u64::from(req.payment_timeout_secs)
        };
        let preimage = self
            .ln
            .wait_preimage(&payment_id, Duration::from_secs(timeout_secs))
            .await
            .map_err(|e| Status::internal(format!("wait preimage: {e:#}")))?;

        let expected_payment_hash =
            hex::decode(&record.payment_hash).map_err(|e| Status::internal(format!("{e:#}")))?;
        let expected_payment_hash: [u8; 32] = expected_payment_hash
            .try_into()
            .map_err(|_| Status::internal("payment_hash must be 32 bytes"))?;
        let got_payment_hash = sha256_preimage(&preimage);
        if got_payment_hash != expected_payment_hash {
            return Err(Status::internal("preimage hash mismatch"));
        }

        let preimage_hex = hex::encode(preimage);
        self.store
            .lock()
            .expect("store mutex poisoned")
            .upsert_swap_payment(
                &record.swap_id,
                &payment_id,
                &preimage_hex,
                SwapStatus::Paid,
            )
            .map_err(|e| Status::internal(format!("persist payment: {e:#}")))?;

        Ok(Response::new(pb::LightningPayment {
            payment_id,
            preimage: hex::decode(preimage_hex)
                .expect("hex encoding/decoding of preimage must roundtrip"),
        }))
    }

    async fn create_asset_claim(
        &self,
        request: Request<pb::CreateAssetClaimRequest>,
    ) -> Result<Response<pb::AssetClaim>, Status> {
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

        if let Some(claim_txid) = record.claim_txid.clone() {
            return Ok(Response::new(pb::AssetClaim { claim_txid }));
        }

        let preimage_hex = record
            .ln_preimage_hex
            .clone()
            .ok_or_else(|| Status::failed_precondition("swap has not been paid yet"))?;
        let preimage: [u8; 32] = hex::decode(preimage_hex)
            .map_err(|e| Status::internal(format!("decode preimage_hex: {e:#}")))?
            .try_into()
            .map_err(|_| Status::internal("preimage must be 32 bytes"))?;

        let claim_fee_sats = if req.claim_fee_sats == 0 {
            DEFAULT_CLAIM_FEE_SATS
        } else {
            req.claim_fee_sats
        };

        let wallet = self.wallet.clone();
        let store = self.store.clone();
        let cfg = self.cfg.clone();
        let record_swap_id = record.swap_id.clone();

        let claim_txid = tokio::task::spawn_blocking(move || -> Result<String> {
            let mut wallet = wallet.lock().expect("wallet mutex poisoned");
            wallet.sync().context("sync liquid wallet")?;

            let buyer_receive = wallet
                .address_at(cfg.buyer_key_index)
                .context("get buyer receive address")?;
            anyhow::ensure!(
                buyer_receive.to_string() == record.buyer_claim_address,
                "buyer_claim_address mismatch"
            );

            let buyer_secret_key = derive_secret_key(wallet.signer(), cfg.buyer_key_index)
                .context("derive buyer secret key")?;

            let witness_script: Script = record
                .witness_script_hex
                .parse()
                .map_err(|e| anyhow::anyhow!("parse witness_script_hex: {e:?}"))?;

            let funding_txid =
                Txid::from_str(&record.funding_txid).context("parse funding_txid")?;
            let asset_id = AssetId::from_str(&record.asset_id).context("parse asset_id")?;
            let funding = HtlcFunding {
                funding_txid,
                asset_vout: record.asset_vout,
                lbtc_vout: record.lbtc_vout,
                asset_id,
                asset_amount: record.asset_amount,
                policy_asset: wallet.policy_asset(),
                fee_subsidy_sats: record.fee_subsidy_sats,
            };

            let tx = claim_tx_from_witness_script(
                &witness_script,
                &funding,
                &buyer_receive,
                &buyer_secret_key,
                preimage,
                claim_fee_sats,
            )
            .context("build claim tx")?;

            let txid = wallet
                .broadcast_transaction(&tx)
                .context("broadcast claim tx")?;

            let mut store = store.lock().expect("store mutex poisoned");
            store
                .upsert_swap_claim(&record_swap_id, &txid.to_string(), SwapStatus::Claimed)
                .context("persist claim")?;

            Ok(txid.to_string())
        })
        .await
        .map_err(|e| Status::internal(format!("join: {e}")))?
        .map_err(|e| Status::internal(format!("claim asset: {e:#}")))?;

        Ok(Response::new(pb::AssetClaim { claim_txid }))
    }
}
