use anyhow::{Context as _, Result};
use lwk_wollet::elements::bitcoin::hashes::{Hash as _, hash160, sha256};
use lwk_wollet::elements::bitcoin::secp256k1::Message as BitcoinMessage;
use lwk_wollet::elements::bitcoin::secp256k1::PublicKey as BitcoinPublicKey;
use lwk_wollet::elements::bitcoin::secp256k1::Secp256k1 as BitcoinSecp256k1;
use lwk_wollet::elements::bitcoin::secp256k1::SecretKey as BitcoinSecretKey;
use lwk_wollet::elements::bitcoin::secp256k1::ecdsa::Signature as BitcoinEcdsaSignature;
use lwk_wollet::elements::confidential::{Asset, Nonce, Value};
use lwk_wollet::elements::opcodes;
use lwk_wollet::elements::script::{Builder, Script};
use lwk_wollet::elements::sighash::SighashCache;
use lwk_wollet::elements::{
    Address, AddressParams, AssetId, EcdsaSighashType, LockTime, OutPoint, Sequence, Transaction,
    TxIn, TxInWitness, TxOut, TxOutWitness, Txid,
};

#[derive(Debug, Clone)]
pub struct HtlcSpec {
    pub payment_hash: [u8; 32],
    pub buyer_pubkey_hash160: [u8; 20],
    pub seller_pubkey_hash160: [u8; 20],
    pub refund_lock_height: u32,
}

impl HtlcSpec {
    pub fn witness_script(&self) -> Script {
        Builder::new()
            .push_opcode(opcodes::all::OP_IF)
            .push_opcode(opcodes::all::OP_SIZE)
            .push_int(32)
            .push_opcode(opcodes::all::OP_EQUALVERIFY)
            .push_opcode(opcodes::all::OP_SHA256)
            .push_slice(&self.payment_hash)
            .push_opcode(opcodes::all::OP_EQUALVERIFY)
            .push_opcode(opcodes::all::OP_DUP)
            .push_opcode(opcodes::all::OP_HASH160)
            .push_slice(&self.buyer_pubkey_hash160)
            .push_opcode(opcodes::all::OP_EQUALVERIFY)
            .push_opcode(opcodes::all::OP_CHECKSIG)
            .push_opcode(opcodes::all::OP_ELSE)
            .push_int(self.refund_lock_height as i64)
            .push_opcode(opcodes::all::OP_CLTV)
            .push_opcode(opcodes::all::OP_DROP)
            .push_opcode(opcodes::all::OP_DUP)
            .push_opcode(opcodes::all::OP_HASH160)
            .push_slice(&self.seller_pubkey_hash160)
            .push_opcode(opcodes::all::OP_EQUALVERIFY)
            .push_opcode(opcodes::all::OP_CHECKSIG)
            .push_opcode(opcodes::all::OP_ENDIF)
            .into_script()
    }

    pub fn p2wsh_address(&self, params: &'static AddressParams) -> Address {
        Address::p2wsh(&self.witness_script(), None, params)
    }

