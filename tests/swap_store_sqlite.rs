use anyhow::{Context as _, Result};

use ln_liquid_swap::swap::store::SqliteStore;
use ln_liquid_swap::swap::{QuoteRecord, SwapDirection, SwapRecord, SwapStatus};

fn sample_quote(quote_id: &str) -> QuoteRecord {
    QuoteRecord {
        quote_id: quote_id.to_string(),
        offer_id: format!("offer_id:{quote_id}"),
        direction: SwapDirection::LnToLiquid,
        asset_id: format!("asset_id:{quote_id}"),
        asset_amount: 1000,
        min_funding_confs: 1,
        total_price_msat: 1_000_000,
        price_msat_per_asset_unit: 1000,
        fee_subsidy_sats: 10_000,
        refund_delta_blocks: 144,
        invoice_expiry_secs: 3600,
        max_min_funding_confs: 6,
        swap_id: None,
    }
}

fn sample_swap(swap_id: &str, quote_id: &str, status: SwapStatus) -> SwapRecord {
    SwapRecord {
        swap_id: swap_id.to_string(),
        quote_id: quote_id.to_string(),
        direction: SwapDirection::LnToLiquid,
        bolt11_invoice: format!("invoice:{swap_id}"),
        payment_hash: format!("payment_hash:{swap_id}"),
        asset_id: format!("asset_id:{swap_id}"),
        asset_amount: 1000,
        total_price_msat: 1_000_000,
        buyer_liquid_address: format!("buyer_liquid_address:{swap_id}"),
        fee_subsidy_sats: 10_000,
        refund_lock_height: 123,
        p2wsh_address: format!("p2wsh:{swap_id}"),
        witness_script_hex: "00".to_string(),
        funding_txid: format!("funding_txid:{swap_id}"),
        asset_vout: 0,
        lbtc_vout: 1,
        min_funding_confs: 1,
        ln_payment_id: None,
        ln_preimage_hex: None,
        claim_txid: None,
        status,
    }
}

#[test]
fn sqlite_store_insert_get_update_list() -> Result<()> {
    let dir = tempfile::tempdir().context("create tempdir")?;
    let path = dir.path().join("swap_store.sqlite3");

    let mut store = SqliteStore::open(path).context("open sqlite store")?;

    let q = sample_quote("quote-a");
    store.insert_quote(&q).context("insert quote-a")?;
    let got_q = store
        .get_quote("quote-a")
        .context("get quote-a")?
        .context("quote-a missing")?;
    assert_eq!(got_q.quote_id, "quote-a");

    let a = sample_swap("swap-a", "quote-a", SwapStatus::Created);
    store.insert_swap(&a).context("insert swap-a")?;

    let got = store
        .get_swap("swap-a")
        .context("get swap-a")?
        .context("swap-a missing")?;
    assert_eq!(got.swap_id, "swap-a");
    assert_eq!(got.status, SwapStatus::Created);

    store
        .update_swap_status("swap-a", SwapStatus::Funded)
        .context("update swap-a status")?;
    let got = store
        .get_swap("swap-a")
        .context("get swap-a after update")?
        .context("swap-a missing after update")?;
    assert_eq!(got.status, SwapStatus::Funded);

    store
        .upsert_swap_payment("swap-a", "payment-a", "00", SwapStatus::Paid)
        .context("set swap-a payment")?;
    let got = store
        .get_swap("swap-a")
        .context("get swap-a after payment")?
        .context("swap-a missing after payment")?;
    assert_eq!(got.status, SwapStatus::Paid);
    assert_eq!(got.ln_payment_id.as_deref(), Some("payment-a"));
    assert_eq!(got.ln_preimage_hex.as_deref(), Some("00"));

    store
        .upsert_swap_claim("swap-a", "claim-a", SwapStatus::Claimed)
        .context("set swap-a claim")?;
    let got = store
        .get_swap("swap-a")
        .context("get swap-a after claim")?
        .context("swap-a missing after claim")?;
    assert_eq!(got.status, SwapStatus::Claimed);
    assert_eq!(got.claim_txid.as_deref(), Some("claim-a"));

    let b = sample_swap("swap-b", "quote-a", SwapStatus::Created);
    store.insert_swap(&b).context("insert swap-b")?;

    let swaps = store.list_swaps().context("list swaps")?;
    assert_eq!(swaps.len(), 2);
    assert_eq!(swaps[0].swap_id, "swap-a");
    assert_eq!(swaps[1].swap_id, "swap-b");

    let err = store
        .update_swap_status("missing", SwapStatus::Failed)
        .unwrap_err();
    assert!(err.to_string().contains("swap not found"));

    Ok(())
}
