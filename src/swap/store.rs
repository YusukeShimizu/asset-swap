use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result};
use rusqlite::{Connection, OptionalExtension as _, params};

use super::{SwapRecord, SwapStatus};

#[derive(Debug)]
pub struct SqliteSwapStore {
    conn: Connection,
    path: PathBuf,
}

impl SqliteSwapStore {
    pub fn open(path: PathBuf) -> Result<Self> {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("create swap store dir {}", dir.display()))?;
        }

        let conn =
            Connection::open(&path).with_context(|| format!("open sqlite {}", path.display()))?;
        conn.busy_timeout(Duration::from_secs(5))
            .context("set sqlite busy_timeout")?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .context("configure sqlite pragmas")?;

        migrate(&conn).context("migrate sqlite schema")?;

        Ok(Self { conn, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn insert_swap(&mut self, record: &SwapRecord) -> Result<()> {
        self.conn
            .execute(
                r#"
INSERT INTO swaps (
  swap_id,
  bolt11_invoice,
  payment_hash,
  asset_id,
  asset_amount,
  fee_subsidy_sats,
  refund_lock_height,
  p2wsh_address,
  witness_script_hex,
  funding_txid,
  asset_vout,
  lbtc_vout,
  min_funding_confs,
  status
) VALUES (
  ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14
)
"#,
                params![
                    &record.swap_id,
                    &record.bolt11_invoice,
                    &record.payment_hash,
                    &record.asset_id,
                    record.asset_amount,
                    record.fee_subsidy_sats,
                    record.refund_lock_height,
                    &record.p2wsh_address,
                    &record.witness_script_hex,
                    &record.funding_txid,
                    record.asset_vout,
                    record.lbtc_vout,
                    record.min_funding_confs,
                    status_to_str(record.status),
                ],
            )
            .with_context(|| format!("insert swap {}", record.swap_id))?;
        Ok(())
    }

    pub fn get_swap(&self, swap_id: &str) -> Result<Option<SwapRecord>> {
        self.conn
            .query_row(
                r#"
SELECT
  swap_id,
  bolt11_invoice,
  payment_hash,
  asset_id,
  asset_amount,
  fee_subsidy_sats,
  refund_lock_height,
  p2wsh_address,
  witness_script_hex,
  funding_txid,
  asset_vout,
  lbtc_vout,
  min_funding_confs,
  status
FROM swaps
WHERE swap_id = ?1
"#,
                params![swap_id],
                |row| {
                    let status_str: String = row.get(13)?;
                    let asset_amount: i64 = row.get(4)?;
                    let fee_subsidy_sats: i64 = row.get(5)?;
                    let refund_lock_height: i64 = row.get(6)?;
                    let asset_vout: i64 = row.get(10)?;
                    let lbtc_vout: i64 = row.get(11)?;
                    let min_funding_confs: i64 = row.get(12)?;
                    let status = status_from_str(&status_str, 13)?;
                    Ok(SwapRecord {
                        swap_id: row.get(0)?,
                        bolt11_invoice: row.get(1)?,
                        payment_hash: row.get(2)?,
                        asset_id: row.get(3)?,
                        asset_amount: u64::try_from(asset_amount).map_err(|_| {
                            rusqlite::Error::FromSqlConversionFailure(
                                4,
                                rusqlite::types::Type::Integer,
                                format!("invalid asset_amount {asset_amount}").into(),
                            )
                        })?,
                        fee_subsidy_sats: u64::try_from(fee_subsidy_sats).map_err(|_| {
                            rusqlite::Error::FromSqlConversionFailure(
                                5,
                                rusqlite::types::Type::Integer,
                                format!("invalid fee_subsidy_sats {fee_subsidy_sats}").into(),
                            )
                        })?,
                        refund_lock_height: u32::try_from(refund_lock_height).map_err(|_| {
                            rusqlite::Error::FromSqlConversionFailure(
                                6,
                                rusqlite::types::Type::Integer,
                                format!("invalid refund_lock_height {refund_lock_height}").into(),
                            )
                        })?,
                        p2wsh_address: row.get(7)?,
                        witness_script_hex: row.get(8)?,
                        funding_txid: row.get(9)?,
                        asset_vout: u32::try_from(asset_vout).map_err(|_| {
                            rusqlite::Error::FromSqlConversionFailure(
                                10,
                                rusqlite::types::Type::Integer,
                                format!("invalid asset_vout {asset_vout}").into(),
                            )
                        })?,
                        lbtc_vout: u32::try_from(lbtc_vout).map_err(|_| {
                            rusqlite::Error::FromSqlConversionFailure(
                                11,
                                rusqlite::types::Type::Integer,
                                format!("invalid lbtc_vout {lbtc_vout}").into(),
                            )
                        })?,
                        min_funding_confs: u32::try_from(min_funding_confs).map_err(|_| {
                            rusqlite::Error::FromSqlConversionFailure(
                                12,
                                rusqlite::types::Type::Integer,
                                format!("invalid min_funding_confs {min_funding_confs}").into(),
                            )
                        })?,
                        status,
                    })
                },
            )
            .optional()
            .with_context(|| format!("get swap {}", swap_id))
    }

    pub fn update_status(&mut self, swap_id: &str, status: SwapStatus) -> Result<()> {
        let rows = self
            .conn
            .execute(
                "UPDATE swaps SET status = ?2 WHERE swap_id = ?1",
                params![swap_id, status_to_str(status)],
            )
            .with_context(|| format!("update swap status {swap_id}"))?;
        anyhow::ensure!(rows == 1, "swap not found: {swap_id}");
        Ok(())
    }

    pub fn list_swaps(&self) -> Result<Vec<SwapRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
SELECT
  swap_id,
  bolt11_invoice,
  payment_hash,
  asset_id,
  asset_amount,
  fee_subsidy_sats,
  refund_lock_height,
  p2wsh_address,
  witness_script_hex,
  funding_txid,
  asset_vout,
  lbtc_vout,
  min_funding_confs,
  status
FROM swaps
ORDER BY swap_id
"#,
            )
            .context("prepare list swaps")?;

        let mut out = Vec::new();
        let rows = stmt
            .query_map([], |row| {
                let status_str: String = row.get(13)?;
                let asset_amount: i64 = row.get(4)?;
                let fee_subsidy_sats: i64 = row.get(5)?;
                let refund_lock_height: i64 = row.get(6)?;
                let asset_vout: i64 = row.get(10)?;
                let lbtc_vout: i64 = row.get(11)?;
                let min_funding_confs: i64 = row.get(12)?;
                let status = status_from_str(&status_str, 13)?;
                Ok(SwapRecord {
                    swap_id: row.get(0)?,
                    bolt11_invoice: row.get(1)?,
                    payment_hash: row.get(2)?,
                    asset_id: row.get(3)?,
                    asset_amount: u64::try_from(asset_amount).map_err(|_| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Integer,
                            format!("invalid asset_amount {asset_amount}").into(),
                        )
                    })?,
                    fee_subsidy_sats: u64::try_from(fee_subsidy_sats).map_err(|_| {
                        rusqlite::Error::FromSqlConversionFailure(
                            5,
                            rusqlite::types::Type::Integer,
                            format!("invalid fee_subsidy_sats {fee_subsidy_sats}").into(),
                        )
                    })?,
                    refund_lock_height: u32::try_from(refund_lock_height).map_err(|_| {
                        rusqlite::Error::FromSqlConversionFailure(
                            6,
                            rusqlite::types::Type::Integer,
                            format!("invalid refund_lock_height {refund_lock_height}").into(),
                        )
                    })?,
                    p2wsh_address: row.get(7)?,
                    witness_script_hex: row.get(8)?,
                    funding_txid: row.get(9)?,
                    asset_vout: u32::try_from(asset_vout).map_err(|_| {
                        rusqlite::Error::FromSqlConversionFailure(
                            10,
                            rusqlite::types::Type::Integer,
                            format!("invalid asset_vout {asset_vout}").into(),
                        )
                    })?,
                    lbtc_vout: u32::try_from(lbtc_vout).map_err(|_| {
                        rusqlite::Error::FromSqlConversionFailure(
                            11,
                            rusqlite::types::Type::Integer,
                            format!("invalid lbtc_vout {lbtc_vout}").into(),
                        )
                    })?,
                    min_funding_confs: u32::try_from(min_funding_confs).map_err(|_| {
                        rusqlite::Error::FromSqlConversionFailure(
                            12,
                            rusqlite::types::Type::Integer,
                            format!("invalid min_funding_confs {min_funding_confs}").into(),
                        )
                    })?,
                    status,
                })
            })
            .context("query list swaps")?;

        for row in rows {
            out.push(row.context("read swap row")?);
        }
        Ok(out)
    }
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS swaps (
  swap_id TEXT PRIMARY KEY,
  bolt11_invoice TEXT NOT NULL,
  payment_hash TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  asset_amount INTEGER NOT NULL,
  fee_subsidy_sats INTEGER NOT NULL,
  refund_lock_height INTEGER NOT NULL,
  p2wsh_address TEXT NOT NULL,
  witness_script_hex TEXT NOT NULL,
  funding_txid TEXT NOT NULL,
  asset_vout INTEGER NOT NULL,
  lbtc_vout INTEGER NOT NULL,
  min_funding_confs INTEGER NOT NULL,
  status TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS swaps_status_idx ON swaps(status);
"#,
    )
    .context("create tables")?;
    Ok(())
}

fn status_to_str(status: SwapStatus) -> &'static str {
    match status {
        SwapStatus::Created => "created",
        SwapStatus::Funded => "funded",
        SwapStatus::Claimed => "claimed",
        SwapStatus::Refunded => "refunded",
        SwapStatus::Failed => "failed",
    }
}

fn status_from_str(s: &str, col: usize) -> rusqlite::Result<SwapStatus> {
    match s {
        "created" => Ok(SwapStatus::Created),
        "funded" => Ok(SwapStatus::Funded),
        "claimed" => Ok(SwapStatus::Claimed),
        "refunded" => Ok(SwapStatus::Refunded),
        "failed" => Ok(SwapStatus::Failed),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            col,
            rusqlite::types::Type::Text,
            format!("unknown swap status: {other}").into(),
        )),
    }
}
