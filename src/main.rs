mod ai;
mod modes;
mod services;
mod utils;

use ai::AiProvider;
use anyhow::{Result, anyhow};
use modes::{pull_request, release};
use services::github::GitHubClient;
use utils::config;

#[tokio::main]
async fn main() -> Result<()> {
    config::load();

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    let mode = config::get("MODE")?;
    let provider = AiProvider::from_config()?;
    let gh = GitHubClient::new()?;

    tracing::info!("Mode: {mode}, AI provider: {provider}");

    if mode == "pr-summary" {
        return pull_request::handle_pr(&gh, &mode, provider).await;
    }

    if mode == "release-summary" {
        return release::handle_release(&gh, provider).await;
    }

    Err(anyhow!(
        "'mode' input must be set to either 'pr-summary' or 'release-summary'",
    ))
}
