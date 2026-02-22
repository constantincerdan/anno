mod ai;
mod modes;
mod services;
mod utils;

use ai::AiProvider;
use anyhow::{Result, bail};
use modes::{pull_request, release};
use services::github::GitHubClient;
use utils::env;

#[tokio::main]
async fn main() -> Result<()> {
    env::load();

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    let mode = Mode::from_env()?;
    let provider = AiProvider::from_env()?;
    let gh = GitHubClient::new()?;

    match mode {
        Mode::PrSummary => {
            tracing::info!("Mode: pr-summary, AI provider: {provider}");
            pull_request::summarise(&gh, provider).await
        }
        Mode::ReleaseSummary => {
            tracing::info!("Mode: release-summary, AI provider: {provider}");
            release::summarise(&gh, provider).await
        }
    }
}

enum Mode {
    PrSummary,
    ReleaseSummary,
}

impl Mode {
    fn from_env() -> Result<Self> {
        let value = env::get("MODE")?;

        match value.as_str() {
            "pr-summary" => Ok(Self::PrSummary),
            "release-summary" => Ok(Self::ReleaseSummary),
            other => bail!("Invalid MODE '{other}': must be 'pr-summary' or 'release-summary'"),
        }
    }
}
