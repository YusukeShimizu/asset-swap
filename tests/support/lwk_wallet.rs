use std::time::Duration;

use anyhow::{Context as _, Result};
use lwk_common::Signer as _;
use lwk_signer::SwSigner;
use lwk_test_util::{add_checksum, pset_rt};
use lwk_wollet::blocking::BlockchainBackend as _;
use lwk_wollet::{
    ElectrumClient, ElectrumUrl, ElementsNetwork, Wollet, WolletDescriptor,
    elements::{Address, AssetId, Txid},
    full_scan_with_electrum_client,
};
use tempfile::TempDir;

use super::lwk_env::LiquidRegtestEnv;

pub struct LwkWalletFixture {
    name: String,
    signer: SwSigner,
    wollet: Wollet,
    client: ElectrumClient,
    persist_dir: Option<TempDir>,
}

impl LwkWalletFixture {
    pub fn new(name: &str, mnemonic: &str, slip77_key: &str, electrum_url: &str) -> Result<Self> {
        let signer = SwSigner::new(mnemonic, false).context("create SwSigner")?;
        let xpub = signer.xpub();

        let desc_str = format!("ct(slip77({slip77_key}),elwpkh({xpub}/*))");
        let desc_str = add_checksum(&desc_str);
        let descriptor: WolletDescriptor = desc_str
            .parse()
            .with_context(|| format!("parse wollet descriptor for {name}"))?;

        let network = ElementsNetwork::default_regtest();
        let persist_dir = tempfile::tempdir().context("create wollet persist dir")?;
        let wollet = Wollet::with_fs_persist(network, descriptor, persist_dir.path())
            .context("create wollet")?;

        let client = electrum_client(electrum_url).context("create electrum client")?;

        let mut wallet = Self {
            name: name.to_string(),
            signer,
            wollet,
            client,
            persist_dir: Some(persist_dir),
        };

        wallet.sync().context("initial sync")?;
        Ok(wallet)
    }

    pub fn policy_asset(&self) -> AssetId {
        self.wollet.policy_asset()
    }

    pub fn address(&self) -> Result<Address> {
        self.wollet
            .address(None)
            .context("get wollet address")
            .map(|r| r.address().clone())
    }

    pub fn balance(&self, asset: &AssetId) -> Result<u64> {
        let balances = self.wollet.balance().context("get wollet balance")?;
        Ok(*balances.get(asset).unwrap_or(&0))
    }

    pub fn sync(&mut self) -> Result<()> {
        full_scan_with_electrum_client(&mut self.wollet, &mut self.client)
            .with_context(|| format!("sync wollet via electrum ({})", self.name))
    }

    pub fn fund_lbtc(&mut self, env: &LiquidRegtestEnv, satoshi: u64) -> Result<Txid> {
        let address = self.address().context("get funding address")?;
        let txid = env.elementsd_sendtoaddress(&address, satoshi, None);
        env.elementsd_generate(1);
        self.wait_for_tx(&txid, Duration::from_secs(60))
            .context("wait funding tx")?;
        Ok(txid)
    }

    pub fn issue_asset(
        &mut self,
        env: &LiquidRegtestEnv,
        asset_amount: u64,
        reissuance_token_amount: u64,
    ) -> Result<(Txid, AssetId, AssetId)> {
        let mut pset = self
            .wollet
            .tx_builder()
            .issue_asset(asset_amount, None, reissuance_token_amount, None, None)
            .context("build issuance pset")?
            .finish()
            .context("finalize issuance pset")?;

        pset = pset_rt(&pset);
        let (asset_id, token_id) = pset.inputs()[0].issuance_ids();

        let sigs = self.signer.sign(&mut pset).context("sign issuance pset")?;
        anyhow::ensure!(sigs > 0, "no signatures added for issuance");

        let tx = self
            .wollet
            .finalize(&mut pset)
            .context("finalize issuance tx")?;
        let txid = self
            .client
            .broadcast(&tx)
            .context("broadcast issuance tx")?;
        env.elementsd_generate(1);
        self.wait_for_tx(&txid, Duration::from_secs(60))
            .context("wait issuance tx")?;

        Ok((txid, asset_id, token_id))
    }

    pub fn send_asset(
        &mut self,
        env: &LiquidRegtestEnv,
        to: &Address,
        asset: &AssetId,
        satoshi: u64,
    ) -> Result<Txid> {
        let mut pset = self
            .wollet
            .tx_builder()
            .add_recipient(to, satoshi, *asset)
            .context("add asset recipient")?
            .finish()
            .context("finalize send pset")?;

        pset = pset_rt(&pset);
        let sigs = self.signer.sign(&mut pset).context("sign send pset")?;
        anyhow::ensure!(sigs > 0, "no signatures added for send");

        let tx = self
            .wollet
            .finalize(&mut pset)
            .context("finalize send tx")?;
        let txid = self.client.broadcast(&tx).context("broadcast send tx")?;
        env.elementsd_generate(1);
        self.wait_for_tx(&txid, Duration::from_secs(60))
            .context("wait send tx")?;

        Ok(txid)
    }

    fn wait_for_tx(&mut self, txid: &Txid, timeout: Duration) -> Result<()> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            self.sync().ok();

            if let Ok(txs) = self.wollet.transactions()
                && txs.iter().any(|e| &e.txid == txid)
            {
                return Ok(());
            }

            if std::time::Instant::now() >= deadline {
                anyhow::bail!("timeout waiting for tx {txid} in wollet {}", self.name);
            }

            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

impl Drop for LwkWalletFixture {
    fn drop(&mut self) {
        if (should_keep_artifacts() || std::thread::panicking())
            && let Some(dir) = self.persist_dir.take()
        {
            let _ = dir.keep();
        }
    }
}

fn electrum_client(url: &str) -> Result<ElectrumClient> {
    let endpoint = url.trim_start_matches("tcp://");
    let electrum_url = ElectrumUrl::new(endpoint, false, false)
        .with_context(|| format!("parse electrum url {endpoint}"))?;
    ElectrumClient::new(&electrum_url).context("create electrum client")
}

fn should_keep_artifacts() -> bool {
    matches!(
        std::env::var("KEEP_LWK_E2E_ARTIFACTS")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes"
    )
}
