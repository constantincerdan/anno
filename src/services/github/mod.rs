pub mod issue;
pub mod pull_request;
pub mod repository;
pub mod workflows;

pub use issue::GitHubIssue;
pub use pull_request::PullRequest;
pub use repository::{CommitAuthor, Repository};

use crate::utils::{config, http};
use anyhow::Result;

pub struct GitHubClient {
    token: String,
    base_url: String,
}

impl GitHubClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            token: config::get("GITHUB_TOKEN")?,
            base_url: config::get("GITHUB_BASE_URL")?,
        })
    }

    pub fn get(&self, url: &str) -> reqwest::RequestBuilder {
        http::client()
            .get(url)
            .bearer_auth(&self.token)
            .header("User-Agent", "Anno")
    }

    pub fn patch(&self, url: &str) -> reqwest::RequestBuilder {
        http::client()
            .patch(url)
            .bearer_auth(&self.token)
            .header("User-Agent", "Anno")
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

pub const IGNORED_REPO_PATHS: [&str; 9] = [
    ".github",
    "build",
    "Cargo.lock",
    "coverage",
    "dist",
    "target",
    "node_modules",
    "package-lock.json",
    "yarn.lock",
];
