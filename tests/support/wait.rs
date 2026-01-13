use std::future::Future;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};

pub async fn wait_for<T, F, Fut>(description: &str, timeout: Duration, mut f: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Option<T>>>,
{
    let deadline = Instant::now() + timeout;
    let mut sleep_for = Duration::from_millis(200);

    loop {
        let now = Instant::now();
        if now >= deadline {
            anyhow::bail!("timeout waiting for {description}");
        }

        if let Some(value) = f().await.with_context(|| format!("poll {description}"))? {
            return Ok(value);
        }

        tokio::time::sleep(sleep_for).await;
        sleep_for = (sleep_for * 2).min(Duration::from_secs(2));
    }
}
