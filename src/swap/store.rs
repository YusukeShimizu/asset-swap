use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result};
use rusqlite::{Connection, OptionalExtension as _, params};

use super::{QuoteRecord, SwapRecord, SwapStatus};

#[derive(Debug)]
pub struct SqliteStore {
    conn: Connection,
    path: PathBuf,
}

impl SqliteStore {
    pub fn open(path: PathBuf) -> Result<Self> {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("create sqlite store dir {}", dir.display()))?;
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

    pub fn insert_quote(&mut self, record: &QuoteRecord) -> Result<()> {
        self.conn
            .execute(
                r#"
INSERT INTO quotes (
  quote_id,
  offer_id,
  asset_id,
  asset_amount,
  buyer_claim_address,
  min_funding_confs,
  total_price_msat,
  price_msat_per_asset_unit,
  fee_subsidy_sats,
  refund_delta_blocks,
  invoice_expiry_secs,
  max_min_funding_confs,
  swap_id
) VALUES (
  ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13
)
"#,
                params![
                    &record.quote_id,
                    &record.offer_id,
                    &record.asset_id,
                    record.asset_amount,
                    &record.buyer_claim_address,
                    record.min_funding_confs,
                    record.total_price_msat,
                    record.price_msat_per_asset_unit,
                    record.fee_subsidy_sats,
                    record.refund_delta_blocks,
                    record.invoice_expiry_secs,
                    record.max_min_funding_confs,
                    record.swap_id.as_deref(),
                ],
            )
            .with_context(|| format!("insert quote {}", record.quote_id))?;
        Ok(())
    }

