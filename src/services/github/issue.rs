use super::GitHubClient;
use crate::utils::http;
use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
pub struct GitHubIssue {
    pub number: u64,
    pub title: String,
    pub html_url: String,
    pull_request: Option<Value>,
}

impl GitHubIssue {
    pub async fn get(gh: &GitHubClient, repo_name: &str, number: u64) -> Result<Option<Self>> {
        tracing::info!("Fetching GitHub issue #{number}");

        let url = format!("{}/repos/{repo_name}/issues/{number}", gh.base_url());

        let response =
            match http::send_with_retry(|| gh.get(&url).header("Accept", "application/json"))
                .await?
                .error_for_status()
            {
                Ok(res) => res,
                Err(err) => {
                    if err.status() == Some(reqwest::StatusCode::NOT_FOUND) {
                        return Ok(None);
                    }

                    tracing::error!("Error fetching GitHub issue #{number}: {err}");
                    Err(err)?
                }
            };

        let issue = response.json::<Self>().await?;

        if issue.pull_request.is_some() {
            return Ok(None);
        }

        Ok(Some(issue))
    }
}
