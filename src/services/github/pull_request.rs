use super::GitHubClient;
use super::repository::Commit;
use crate::utils::{http, target_paths::TargetPaths};
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
    pub async fn get(gh: &GitHubClient, repo_name: &str, number: &str) -> Result<Self> {
        tracing::info!("Fetching pull request #{number}");

        let url = format!("https://api.github.com/repos/{repo_name}/pulls/{number}");

        let pr = http::send_with_retry(|| gh.get(&url).header("Accept", "application/json"))
            .await?
            .error_for_status()
            .inspect_err(|e| tracing::error!("Error fetching PR #{number}: {e}"))?
            .json()
            .await?;

        Ok(pr)
    }

    pub async fn set_body(&self, gh: &GitHubClient, body: String) -> Result<()> {
        tracing::info!("Setting pull request #{} body", &self.number);

        let payload = json!({ "body": body });

        http::send_with_retry(|| {
            gh.patch(&self.url)
                .header("Accept", "application/json")
                .json(&payload)
        })
        .await?
        .error_for_status()
        .inspect_err(|e| tracing::error!("Error setting PR body: {e}"))?;

        Ok(())
    }

    pub async fn get_diff(&self, gh: &GitHubClient) -> Result<String> {
        tracing::info!("Fetching pull request #{} diff", &self.number);

        let diff = http::send_with_retry(|| {
            gh.get(&self.url)
                .header("Accept", "application/vnd.github.diff")
        })
        .await?
        .error_for_status()
        .inspect_err(|e| tracing::error!("Error fetching PR diff: {e}"))?
        .text()
        .await?;

        Ok(TargetPaths::default().filter_diff(&diff))
    }

    pub async fn get_commit_messages(&self, gh: &GitHubClient) -> Result<Vec<String>> {
        tracing::info!("Fetching pull request #{} commit messages", &self.number);

        let mut all_commits: Vec<Commit> = Vec::new();
        let mut page: u32 = 1;
        loop {
            let commits: Vec<Commit> = http::send_with_retry(|| {
                gh.get(&self.commits_url)
                    .header("Accept", "application/json")
                    .query(&[("page", page), ("per_page", 100)])
            })
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