    pub fn get_quote(&self, quote_id: &str) -> Result<Option<QuoteRecord>> {
        self.conn
            .query_row(
                r#"
SELECT
  quote_id,
  offer_id,
  asset_id,
  asset_amount,
  buyer_claim_address,
  min_funding_confs,
  total_price_msat,
  price_msat_per_asset_unit,
  fee_subsidy_sats,
  refund_delta_blocks,
  invoice_expiry_secs,
  max_min_funding_confs,
  swap_id
FROM quotes
WHERE quote_id = ?1
"#,
                params![quote_id],
                |row| {
                    let asset_amount: i64 = row.get(3)?;
                    let min_funding_confs: i64 = row.get(5)?;
                    let total_price_msat: i64 = row.get(6)?;
                    let price_msat_per_asset_unit: i64 = row.get(7)?;
                    let fee_subsidy_sats: i64 = row.get(8)?;
                    let refund_delta_blocks: i64 = row.get(9)?;
                    let invoice_expiry_secs: i64 = row.get(10)?;
                    let max_min_funding_confs: i64 = row.get(11)?;

                    Ok(QuoteRecord {
                        quote_id: row.get(0)?,
                        offer_id: row.get(1)?,
                        asset_id: row.get(2)?,
                        asset_amount: u64::try_from(asset_amount).map_err(|_| {
                            rusqlite::Error::FromSqlConversionFailure(
                                3,
                                rusqlite::types::Type::Integer,
                                format!("invalid asset_amount {asset_amount}").into(),
                            )
                        })?,
                        buyer_claim_address: row.get(4)?,
                        min_funding_confs: u32::try_from(min_funding_confs).map_err(|_| {
                            rusqlite::Error::FromSqlConversionFailure(
                                5,
                                rusqlite::types::Type::Integer,
                                format!("invalid min_funding_confs {min_funding_confs}").into(),
                            )
                        })?,
                        total_price_msat: u64::try_from(total_price_msat).map_err(|_| {
                            rusqlite::Error::FromSqlConversionFailure(
                                6,
                                rusqlite::types::Type::Integer,
                                format!("invalid total_price_msat {total_price_msat}").into(),
                            )
                        })?,
                        price_msat_per_asset_unit: u64::try_from(price_msat_per_asset_unit)
                            .map_err(|_| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    7,
                                    rusqlite::types::Type::Integer,
                                    format!(
                                        "invalid price_msat_per_asset_unit {price_msat_per_asset_unit}"
                                    )
                                    .into(),
                                )
                            })?,
                        fee_subsidy_sats: u64::try_from(fee_subsidy_sats).map_err(|_| {
                            rusqlite::Error::FromSqlConversionFailure(
                                8,
                                rusqlite::types::Type::Integer,
                                format!("invalid fee_subsidy_sats {fee_subsidy_sats}").into(),
                            )
                        })?,
                        refund_delta_blocks: u32::try_from(refund_delta_blocks).map_err(|_| {
                            rusqlite::Error::FromSqlConversionFailure(
                                9,
                                rusqlite::types::Type::Integer,
                                format!("invalid refund_delta_blocks {refund_delta_blocks}").into(),
                            )
                        })?,
                        invoice_expiry_secs: u32::try_from(invoice_expiry_secs).map_err(|_| {
                            rusqlite::Error::FromSqlConversionFailure(
                                10,
                                rusqlite::types::Type::Integer,
                                format!("invalid invoice_expiry_secs {invoice_expiry_secs}").into(),
                            )
                        })?,
                        max_min_funding_confs: u32::try_from(max_min_funding_confs).map_err(|_| {
                            rusqlite::Error::FromSqlConversionFailure(
                                11,
                                rusqlite::types::Type::Integer,
                                format!("invalid max_min_funding_confs {max_min_funding_confs}")
                                    .into(),
                            )
                        })?,
                        swap_id: row.get(12)?,
                    })
                },
            )
            .optional()
            .with_context(|| format!("get quote {}", quote_id))
    }

    pub fn set_quote_swap_id(&mut self, quote_id: &str, swap_id: &str) -> Result<()> {
        let rows = self
            .conn
            .execute(
                "UPDATE quotes SET swap_id = ?2 WHERE quote_id = ?1",
                params![quote_id, swap_id],
            )
            .with_context(|| format!("set quote swap_id quote_id={quote_id}"))?;
        anyhow::ensure!(rows == 1, "quote not found: {quote_id}");
        Ok(())
    }

    pub fn insert_swap(&mut self, record: &SwapRecord) -> Result<()> {
        self.conn
            .execute(
                r#"
INSERT INTO swaps (
  swap_id,
  quote_id,
  bolt11_invoice,
  payment_hash,
  asset_id,
  asset_amount,
  total_price_msat,
  buyer_claim_address,
  fee_subsidy_sats,
  refund_lock_height,
  p2wsh_address,
  witness_script_hex,
  funding_txid,
  asset_vout,
  lbtc_vout,
  min_funding_confs,
  ln_payment_id,
  ln_preimage_hex,
  claim_txid,
  status
) VALUES (
  ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20
)
"#,
                params![
                    &record.swap_id,
                    &record.quote_id,
                    &record.bolt11_invoice,
                    &record.payment_hash,
                    &record.asset_id,
                    record.asset_amount,
                    record.total_price_msat,
                    &record.buyer_claim_address,
                    record.fee_subsidy_sats,
                    record.refund_lock_height,
                    &record.p2wsh_address,
                    &record.witness_script_hex,
                    &record.funding_txid,
                    record.asset_vout,
                    record.lbtc_vout,
                    record.min_funding_confs,
                    record.ln_payment_id.as_deref(),
                    record.ln_preimage_hex.as_deref(),
                    record.claim_txid.as_deref(),
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
  quote_id,
  bolt11_invoice,
  payment_hash,
  asset_id,
  asset_amount,
  total_price_msat,
  buyer_claim_address,
  fee_subsidy_sats,
  refund_lock_height,
  p2wsh_address,
  witness_script_hex,
  funding_txid,
  asset_vout,
  lbtc_vout,
  min_funding_confs,
  ln_payment_id,
  ln_preimage_hex,
  claim_txid,
  status
FROM swaps
WHERE swap_id = ?1
"#,
                params![swap_id],
                row_to_swap_record,
            )
            .optional()
            .with_context(|| format!("get swap {}", swap_id))
    }

    pub fn update_swap_status(&mut self, swap_id: &str, status: SwapStatus) -> Result<()> {
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

    pub fn upsert_swap_payment(
        &mut self,
        swap_id: &str,
        payment_id: &str,
        preimage_hex: &str,
        status: SwapStatus,
    ) -> Result<()> {
        let rows = self
            .conn
            .execute(
                r#"
UPDATE swaps
SET ln_payment_id = ?2,
    ln_preimage_hex = ?3,
    status = ?4
WHERE swap_id = ?1
"#,
                params![swap_id, payment_id, preimage_hex, status_to_str(status)],
            )
            .with_context(|| format!("update swap payment {swap_id}"))?;
        anyhow::ensure!(rows == 1, "swap not found: {swap_id}");
        Ok(())
    }

    pub fn upsert_swap_claim(
        &mut self,
        swap_id: &str,
        claim_txid: &str,
        status: SwapStatus,
    ) -> Result<()> {
        let rows = self
            .conn
            .execute(
                r#"
UPDATE swaps
SET claim_txid = ?2,
    status = ?3
WHERE swap_id = ?1
"#,
                params![swap_id, claim_txid, status_to_str(status)],
            )
            .with_context(|| format!("update swap claim {swap_id}"))?;
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
  quote_id,
  bolt11_invoice,
  payment_hash,
  asset_id,
  asset_amount,
  total_price_msat,
  buyer_claim_address,
  fee_subsidy_sats,
  refund_lock_height,
  p2wsh_address,
  witness_script_hex,
  funding_txid,
  asset_vout,
  lbtc_vout,
  min_funding_confs,
  ln_payment_id,
  ln_preimage_hex,
  claim_txid,
  status
FROM swaps
ORDER BY swap_id
"#,
            )
            .context("prepare list swaps")?;

        let mut out = Vec::new();
        let rows = stmt
            .query_map([], row_to_swap_record)
            .context("query list swaps")?;

        for row in rows {
            out.push(row.context("read swap row")?);
        }
        Ok(out)
    }
}