    pub fn parse_witness_script(witness_script: &Script) -> Result<Self> {
        use lwk_wollet::elements::script::Instruction;

        fn next_instruction<'a>(
            iter: &mut impl Iterator<
                Item = std::result::Result<Instruction<'a>, lwk_wollet::elements::script::Error>,
            >,
        ) -> Result<Instruction<'a>> {
            iter.next()
                .transpose()
                .map_err(|e| anyhow::anyhow!("decode witness script instruction: {e:?}"))?
                .context("unexpected end of witness script")
        }

        fn expect_op(actual: Instruction<'_>, expected: opcodes::All) -> Result<()> {
            match actual {
                Instruction::Op(op) if op == expected => Ok(()),
                other => {
                    anyhow::bail!("unexpected instruction: expected {expected:?}, got {other:?}")
                }
            }
        }

        fn parse_script_num(actual: Instruction<'_>) -> Result<i64> {
            match actual {
                Instruction::PushBytes(bytes) => decode_script_num(bytes),
                Instruction::Op(op) => {
                    let code = op.into_u8();
                    if code == opcodes::all::OP_PUSHNUM_NEG1.into_u8() {
                        return Ok(-1);
                    }

                    let one = opcodes::all::OP_PUSHNUM_1.into_u8();
                    let sixteen = opcodes::all::OP_PUSHNUM_16.into_u8();
                    if (one..=sixteen).contains(&code) {
                        return Ok((code - one + 1) as i64);
                    }

                    anyhow::bail!("unexpected opcode where script number expected: {op:?}");
                }
            }
        }

        fn decode_script_num(bytes: &[u8]) -> Result<i64> {
            if bytes.is_empty() {
                return Ok(0);
            }
            if bytes.len() > 8 {
                anyhow::bail!("script number too large: {} bytes", bytes.len());
            }

            let mut magnitude = bytes.to_vec();
            let negative = magnitude.last().is_some_and(|b| (b & 0x80) != 0);
            if let Some(last) = magnitude.last_mut() {
                *last &= 0x7f;
            }

            let mut value: i64 = 0;
            for (i, b) in magnitude.iter().enumerate() {
                value |= (*b as i64) << (8 * i);
            }

            Ok(if negative { -value } else { value })
        }

        fn expect_push<const N: usize>(actual: Instruction<'_>) -> Result<[u8; N]> {
            match actual {
                Instruction::PushBytes(bytes) if bytes.len() == N => {
                    let mut out = [0u8; N];
                    out.copy_from_slice(bytes);
                    Ok(out)
                }
                other => {
                    anyhow::bail!("unexpected instruction: expected push {N} bytes, got {other:?}")
                }
            }
        }

        let mut iter = witness_script.instructions_minimal();

        expect_op(next_instruction(&mut iter)?, opcodes::all::OP_IF)?;
        expect_op(next_instruction(&mut iter)?, opcodes::all::OP_SIZE)?;
        let size = parse_script_num(next_instruction(&mut iter)?)?;
        anyhow::ensure!(size == 32, "unexpected preimage size check: {size}");
        expect_op(next_instruction(&mut iter)?, opcodes::all::OP_EQUALVERIFY)?;

        expect_op(next_instruction(&mut iter)?, opcodes::all::OP_SHA256)?;
        let payment_hash = expect_push::<32>(next_instruction(&mut iter)?)?;
        expect_op(next_instruction(&mut iter)?, opcodes::all::OP_EQUALVERIFY)?;

        expect_op(next_instruction(&mut iter)?, opcodes::all::OP_DUP)?;
        expect_op(next_instruction(&mut iter)?, opcodes::all::OP_HASH160)?;
        let buyer_pubkey_hash160 = expect_push::<20>(next_instruction(&mut iter)?)?;
        expect_op(next_instruction(&mut iter)?, opcodes::all::OP_EQUALVERIFY)?;
        expect_op(next_instruction(&mut iter)?, opcodes::all::OP_CHECKSIG)?;

        expect_op(next_instruction(&mut iter)?, opcodes::all::OP_ELSE)?;
        let refund_lock_height = parse_script_num(next_instruction(&mut iter)?)?;
        anyhow::ensure!(
            refund_lock_height >= 0 && refund_lock_height <= u32::MAX as i64,
            "refund_lock_height out of range: {refund_lock_height}"
        );
        let refund_lock_height = refund_lock_height as u32;
        expect_op(next_instruction(&mut iter)?, opcodes::all::OP_CLTV)?;
        expect_op(next_instruction(&mut iter)?, opcodes::all::OP_DROP)?;

        expect_op(next_instruction(&mut iter)?, opcodes::all::OP_DUP)?;
        expect_op(next_instruction(&mut iter)?, opcodes::all::OP_HASH160)?;
        let seller_pubkey_hash160 = expect_push::<20>(next_instruction(&mut iter)?)?;
        expect_op(next_instruction(&mut iter)?, opcodes::all::OP_EQUALVERIFY)?;
        expect_op(next_instruction(&mut iter)?, opcodes::all::OP_CHECKSIG)?;
        expect_op(next_instruction(&mut iter)?, opcodes::all::OP_ENDIF)?;

        anyhow::ensure!(
            iter.next().is_none(),
            "unexpected trailing instructions in witness script"
        );

        Ok(Self {
            payment_hash,
            buyer_pubkey_hash160,
            seller_pubkey_hash160,
            refund_lock_height,
        })
    }
}

#[derive(Debug, Clone)]
pub struct HtlcFunding {
    pub funding_txid: Txid,
    pub asset_vout: u32,
    pub lbtc_vout: u32,
    pub asset_id: AssetId,
    pub asset_amount: u64,
    pub policy_asset: AssetId,
    pub fee_subsidy_sats: u64,
}

pub fn pubkey_hash160_from_p2wpkh_address(address: &Address) -> Result<[u8; 20]> {
    pubkey_hash160_from_p2wpkh_script(&address.script_pubkey())
}

