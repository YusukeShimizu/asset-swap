use anyhow::{Context as _, Result};
use lwk_signer::SwSigner;
use lwk_wollet::elements::bitcoin::bip32::{ChildNumber, DerivationPath};
use lwk_wollet::elements::bitcoin::secp256k1::SecretKey as BitcoinSecretKey;

pub fn derive_secret_key(signer: &SwSigner, index: u32) -> Result<BitcoinSecretKey> {
    let child = ChildNumber::from_normal_idx(index).context("invalid derivation index")?;
    let path = DerivationPath::from(vec![child]);
    let xprv = signer.derive_xprv(&path).context("derive xprv")?;
    Ok(xprv.private_key)
}
