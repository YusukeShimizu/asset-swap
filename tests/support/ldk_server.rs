use std::fs::File;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use ldk_server_client::client::LdkServerClient;
use ldk_server_protos::api::GetNodeInfoRequest;
use tempfile::TempDir;

use super::bitcoind::BitcoindProcess;
use super::port::get_available_port;

pub struct LdkServerProcess {
    temp_dir: Option<TempDir>,
    name: String,
    listen_addr: String,
    child: Child,
    client: LdkServerClient,
    stdout_log_path: PathBuf,
    file_log_path: PathBuf,
}

impl LdkServerProcess {
    pub fn start(name: &str, bitcoind: &BitcoindProcess) -> anyhow::Result<Self> {
        let temp_dir = tempfile::tempdir().context("create ldk-server tempdir")?;
        let root_dir = temp_dir.path().to_path_buf();

        let listen_port = get_available_port().context("select ldk-server listen port")?;
        let rest_port = get_available_port().context("select ldk-server rest port")?;

        let listen_addr = format!("127.0.0.1:{listen_port}");
        let rest_service_address = format!("127.0.0.1:{rest_port}");

        let storage_dir = root_dir.join("data");
        std::fs::create_dir_all(&storage_dir).context("create ldk-server storage dir")?;

        let file_log_path = root_dir.join("ldk-server.log");

        let config_path = root_dir.join("ldk-server-config.toml");
        let mut config_file = File::create(&config_path).context("create ldk-server config")?;

        writeln!(config_file, "[node]")?;
        writeln!(config_file, "network = \"regtest\"")?;
        writeln!(config_file, "listening_address = \"{listen_addr}\"")?;
        writeln!(
            config_file,
            "rest_service_address = \"{rest_service_address}\""
        )?;
        writeln!(config_file)?;

        writeln!(config_file, "[storage.disk]")?;
        writeln!(config_file, "dir_path = \"{}\"", storage_dir.display())?;
        writeln!(config_file)?;

        writeln!(config_file, "[log]")?;
        writeln!(config_file, "level = \"Debug\"")?;
        writeln!(config_file, "file_path = \"{}\"", file_log_path.display())?;
        writeln!(config_file)?;

        let bitcoind_rpc = bitcoind.rpc_url().trim_start_matches("http://").to_string();
        writeln!(config_file, "[bitcoind]")?;
        writeln!(config_file, "rpc_address = \"{bitcoind_rpc}\"")?;
        writeln!(config_file, "rpc_user = \"{}\"", bitcoind.rpc_user())?;
        writeln!(
            config_file,
            "rpc_password = \"{}\"",
            bitcoind.rpc_password()
        )?;

        let stdout_log_path = root_dir.join("ldk-server.stdout.log");
        let stdout_log = File::create(&stdout_log_path).context("create ldk-server stdout log")?;
        let stdout_log_err = stdout_log
            .try_clone()
            .context("clone ldk-server stdout log")?;

        let child = Command::new("ldk-server")
            .arg(config_path)
            .stdout(Stdio::from(stdout_log))
            .stderr(Stdio::from(stdout_log_err))
            .spawn()
            .with_context(|| format!("spawn ldk-server ({name})"))?;

        let client = LdkServerClient::new(rest_service_address);

        Ok(Self {
            temp_dir: Some(temp_dir),
            name: name.to_string(),
            listen_addr,
            child,
            client,
            stdout_log_path,
            file_log_path,
        })
    }

    pub fn client(&self) -> LdkServerClient {
        self.client.clone()
    }

    pub fn listen_addr(&self) -> &str {
        &self.listen_addr
    }

    pub fn stdout_log_path(&self) -> &PathBuf {
        &self.stdout_log_path
    }

    pub fn file_log_path(&self) -> &PathBuf {
        &self.file_log_path
    }

    pub async fn wait_ready(&mut self, timeout: Duration) -> anyhow::Result<()> {
        let deadline = Instant::now() + timeout;

        loop {
            if let Some(status) = self.child.try_wait().context("poll ldk-server status")? {
                anyhow::bail!(
                    "ldk-server({}) exited early with status {status} (stdout_log={} file_log={})",
                    self.name,
                    self.stdout_log_path.display(),
                    self.file_log_path.display()
                );
            }

            if self
                .client
                .get_node_info(GetNodeInfoRequest {})
                .await
                .is_ok()
            {
                return Ok(());
            }

            if Instant::now() >= deadline {
                anyhow::bail!(
                    "timeout waiting for ldk-server({}) readiness (stdout_log={} file_log={})",
                    self.name,
                    self.stdout_log_path.display(),
                    self.file_log_path.display()
                );
            }

            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}

impl Drop for LdkServerProcess {
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
