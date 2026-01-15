use std::str::FromStr as _;

use anyhow::{Context as _, Result};
use ln_liquid_swap::liquid::htlc::{
    HtlcFunding, HtlcSpec, claim_tx_from_witness_script, pubkey_hash160_from_p2wpkh_address,
    refund_tx_from_witness_script, sha256_preimage,
};
use lwk_wollet::elements::bitcoin::PublicKey;
use lwk_wollet::elements::bitcoin::secp256k1::{Secp256k1, SecretKey};
use lwk_wollet::elements::{Address, AddressParams, AssetId, LockTime, Txid};

#[test]
fn htlc_claim_and_refund_builds() -> Result<()> {
    let secp = Secp256k1::new();
    let claimer_secret_key = SecretKey::from_slice(&[1u8; 32]).context("claimer secret key")?;
    let refunder_secret_key = SecretKey::from_slice(&[2u8; 32]).context("refunder secret key")?;

    let claimer_pubkey = PublicKey::new(claimer_secret_key.public_key(&secp));
    let refunder_pubkey = PublicKey::new(refunder_secret_key.public_key(&secp));

    let claimer_address = Address::p2wpkh(&claimer_pubkey, None, &AddressParams::ELEMENTS);
    let refunder_address = Address::p2wpkh(&refunder_pubkey, None, &AddressParams::ELEMENTS);

    let payment_preimage = [9u8; 32];
    let payment_hash = sha256_preimage(&payment_preimage);

    let claimer_pubkey_hash160 =
        pubkey_hash160_from_p2wpkh_address(&claimer_address).context("claimer pubkey hash160")?;
    let refunder_pubkey_hash160 =
        pubkey_hash160_from_p2wpkh_address(&refunder_address).context("refunder pubkey hash160")?;

    let refund_lock_height = 1_000;
    let spec = HtlcSpec {
        payment_hash,
        claimer_pubkey_hash160,
        refunder_pubkey_hash160,
        refund_lock_height,
    };

    let funding = HtlcFunding {
        funding_txid: Txid::from_str(
            "0000000000000000000000000000000000000000000000000000000000000001",
        )
        .context("funding_txid")?,
        asset_vout: 0,
        lbtc_vout: 1,
        asset_id: AssetId::from_str(
            "0101010101010101010101010101010101010101010101010101010101010101",
        )
        .context("asset_id")?,
        asset_amount: 5_000,
        policy_asset: AssetId::from_str(
            "0202020202020202020202020202020202020202020202020202020202020202",
        )
        .context("policy_asset")?,
        fee_subsidy_sats: 2_000,
    };

    let witness_script = spec.witness_script();

    let claim_tx = claim_tx_from_witness_script(
        &witness_script,
        &funding,
        &claimer_address,
        &claimer_secret_key,
        payment_preimage,
        500,
    )
    .context("build claim tx")?;
    assert_eq!(claim_tx.lock_time, LockTime::ZERO);
    assert_eq!(
        claim_tx.output[0].script_pubkey,
        claimer_address.script_pubkey()
    );
    assert_eq!(
        claim_tx.output[1].script_pubkey,
        claimer_address.script_pubkey()
    );

    let refund_tx = refund_tx_from_witness_script(
        &witness_script,
        refund_lock_height,
        &funding,
        &refunder_address,
        &refunder_secret_key,
        500,
    )
    .context("build refund tx")?;
    assert_eq!(
        refund_tx.lock_time,
        LockTime::from_height(refund_lock_height).context("refund locktime")?
    );
    assert_eq!(
        refund_tx.output[0].script_pubkey,
        refunder_address.script_pubkey()
    );
    assert_eq!(
        refund_tx.output[1].script_pubkey,
        refunder_address.script_pubkey()
    );

    Ok(())
}
