use super::{AccessToken, IGNORED_REPO_PATHS, repository::Commit};
use crate::utils::config;
use anyhow::Result;
use serde::Deserialize;
use serde_json::json;
use std::cmp::Reverse;

#[derive(Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub html_url: String,
    pub body: Option<String>,
    pub user: User,
    pub head: Head,
    url: String,
    comments_url: String,
    commits_url: String,
}

impl PullRequest {
    pub async fn get(repo_name: &str, number: &str) -> Result<Self> {
        tracing::info!("Fetching pull request #{number}");

        let gh_token = AccessToken::get().await?;

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

        let gh_token = AccessToken::get().await?;

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

        let gh_token = AccessToken::get().await?;

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

        let gh_token = AccessToken::get().await?;

        let mut all_commits: Vec<Commit> = Vec::new();
        let mut page = 1;
        loop {
            let commits: Vec<Commit> = reqwest::Client::new()
                .get(&self.commits_url)
                .bearer_auth(gh_token)
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

    pub async fn get_anno_comments(&self) -> Result<Vec<Comment>> {
        let mut comments: Vec<_> = self
            .list_comments()
            .await?
            .into_iter()
            .filter(|c| c.is_by_anno())
            .collect();

        comments.sort_by_key(|c| Reverse(c.created_at.clone()));

        Ok(comments)
    }

    pub async fn add_comment(&self, comment: &str) -> Result<()> {
        tracing::info!("Adding pull request #{} comment", &self.number);

        let pr_comment_enabled = config::get("PR_COMMENT_ENABLED") == "true";

        if !pr_comment_enabled {
            println!("{comment}");
            return Ok(());
        }

        let gh_token = AccessToken::get().await?;

        reqwest::Client::new()
            .post(&self.comments_url)
            .bearer_auth(gh_token)
            .header("Accept", "application/json")
            .header("User-Agent", "Anno")
            .json(&json!({ "body": format!("<!-- anno -->{comment}") }))
            .send()
            .await?
            .error_for_status()
            .inspect_err(|e| tracing::error!("Error adding GitHub comment: {e}"))?;

        Ok(())
    }

    pub async fn clear_prev_comments(&self, comments: &[Comment]) -> Result<()> {
        let pr_comment_enabled = config::get("PR_COMMENT_ENABLED") == "true";

        if !pr_comment_enabled {
            return Ok(());
        }

        let (positives, negatives) = comments.iter().partition::<Vec<_>, _>(|c| c.is_positive());

        if let Some(prev_positive) = positives.first() {
            prev_positive.delete().await?;
        }

        if let Some(prev_negative) = negatives.first() {
            prev_negative.hide_as_outdated().await?;
        }

        Ok(())
    }

    async fn list_comments(&self) -> Result<Vec<Comment>> {
        tracing::info!("Getting pull request #{} comments", &self.number);

        let gh_token = AccessToken::get().await?;

        let comments = reqwest::Client::new()
            .get(&self.comments_url)
            .bearer_auth(gh_token)
            .header("Accept", "application/json")
            .header("User-Agent", "Anno")
            .query(&[("per_page", "100")])
            .send()
            .await?
            .error_for_status()
            .inspect_err(|e| tracing::error!("Error getting GitHub comments: {e}"))?
            .json::<Vec<Comment>>()
            .await?;

        Ok(comments)
    }
}

#[derive(Deserialize)]
pub struct Head {
    pub r#ref: String,
}

#[derive(Deserialize)]
pub struct Comment {
    body: String,
    url: String,
    node_id: String,
    created_at: String,
}

impl Comment {
    pub fn is_positive(&self) -> bool {
        self.is_by_anno() && self.body.contains("LGTM")
    }

    pub fn is_by_anno(&self) -> bool {
        self.body.starts_with("<!-- anno -->")
    }

    pub async fn hide_as_outdated(&self) -> Result<()> {
        tracing::info!("Marking comment {} as outdated", &self.node_id);

        let gh_token = AccessToken::get().await?;
        let mutation = format!(
            r#"
            mutation {{
                minimizeComment(input: {{
                    subjectId: "{comment_id}",
                    classifier: OUTDATED
                }}) {{
                    minimizedComment {{
                        isMinimized
                    }}
                }}
            }}"#,
            comment_id = &self.node_id
        );

        reqwest::Client::new()
            .post("https://api.github.com/graphql")
            .bearer_auth(gh_token)
            .header("Accept", "application/json")
            .header("User-Agent", "Anno")
            .json(&json!({ "query": mutation }))
            .send()
            .await?
            .error_for_status()
            .inspect_err(|e| tracing::error!("Error hiding GitHub comment: {e}"))?;

        Ok(())
    }

    pub async fn delete(&self) -> Result<()> {
        tracing::info!("Deleting comment {}", &self.node_id);

        let gh_token = AccessToken::get().await?;

        reqwest::Client::new()
            .delete(&self.url)
            .bearer_auth(gh_token)
            .header("Accept", "application/json")
            .header("User-Agent", "Anno")
            .send()
            .await?
            .error_for_status()
            .inspect_err(|e| tracing::error!("Error deleting GitHub comment: {e}"))?;

        Ok(())
    }
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
