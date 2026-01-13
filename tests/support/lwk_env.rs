use anyhow::{Context as _, Result};
use lwk_test_util::{TestEnv, TestEnvBuilder};
use lwk_wollet::elements::{Address, AssetId, Txid};

pub struct LiquidRegtestEnv {
    inner: TestEnv,
}

impl LiquidRegtestEnv {
    pub fn start() -> Result<Self> {
        require_env_var("ELEMENTSD_EXEC")?;
        require_env_var("ELECTRS_LIQUID_EXEC")?;

        let inner = TestEnvBuilder::from_env().with_electrum().build();
        Ok(Self { inner })
    }

    pub fn electrum_url(&self) -> String {
        self.inner.electrum_url()
    }

    pub fn elementsd_generate(&self, blocks: u32) {
        self.inner.elementsd_generate(blocks);
    }

    pub fn elementsd_sendtoaddress(
        &self,
        address: &Address,
        satoshi: u64,
        asset: Option<AssetId>,
    ) -> Txid {
        self.inner.elementsd_sendtoaddress(address, satoshi, asset)
    }
}

fn require_env_var(name: &str) -> Result<()> {
    let value = std::env::var(name)
        .with_context(|| format!("required env var {name} is not set (run via `nix develop`)"))?;
    anyhow::ensure!(
        !value.trim().is_empty(),
        "required env var {name} is empty (run via `nix develop`)"
    );
    Ok(())
}
