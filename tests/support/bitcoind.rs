use std::fs::File;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use bitcoincore_rpc::bitcoin::address::{NetworkChecked, NetworkUnchecked};
use bitcoincore_rpc::bitcoin::{Address, Amount, Network, Txid};
use bitcoincore_rpc::{Auth, Client, RpcApi as _};
use tempfile::TempDir;

use super::port::get_available_port;

pub struct BitcoindProcess {
    temp_dir: Option<TempDir>,
    rpc_port: u16,
    rpc_user: String,
    rpc_password: String,
    child: Child,
    log_path: PathBuf,
}

impl BitcoindProcess {
    pub fn start() -> anyhow::Result<Self> {
        let temp_dir = tempfile::tempdir().context("create bitcoind tempdir")?;
        let root_dir = temp_dir.path().to_path_buf();

        let rpc_port = get_available_port().context("select bitcoind rpc port")?;
        let p2p_port = get_available_port().context("select bitcoind p2p port")?;
        let rpc_user = "rpcuser".to_string();
        let rpc_password = "rpcpassword".to_string();

        let conf_path = root_dir.join("bitcoin.conf");
        let mut conf = File::create(&conf_path).context("create bitcoin.conf")?;
        writeln!(conf, "regtest=1")?;
        writeln!(conf, "server=1")?;
        writeln!(conf, "txindex=1")?;
        writeln!(conf, "rpcuser={rpc_user}")?;
        writeln!(conf, "rpcpassword={rpc_password}")?;
        writeln!(conf, "[regtest]")?;
        writeln!(conf, "fallbackfee=0.0001")?;
        writeln!(conf, "rpcbind=127.0.0.1")?;
        writeln!(conf, "rpcallowip=127.0.0.1")?;
        writeln!(conf, "rpcport={rpc_port}")?;
        writeln!(conf, "port={p2p_port}")?;

        let log_path = root_dir.join("bitcoind.stdout.log");
        let log_file = File::create(&log_path).context("create bitcoind log file")?;
        let log_file_err = log_file.try_clone().context("clone bitcoind log file")?;

        let mut child = Command::new("bitcoind")
            .arg(format!("-datadir={}", root_dir.display()))
            .arg(format!("-conf={}", conf_path.display()))
            .arg("-printtoconsole=1")
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_file_err))
            .spawn()
            .context("spawn bitcoind")?;

        let rpc_url = format!("http://127.0.0.1:{rpc_port}");
        let auth = Auth::UserPass(rpc_user.clone(), rpc_password.clone());
        let client = Client::new(&rpc_url, auth).context("create bitcoind rpc client")?;

        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                maybe_persist_tempdir(temp_dir);
                anyhow::bail!(
                    "bitcoind rpc did not become ready (log: {})",
                    log_path.display()
                );
            }

            if let Some(status) = child.try_wait().context("poll bitcoind process status")? {
                maybe_persist_tempdir(temp_dir);
                anyhow::bail!(
                    "bitcoind exited early with status {status} (log: {})",
                    log_path.display()
                );
            }

            match client.get_blockchain_info() {
                Ok(_) => break,
                Err(_) => std::thread::sleep(Duration::from_millis(200)),
            }
        }

        Ok(Self {
            temp_dir: Some(temp_dir),
            rpc_port,
            rpc_user,
            rpc_password,
            child,
            log_path,
        })
    }

    pub fn rpc_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.rpc_port)
    }

    pub fn auth(&self) -> Auth {
        Auth::UserPass(self.rpc_user.clone(), self.rpc_password.clone())
    }

    pub fn rpc_user(&self) -> &str {
        &self.rpc_user
    }

    pub fn rpc_password(&self) -> &str {
        &self.rpc_password
    }

    pub fn client(&self) -> anyhow::Result<Client> {
        Client::new(&self.rpc_url(), self.auth()).context("create bitcoind client")
    }

    pub fn wallet_client(&self, wallet_name: &str) -> anyhow::Result<Client> {
        let url = format!("{}/wallet/{wallet_name}", self.rpc_url());
        Client::new(&url, self.auth()).context("create bitcoind wallet client")
    }

    pub fn create_wallet_if_missing(&self, wallet_name: &str) -> anyhow::Result<()> {
        let client = self.client()?;
        let wallets = client.list_wallets().context("list wallets")?;
        if wallets.iter().any(|w| w == wallet_name) {
            return Ok(());
        }

        client
            .create_wallet(wallet_name, None, None, None, None)
            .with_context(|| format!("create wallet {wallet_name}"))?;
        Ok(())
    }

    pub fn mine_blocks(&self, wallet_name: &str, blocks: u64) -> anyhow::Result<()> {
        self.create_wallet_if_missing(wallet_name)?;

        let wallet = self.wallet_client(wallet_name)?;
        let mining_address_unchecked: Address<NetworkUnchecked> = wallet
            .get_new_address(None, None)
            .context("get mining address")?;
        let mining_address: Address<NetworkChecked> = mining_address_unchecked
            .require_network(Network::Regtest)
            .context("check mining address network")?;

        let client = self.client()?;
        client
            .generate_to_address(blocks, &mining_address)
            .context("generate blocks")?;

        Ok(())
    }

    pub fn send_to_address(
        &self,
        wallet_name: &str,
        address: &Address<NetworkChecked>,
        amount_sats: u64,
    ) -> anyhow::Result<Txid> {
        self.create_wallet_if_missing(wallet_name)?;
        let wallet = self.wallet_client(wallet_name)?;
        wallet
            .send_to_address(
                address,
                Amount::from_sat(amount_sats),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .context("send_to_address")
    }

    pub fn log_path(&self) -> &PathBuf {
        &self.log_path
    }
}

impl Drop for BitcoindProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();

        if (should_keep_artifacts() || std::thread::panicking())
            && let Some(temp_dir) = self.temp_dir.take()
        {
            let _ = temp_dir.keep();
        }
    }
}

fn should_keep_artifacts() -> bool {
    matches!(
        std::env::var("KEEP_LDK_E2E_ARTIFACTS")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes"
    )
}

fn maybe_persist_tempdir(temp_dir: TempDir) {
    if should_keep_artifacts() {
        let _ = temp_dir.keep();
    }
}
