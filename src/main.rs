mod ai;
mod modes;
mod services;
mod utils;

use anyhow::{Result, anyhow};
use modes::{pull_request, release};
use utils::config;

#[tokio::main]
async fn main() -> Result<()> {
    config::load();

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    let mode = config::get("MODE");

    if mode == "pr-summary" || mode == "pr-review" {
        return pull_request::handle_pr(&mode).await;
    }

    if mode == "release-summary" {
        return release::handle_release().await;
    }

    Err(anyhow!(
        "'mode' input must be set to either 'pr-summary', 'pr-review' or 'release-summary'",
    ))
}
