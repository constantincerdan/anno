use crate::utils::{config, http};

use super::pull_request::PullRequest;
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
pub struct Repository {
    pub full_name: String,
    pub name: String,
    pub html_url: String,
    compare_url: String,
    contents_url: String,
    commits_url: String,
    pub default_branch: String,
}

impl Repository {
    pub fn get_compare_url(&self, old_sha: &str, new_sha: &str) -> String {
        format!(
            "https://github.com/{}/compare/{}...{}",
            self.full_name, old_sha, new_sha
        )
    }

    pub fn get_commit_url(&self, sha: &str) -> String {
        format!("{}/commit/{sha}", self.html_url)
    }

    pub async fn get_pull_requests_for_commit(&self, sha: &str) -> Result<Vec<PullRequest>> {
        tracing::info!("Fetching associated pull requests for commit {sha}");

        let gh_token = config::get("GITHUB_TOKEN")?;
        let url = self.commits_url.replace("{/sha}", &format!("/{sha}/pulls"));

        let response = http::send_with_retry(|| {
            http::client()
                .get(&url)
                .bearer_auth(&gh_token)
                .header("Accept", "application/json")
                .header("User-Agent", "Anno")
        })
        .await?
        .error_for_status()
        .inspect_err(|e| tracing::error!("Error getting associated PRs: {e}"))?
        .json::<Vec<PullRequest>>()
        .await?;

        Ok(response)
    }

    pub async fn get_file(&self, path: &str) -> Result<RepoFile> {
        tracing::info!("Fetching file {path}");

        let gh_token = config::get("GITHUB_TOKEN")?;
        let url = self.contents_url.replace("{+path}", path);

        let response = http::send_with_retry(|| {
            http::client()
                .get(&url)
                .bearer_auth(&gh_token)
                .header("Accept", "application/json")
                .header("User-Agent", "Anno")
        })
        .await?
        .error_for_status()
        .inspect_err(|e| tracing::error!("Error getting repo file: {e}"))?
        .json::<RepoFile>()
        .await?;

        Ok(response)
    }

    pub async fn get_diff_for_commit(&self, sha: &str) -> Result<String> {
        tracing::info!("Fetching diff for commit {sha}");

        let gh_token = config::get("GITHUB_TOKEN")?;
        let url = self.commits_url.replace("{/sha}", &format!("/{sha}"));

        let diff = http::send_with_retry(|| {
            http::client()
                .get(&url)
                .bearer_auth(&gh_token)
                .header("Accept", "application/vnd.github.diff")
                .header("User-Agent", "Anno")
        })
        .await?
        .error_for_status()
        .inspect_err(|e| tracing::error!("Error getting commit diff: {e}"))?
        .text()
        .await?;

        Ok(diff)
    }

    pub async fn get_commit_message(&self, sha: &str) -> Result<String> {
        tracing::info!("Fetching commit message for commit {sha}");

        let gh_token = config::get("GITHUB_TOKEN")?;
        let url = self.commits_url.replace("{/sha}", &format!("/{sha}"));

        let message = http::send_with_retry(|| {
            http::client()
                .get(&url)
                .bearer_auth(&gh_token)
                .header("Accept", "application/json")
                .header("User-Agent", "Anno")
        })
        .await?
        .error_for_status()
        .inspect_err(|e| tracing::error!("Error getting commit message: {e}"))?
        .json::<Commit>()
        .await?
        .commit
        .message;

        Ok(message)
    }

    pub async fn get_diff_between_commits(&self, old_sha: &str, new_sha: &str) -> Result<String> {
        tracing::info!("Fetching diff between commits {old_sha} and {new_sha}");

        let gh_token = config::get("GITHUB_TOKEN")?;
        let url = self
            .compare_url
            .replace("{base}...{head}", &format!("{old_sha}...{new_sha}"));

        let diff = http::send_with_retry(|| {
            http::client()
                .get(&url)
                .bearer_auth(&gh_token)
                .header("Accept", "application/vnd.github.diff")
                .header("User-Agent", "Anno")
        })
        .await?
        .error_for_status()
        .inspect_err(|e| tracing::error!("Error fetching repo diff: {e}"))?
        .text()
        .await?;

        Ok(diff)
    }

    pub async fn get_contributors_between_commits(
        &self,
        old_sha: &str,
        new_sha: &str,
    ) -> Result<Vec<CommitAuthor>> {
        tracing::info!("Fetching contributors between {old_sha} and {new_sha}");

        let gh_token = config::get("GITHUB_TOKEN")?;
        let url = self
            .compare_url
            .replace("{base}...{head}", &format!("{old_sha}...{new_sha}"));

        let response = http::send_with_retry(|| {
            http::client()
                .get(&url)
                .bearer_auth(&gh_token)
                .header("Accept", "application/json")
                .header("User-Agent", "Anno")
        })
        .await?
        .error_for_status()
        .inspect_err(|e| tracing::error!("Error fetching compare data: {e}"))?
        .json::<CompareResponse>()
        .await?;

        Ok(unique_contributors(
            response.commits.into_iter().filter_map(|c| c.author),
        ))
    }

    pub async fn get_commit_contributors(&self, sha: &str) -> Result<Vec<CommitAuthor>> {
        tracing::info!("Fetching contributor for commit {sha}");

        let gh_token = config::get("GITHUB_TOKEN")?;
        let url = self.commits_url.replace("{/sha}", &format!("/{sha}"));

        let commit = http::send_with_retry(|| {
            http::client()
                .get(&url)
                .bearer_auth(&gh_token)
                .header("Accept", "application/json")
                .header("User-Agent", "Anno")
        })
        .await?
        .error_for_status()
        .inspect_err(|e| tracing::error!("Error fetching commit: {e}"))?
        .json::<Commit>()
        .await?;

        Ok(commit.author.into_iter().collect())
    }

    pub fn get_compare_to_default_branch_url(&self, commit: &str) -> String {
        format!(
            "{}/compare/{}...{}",
            self.html_url, commit, self.default_branch
        )
    }
}

#[derive(Deserialize)]
pub struct RepoFile {
    pub content: String,
}

#[derive(Deserialize)]
pub struct Commit {
    pub commit: CommitDetails,
    pub author: Option<CommitAuthor>,
}

#[derive(Deserialize)]
pub struct CommitDetails {
    pub message: String,
}

#[derive(Deserialize)]
pub struct CommitAuthor {
    pub login: String,
    pub avatar_url: String,
}

#[derive(Deserialize)]
struct CompareResponse {
    commits: Vec<Commit>,
}

fn unique_contributors(authors: impl Iterator<Item = CommitAuthor>) -> Vec<CommitAuthor> {
    let mut seen = HashSet::new();
    authors.filter(|a| seen.insert(a.login.clone())).collect()
}
