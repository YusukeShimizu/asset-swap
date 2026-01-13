use anyhow::{Context as _, Result};

use ln_liquid_swap::swap::store::SqliteSwapStore;
use ln_liquid_swap::swap::{SwapRecord, SwapStatus};

fn sample_record(swap_id: &str, status: SwapStatus) -> SwapRecord {
    SwapRecord {
        swap_id: swap_id.to_string(),
        bolt11_invoice: format!("invoice:{swap_id}"),
        payment_hash: format!("payment_hash:{swap_id}"),
        asset_id: format!("asset_id:{swap_id}"),
        asset_amount: 1000,
        fee_subsidy_sats: 10_000,
        refund_lock_height: 123,
        p2wsh_address: format!("p2wsh:{swap_id}"),
        witness_script_hex: "00".to_string(),
        funding_txid: format!("funding_txid:{swap_id}"),
        asset_vout: 0,
        lbtc_vout: 1,
        min_funding_confs: 1,
        status,
    }
}

#[test]
fn sqlite_store_insert_get_update_list() -> Result<()> {
    let dir = tempfile::tempdir().context("create tempdir")?;
    let path = dir.path().join("swap_store.sqlite3");

    let mut store = SqliteSwapStore::open(path).context("open sqlite store")?;

    let a = sample_record("swap-a", SwapStatus::Created);
    store.insert_swap(&a).context("insert swap-a")?;

    let got = store
        .get_swap("swap-a")
        .context("get swap-a")?
        .context("swap-a missing")?;
    assert_eq!(got.swap_id, "swap-a");
    assert_eq!(got.status, SwapStatus::Created);

    store
        .update_status("swap-a", SwapStatus::Funded)
        .context("update swap-a status")?;
    let got = store
        .get_swap("swap-a")
        .context("get swap-a after update")?
        .context("swap-a missing after update")?;
    assert_eq!(got.status, SwapStatus::Funded);

    let b = sample_record("swap-b", SwapStatus::Created);
    store.insert_swap(&b).context("insert swap-b")?;

    let swaps = store.list_swaps().context("list swaps")?;
    assert_eq!(swaps.len(), 2);
    assert_eq!(swaps[0].swap_id, "swap-a");
    assert_eq!(swaps[1].swap_id, "swap-b");

    let err = store
        .update_status("missing", SwapStatus::Failed)
        .unwrap_err();
    assert!(err.to_string().contains("swap not found"));

    Ok(())
}
