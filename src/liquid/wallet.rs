use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use lwk_common::Signer as _;
use lwk_signer::SwSigner;
use lwk_wollet::blocking::BlockchainBackend as _;
use lwk_wollet::{
    ElectrumClient, ElectrumUrl, ElementsNetwork, History, Wollet, WolletDescriptor,
    elements::{Address, AssetId, Script, Transaction, Txid, confidential},
    full_scan_with_electrum_client,
};

pub struct LiquidWallet {
    signer: SwSigner,
    wollet: Wollet,
    client: ElectrumClient,
    network: ElementsNetwork,
}

impl LiquidWallet {
    pub fn new(
        mnemonic: &str,
        slip77_key: &str,
        electrum_url: &str,
        persist_dir: &Path,
        network: ElementsNetwork,
    ) -> Result<Self> {
        let signer = SwSigner::new(mnemonic, false).context("create SwSigner")?;
        let xpub = signer.xpub();

        let desc_str = format!("ct(slip77({slip77_key}),elwpkh({xpub}/*))");
        let descriptor: WolletDescriptor = desc_str.parse().context("parse wollet descriptor")?;

        let wollet =
            Wollet::with_fs_persist(network, descriptor, persist_dir).context("create wollet")?;

        let client = electrum_client(electrum_url).context("create electrum client")?;

        let mut wallet = Self {
            signer,
            wollet,
            client,
            network,
        };
        wallet.sync().context("initial sync")?;
        Ok(wallet)
    }

    pub fn network(&self) -> ElementsNetwork {
        self.network
    }

    pub fn policy_asset(&self) -> AssetId {
        self.wollet.policy_asset()
    }

    pub fn balance(&self, asset: &AssetId) -> Result<u64> {
        let balances = self.wollet.balance().context("get wollet balance")?;
        Ok(*balances.get(asset).unwrap_or(&0))
    }

    pub fn tip_height(&self) -> u32 {
        self.wollet.tip().height()
    }

    pub fn address_at(&self, index: u32) -> Result<Address> {
        Ok(self
            .wollet
            .address(Some(index))
            .context("get wollet address")?
            .address()
            .clone())
    }

    pub fn sync(&mut self) -> Result<()> {
        full_scan_with_electrum_client(&mut self.wollet, &mut self.client)
            .context("sync wollet via electrum")
    }

    pub fn build_and_broadcast_funding(
        &mut self,
        htlc_address: &Address,
        asset_id: AssetId,
        asset_amount: u64,
        fee_subsidy_sats: u64,
    ) -> Result<(Transaction, Txid, u32, u32)> {
        self.sync()
            .context("sync wallet before building funding tx")?;

        let policy_asset = self.policy_asset();

        let mut pset = self
            .wollet
            .tx_builder()
            .add_explicit_recipient(htlc_address, asset_amount, asset_id)
            .context("add htlc asset output")?
            .add_explicit_recipient(htlc_address, fee_subsidy_sats, policy_asset)
            .context("add htlc lbtc subsidy output")?
            .finish()
            .context("finalize funding pset")?;

        let sigs = self.signer.sign(&mut pset).context("sign funding pset")?;
        anyhow::ensure!(sigs > 0, "no signatures added for funding");

        let tx = self
            .wollet
            .finalize(&mut pset)
            .context("finalize funding tx")?;
        let txid = self.client.broadcast(&tx).context("broadcast funding tx")?;

        let mut asset_vout: Option<u32> = None;
        let mut lbtc_vout: Option<u32> = None;

        let htlc_spk = htlc_address.script_pubkey();
        for (vout, output) in tx.output.iter().enumerate() {
            if output.script_pubkey != htlc_spk {
                continue;
            }

            match output.asset {
                confidential::Asset::Explicit(a) if a == asset_id => {
                    asset_vout = Some(vout as u32);
                }
                confidential::Asset::Explicit(a) if a == policy_asset => {
                    lbtc_vout = Some(vout as u32);
                }
                _ => {}
            }
        }

        let asset_vout = asset_vout.context("asset htlc output not found")?;
        let lbtc_vout = lbtc_vout.context("lbtc htlc output not found")?;

        Ok((tx, txid, asset_vout, lbtc_vout))
    }

    pub fn tx_confirmations_for_script(
        &self,
        script_pubkey: &Script,
        txid: &Txid,
    ) -> Result<Option<u32>> {
        let mut histories = self
            .client
            .get_scripts_history(&[script_pubkey])
            .context("get script history")?;
        let history: Vec<History> = histories.pop().unwrap_or_default();
        let Some(entry) = history.into_iter().find(|h| &h.txid == txid) else {
            return Ok(None);
        };

        if entry.height <= 0 {
            return Ok(Some(0));
        }

        let height = u32::try_from(entry.height).context("history height must be positive")?;
        let tip = self.tip_height();
        if tip < height {
            return Ok(Some(0));
        }
        Ok(Some(tip - height + 1))
    }

    pub fn wait_for_tx_confirmations_for_script(
        &mut self,
        script_pubkey: &Script,
        txid: &Txid,
        min_confs: u32,
        timeout: Duration,
    ) -> Result<u32> {
        let deadline = Instant::now() + timeout;
        loop {
            self.sync().context("sync wallet")?;
            if let Some(confs) = self
                .tx_confirmations_for_script(script_pubkey, txid)
                .context("get tx confirmations")?
                && confs >= min_confs
            {
                return Ok(confs);
            }

            if Instant::now() >= deadline {
                anyhow::bail!(
                    "timeout waiting for confirmations: txid={txid} min_confs={min_confs}"
                );
            }

            std::thread::sleep(Duration::from_millis(500));
        }
    }

    pub fn broadcast_transaction(&self, tx: &Transaction) -> Result<Txid> {
        self.client.broadcast(tx).context("broadcast tx")
    }

    pub fn get_transaction(&self, txid: &Txid) -> Result<Transaction> {
        let mut txs = self
            .client
            .get_transactions(&[*txid])
            .context("get transaction")?;
        let tx = txs.pop().context("transaction not found")?;
        Ok(tx)
    }

    pub fn signer(&self) -> &SwSigner {
        &self.signer
    }
}

fn electrum_client(url: &str) -> Result<ElectrumClient> {
    let endpoint = url.trim_start_matches("tcp://");
    let electrum_url = ElectrumUrl::new(endpoint, false, false)
        .with_context(|| format!("parse electrum url {endpoint}"))?;
    ElectrumClient::new(&electrum_url).context("create electrum client")
}
