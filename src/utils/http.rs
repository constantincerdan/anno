use std::sync::LazyLock;
use std::time::Duration;

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to build HTTP client")
});

pub fn client() -> &'static reqwest::Client {
    &CLIENT
}

/// Sends a request with exponential backoff retry on 5xx/timeout/connection errors.
/// The closure must build a fresh RequestBuilder each call (since it's consumed by send).
pub async fn send_with_retry(
    build_request: impl Fn() -> reqwest::RequestBuilder,
) -> reqwest::Result<reqwest::Response> {
    const MAX_RETRIES: u32 = 3;

    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
        }

        match build_request().send().await {
            Ok(resp) if resp.status().is_server_error() && attempt < MAX_RETRIES - 1 => {
                tracing::warn!(
                    "HTTP {} response, retrying ({}/{})",
                    resp.status(),
                    attempt + 1,
                    MAX_RETRIES
                );
                continue;
            }
            result => return result,
        }
    }

    unreachable!()
}