pub fn pubkey_hash160_from_p2wpkh_script(script_pubkey: &Script) -> Result<[u8; 20]> {
    let bytes = script_pubkey.as_bytes();
    if bytes.len() != 22 || bytes[0] != 0x00 || bytes[1] != 0x14 {
        anyhow::bail!("expected P2WPKH script_pubkey (0x0014..), got {script_pubkey:?}");
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&bytes[2..22]);
    Ok(out)
}

pub fn claim_tx(
    spec: &HtlcSpec,
    funding: &HtlcFunding,
    buyer_receive: &Address,
    buyer_secret_key: &BitcoinSecretKey,
    preimage: [u8; 32],
    fee_sats: u64,
) -> Result<Transaction> {
    let witness_script = spec.witness_script();
    claim_tx_from_witness_script(
        &witness_script,
        funding,
        buyer_receive,
        buyer_secret_key,
        preimage,
        fee_sats,
    )
}

pub fn refund_tx(
    spec: &HtlcSpec,
    funding: &HtlcFunding,
    seller_receive: &Address,
    seller_secret_key: &BitcoinSecretKey,
    fee_sats: u64,
) -> Result<Transaction> {
    let witness_script = spec.witness_script();
    refund_tx_from_witness_script(
        &witness_script,
        spec.refund_lock_height,
        funding,
        seller_receive,
        seller_secret_key,
        fee_sats,
    )
}

pub fn claim_tx_from_witness_script(
    witness_script: &Script,
    funding: &HtlcFunding,
    buyer_receive: &Address,
    buyer_secret_key: &BitcoinSecretKey,
    preimage: [u8; 32],
    fee_sats: u64,
) -> Result<Transaction> {
    anyhow::ensure!(
        fee_sats < funding.fee_subsidy_sats,
        "fee_sats must be less than fee_subsidy_sats"
    );

    let inputs = vec![
        TxIn {
            previous_output: OutPoint {
                txid: funding.funding_txid,
                vout: funding.asset_vout,
            },
            is_pegin: false,
            script_sig: Script::new(),
            sequence: Sequence::MAX,
            asset_issuance: Default::default(),
            witness: TxInWitness::default(),
        },
        TxIn {
            previous_output: OutPoint {
                txid: funding.funding_txid,
                vout: funding.lbtc_vout,
            },
            is_pegin: false,
            script_sig: Script::new(),
            sequence: Sequence::MAX,
            asset_issuance: Default::default(),
            witness: TxInWitness::default(),
        },
    ];

    let buyer_spk = buyer_receive.script_pubkey();

    let outputs = vec![
        TxOut {
            asset: Asset::Explicit(funding.asset_id),
            value: Value::Explicit(funding.asset_amount),
            nonce: Nonce::Null,
            script_pubkey: buyer_spk.clone(),
            witness: TxOutWitness::default(),
        },
        TxOut {
            asset: Asset::Explicit(funding.policy_asset),
            value: Value::Explicit(funding.fee_subsidy_sats - fee_sats),
            nonce: Nonce::Null,
            script_pubkey: buyer_spk,
            witness: TxOutWitness::default(),
        },
        TxOut::new_fee(fee_sats, funding.policy_asset),
    ];

    let mut tx = Transaction {
        version: 2,
        lock_time: LockTime::ZERO,
        input: inputs,
        output: outputs,
    };

    let secp = BitcoinSecp256k1::new();
    let sighash_type = EcdsaSighashType::All;

    let mut cache = SighashCache::new(&tx);
    let asset_sig = segwit_v0_sign(
        &secp,
        &mut cache,
        0,
        witness_script,
        funding.asset_amount,
        buyer_secret_key,
        sighash_type,
    )
    .context("sign asset input")?;
    let lbtc_sig = segwit_v0_sign(
        &secp,
        &mut cache,
        1,
        witness_script,
        funding.fee_subsidy_sats,
        buyer_secret_key,
        sighash_type,
    )
    .context("sign lbtc input")?;

    let buyer_pubkey = BitcoinPublicKey::from_secret_key(&secp, buyer_secret_key).serialize();

    tx.input[0].witness.script_witness = vec![
        asset_sig,
        buyer_pubkey.to_vec(),
        preimage.to_vec(),
        vec![1u8],
        witness_script.to_bytes(),
    ];
    tx.input[1].witness.script_witness = vec![
        lbtc_sig,
        buyer_pubkey.to_vec(),
        preimage.to_vec(),
        vec![1u8],
        witness_script.to_bytes(),
    ];

    Ok(tx)
}

