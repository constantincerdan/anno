use super::{IGNORED_REPO_PATHS, repository::Commit};
use crate::utils::config;
use anyhow::Result;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub html_url: String,
    pub body: Option<String>,
    pub user: User,
    pub head: Head,
    url: String,
    commits_url: String,
}

impl PullRequest {
    pub async fn get(repo_name: &str, number: &str) -> Result<Self> {
        tracing::info!("Fetching pull request #{number}");

        let gh_token = config::get("GITHUB_TOKEN");

        let pr = reqwest::Client::new()
            .get(format!(
                "https://api.github.com/repos/{repo_name}/pulls/{number}"
            ))
            .bearer_auth(gh_token)
            .header("Accept", "application/json")
            .header("User-Agent", "Anno")
            .send()
            .await?
            .error_for_status()
            .inspect_err(|e| tracing::error!("Error fetching PR #{number}: {e}"))?
            .json()
            .await?;

        Ok(pr)
    }

    pub async fn set_body(&self, body: String) -> Result<()> {
        tracing::info!("Setting pull request #{} body", &self.number);

        let gh_token = config::get("GITHUB_TOKEN");

        reqwest::Client::new()
            .patch(&self.url)
            .bearer_auth(gh_token)
            .header("Accept", "application/json")
            .header("User-Agent", "Anno")
            .json(&json!({ "body": body }))
            .send()
            .await?
            .error_for_status()
            .inspect_err(|e| tracing::error!("Error setting PR body: {e}"))?;

        Ok(())
    }

    pub async fn get_diff(&self) -> Result<String> {
        tracing::info!("Fetching pull request #{} diff", &self.number);

        let gh_token = config::get("GITHUB_TOKEN");

        let diff = reqwest::Client::new()
            .get(&self.url)
            .bearer_auth(gh_token)
            .header("Accept", "application/vnd.github.diff")
            .header("User-Agent", "Anno")
            .send()
            .await?
            .error_for_status()
            .inspect_err(|e| tracing::error!("Error fetching PR diff: {e}"))?
            .text()
            .await?;

        let mut is_inside_ignored_file = false;

        let filtered_diff = diff
            .lines()
            .filter(|line| {
                if line.contains("diff --git") {
                    is_inside_ignored_file = IGNORED_REPO_PATHS.iter().any(|p| line.contains(p));
                }

                !is_inside_ignored_file
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(filtered_diff)
    }

    pub async fn get_commit_messages(&self) -> Result<Vec<String>> {
        tracing::info!("Fetching pull request #{} commit messages", &self.number);

        let gh_token = config::get("GITHUB_TOKEN");

        let mut all_commits: Vec<Commit> = Vec::new();
        let mut page = 1;
        loop {
            let commits: Vec<Commit> = reqwest::Client::new()
                .get(&self.commits_url)
                .bearer_auth(&gh_token)
                .header("Accept", "application/json")
                .header("User-Agent", "Anno")
                .query(&[("page", page), ("per_page", 100)])
                .send()
                .await?
                .error_for_status()
                .inspect_err(|e| tracing::error!("Error fetching PR commits: {e}"))?
                .json()
                .await?;

            if commits.is_empty() {
                break;
            }

            all_commits.extend(commits);

            page += 1;
        }

        let all_messages = all_commits.into_iter().map(|c| c.commit.message).collect();

        Ok(all_messages)
    }
}

#[derive(Deserialize)]
pub struct Head {
    pub r#ref: String,
}

#[derive(Deserialize)]
pub struct User {
    r#type: UserType,
}

impl User {
    pub fn is_bot(&self) -> bool {
        matches!(self.r#type, UserType::Bot)
    }
}

#[derive(Deserialize)]
enum UserType {
    User,
    Bot,
}
