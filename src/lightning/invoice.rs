use anyhow::Result;
use bitcoin::hashes::Hash as _;
use lightning_invoice::Bolt11Invoice;
use std::str::FromStr as _;

pub fn payment_hash_from_bolt11(invoice: &str) -> Result<[u8; 32]> {
    let invoice = Bolt11Invoice::from_str(invoice)
        .map_err(|e| anyhow::anyhow!("parse BOLT11 invoice: {e:?}"))?;
    Ok(invoice.payment_hash().to_byte_array())
}

pub fn amount_msat_from_bolt11(invoice: &str) -> Result<Option<u64>> {
    let invoice = Bolt11Invoice::from_str(invoice)
        .map_err(|e| anyhow::anyhow!("parse BOLT11 invoice: {e:?}"))?;
    Ok(invoice.amount_milli_satoshis())
}