pub fn refund_tx_from_witness_script(
    witness_script: &Script,
    refund_lock_height: u32,
    funding: &HtlcFunding,
    seller_receive: &Address,
    seller_secret_key: &BitcoinSecretKey,
    fee_sats: u64,
) -> Result<Transaction> {
    anyhow::ensure!(
        fee_sats < funding.fee_subsidy_sats,
        "fee_sats must be less than fee_subsidy_sats"
    );

    let inputs = vec![
        TxIn {
            previous_output: OutPoint {
                txid: funding.funding_txid,
                vout: funding.asset_vout,
            },
            is_pegin: false,
            script_sig: Script::new(),
            sequence: Sequence::ENABLE_LOCKTIME_NO_RBF,
            asset_issuance: Default::default(),
            witness: TxInWitness::default(),
        },
        TxIn {
            previous_output: OutPoint {
                txid: funding.funding_txid,
                vout: funding.lbtc_vout,
            },
            is_pegin: false,
            script_sig: Script::new(),
            sequence: Sequence::ENABLE_LOCKTIME_NO_RBF,
            asset_issuance: Default::default(),
            witness: TxInWitness::default(),
        },
    ];

    let seller_spk = seller_receive.script_pubkey();

    let outputs = vec![
        TxOut {
            asset: Asset::Explicit(funding.asset_id),
            value: Value::Explicit(funding.asset_amount),
            nonce: Nonce::Null,
            script_pubkey: seller_spk.clone(),
            witness: TxOutWitness::default(),
        },
        TxOut {
            asset: Asset::Explicit(funding.policy_asset),
            value: Value::Explicit(funding.fee_subsidy_sats - fee_sats),
            nonce: Nonce::Null,
            script_pubkey: seller_spk,
            witness: TxOutWitness::default(),
        },
        TxOut::new_fee(fee_sats, funding.policy_asset),
    ];

    let mut tx = Transaction {
        version: 2,
        lock_time: LockTime::from_height(refund_lock_height)
            .context("refund_lock_height is invalid locktime")?,
        input: inputs,
        output: outputs,
    };

    let secp = BitcoinSecp256k1::new();
    let sighash_type = EcdsaSighashType::All;

    let mut cache = SighashCache::new(&tx);
    let asset_sig = segwit_v0_sign(
        &secp,
        &mut cache,
        0,
        witness_script,
        funding.asset_amount,
        seller_secret_key,
        sighash_type,
    )
    .context("sign asset input")?;
    let lbtc_sig = segwit_v0_sign(
        &secp,
        &mut cache,
        1,
        witness_script,
        funding.fee_subsidy_sats,
        seller_secret_key,
        sighash_type,
    )
    .context("sign lbtc input")?;

    let seller_pubkey = BitcoinPublicKey::from_secret_key(&secp, seller_secret_key).serialize();

    tx.input[0].witness.script_witness = vec![
        asset_sig,
        seller_pubkey.to_vec(),
        vec![],
        witness_script.to_bytes(),
    ];
    tx.input[1].witness.script_witness = vec![
        lbtc_sig,
        seller_pubkey.to_vec(),
        vec![],
        witness_script.to_bytes(),
    ];

    Ok(tx)
}

fn segwit_v0_sign(
    secp: &BitcoinSecp256k1<lwk_wollet::elements::bitcoin::secp256k1::All>,
    cache: &mut SighashCache<&Transaction>,
    input_index: usize,
    script_code: &Script,
    value: u64,
    secret_key: &BitcoinSecretKey,
    sighash_type: EcdsaSighashType,
) -> Result<Vec<u8>> {
    let sighash = cache.segwitv0_sighash(
        input_index,
        script_code,
        Value::Explicit(value),
        sighash_type,
    );

    let msg = BitcoinMessage::from_digest_slice(&sighash.to_byte_array())
        .context("create sighash message")?;
    let sig: BitcoinEcdsaSignature = secp.sign_ecdsa(&msg, secret_key);
    let mut sig_bytes = sig.serialize_der().to_vec();
    sig_bytes.push(sighash_type.as_u32() as u8);
    Ok(sig_bytes)
}

pub fn sha256_preimage(preimage: &[u8; 32]) -> [u8; 32] {
    sha256::Hash::hash(preimage).to_byte_array()
}

pub fn pubkey_hash160(pubkey_bytes: &[u8]) -> [u8; 20] {
    hash160::Hash::hash(pubkey_bytes).to_byte_array()
}