fn row_to_swap_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<SwapRecord> {
    let asset_amount: i64 = row.get(5)?;
    let total_price_msat: i64 = row.get(6)?;
    let fee_subsidy_sats: i64 = row.get(8)?;
    let refund_lock_height: i64 = row.get(9)?;
    let asset_vout: i64 = row.get(13)?;
    let lbtc_vout: i64 = row.get(14)?;
    let min_funding_confs: i64 = row.get(15)?;

    let status_str: String = row.get(19)?;
    let status = status_from_str(&status_str, 19)?;

    Ok(SwapRecord {
        swap_id: row.get(0)?,
        quote_id: row.get(1)?,
        bolt11_invoice: row.get(2)?,
        payment_hash: row.get(3)?,
        asset_id: row.get(4)?,
        asset_amount: u64::try_from(asset_amount).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Integer,
                format!("invalid asset_amount {asset_amount}").into(),
            )
        })?,
        total_price_msat: u64::try_from(total_price_msat).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Integer,
                format!("invalid total_price_msat {total_price_msat}").into(),
            )
        })?,
        buyer_claim_address: row.get(7)?,
        fee_subsidy_sats: u64::try_from(fee_subsidy_sats).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                8,
                rusqlite::types::Type::Integer,
                format!("invalid fee_subsidy_sats {fee_subsidy_sats}").into(),
            )
        })?,
        refund_lock_height: u32::try_from(refund_lock_height).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                9,
                rusqlite::types::Type::Integer,
                format!("invalid refund_lock_height {refund_lock_height}").into(),
            )
        })?,
        p2wsh_address: row.get(10)?,
        witness_script_hex: row.get(11)?,
        funding_txid: row.get(12)?,
        asset_vout: u32::try_from(asset_vout).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                13,
                rusqlite::types::Type::Integer,
                format!("invalid asset_vout {asset_vout}").into(),
            )
        })?,
        lbtc_vout: u32::try_from(lbtc_vout).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                14,
                rusqlite::types::Type::Integer,
                format!("invalid lbtc_vout {lbtc_vout}").into(),
            )
        })?,
        min_funding_confs: u32::try_from(min_funding_confs).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                15,
                rusqlite::types::Type::Integer,
                format!("invalid min_funding_confs {min_funding_confs}").into(),
            )
        })?,
        ln_payment_id: row.get(16)?,
        ln_preimage_hex: row.get(17)?,
        claim_txid: row.get(18)?,
        status,
    })
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS quotes (
  quote_id TEXT PRIMARY KEY,
  offer_id TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  asset_amount INTEGER NOT NULL,
  buyer_claim_address TEXT NOT NULL,
  min_funding_confs INTEGER NOT NULL,
  total_price_msat INTEGER NOT NULL,
  price_msat_per_asset_unit INTEGER NOT NULL,
  fee_subsidy_sats INTEGER NOT NULL,
  refund_delta_blocks INTEGER NOT NULL,
  invoice_expiry_secs INTEGER NOT NULL,
  max_min_funding_confs INTEGER NOT NULL,
  swap_id TEXT
);
CREATE INDEX IF NOT EXISTS quotes_swap_id_idx ON quotes(swap_id);

CREATE TABLE IF NOT EXISTS swaps (
  swap_id TEXT PRIMARY KEY,
  quote_id TEXT NOT NULL DEFAULT '',
  bolt11_invoice TEXT NOT NULL,
  payment_hash TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  asset_amount INTEGER NOT NULL,
  total_price_msat INTEGER NOT NULL DEFAULT 0,
  buyer_claim_address TEXT NOT NULL DEFAULT '',
  fee_subsidy_sats INTEGER NOT NULL,
  refund_lock_height INTEGER NOT NULL,
  p2wsh_address TEXT NOT NULL,
  witness_script_hex TEXT NOT NULL,
  funding_txid TEXT NOT NULL,
  asset_vout INTEGER NOT NULL,
  lbtc_vout INTEGER NOT NULL,
  min_funding_confs INTEGER NOT NULL,
  ln_payment_id TEXT,
  ln_preimage_hex TEXT,
  claim_txid TEXT,
  status TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS swaps_status_idx ON swaps(status);
"#,
    )
    .context("create tables")?;

    ensure_columns(conn).context("ensure columns")?;
    Ok(())
}

fn ensure_columns(conn: &Connection) -> Result<()> {
    let swaps_cols = table_columns(conn, "swaps").context("read swaps columns")?;
    ensure_column(
        conn,
        "swaps",
        &swaps_cols,
        "quote_id",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "swaps",
        &swaps_cols,
        "total_price_msat",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "swaps",
        &swaps_cols,
        "buyer_claim_address",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(conn, "swaps", &swaps_cols, "ln_payment_id", "TEXT")?;
    ensure_column(conn, "swaps", &swaps_cols, "ln_preimage_hex", "TEXT")?;
    ensure_column(conn, "swaps", &swaps_cols, "claim_txid", "TEXT")?;

    let quotes_cols = table_columns(conn, "quotes").context("read quotes columns")?;
    ensure_column(conn, "quotes", &quotes_cols, "swap_id", "TEXT")?;

    Ok(())
}

fn table_columns(conn: &Connection, table: &str) -> Result<HashSet<String>> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .with_context(|| format!("prepare PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([]).context("query PRAGMA table_info")?;

    let mut out = HashSet::new();
    while let Some(row) = rows.next().context("read PRAGMA row")? {
        let name: String = row.get(1)?;
        out.insert(name);
    }
    Ok(out)
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    columns: &HashSet<String>,
    name: &str,
    decl: &str,
) -> Result<()> {
    if columns.contains(name) {
        return Ok(());
    }
    conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {name} {decl}"), [])
        .with_context(|| format!("add column {table}.{name}"))?;
    Ok(())
}

fn status_to_str(status: SwapStatus) -> &'static str {
    match status {
        SwapStatus::Created => "created",
        SwapStatus::Funded => "funded",
        SwapStatus::Paid => "paid",
        SwapStatus::Claimed => "claimed",
        SwapStatus::Refunded => "refunded",
        SwapStatus::Failed => "failed",
    }
}

fn status_from_str(s: &str, col: usize) -> rusqlite::Result<SwapStatus> {
    match s {
        "created" => Ok(SwapStatus::Created),
        "funded" => Ok(SwapStatus::Funded),
        "paid" => Ok(SwapStatus::Paid),
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
