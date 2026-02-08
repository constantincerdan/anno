use crate::utils::{config, http};
use anyhow::Result;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Issue {
    pub key: String,
    pub fields: IssueFields,
}

impl Issue {
    pub async fn get_by_key(key: &str) -> Result<Option<Self>> {
        let jira_base_url = config::get("JIRA_BASE_URL")?;
        let jira_api_key = config::get("JIRA_API_KEY")?;

        tracing::info!("Fetching Jira issue {key}");

        let url = format!("{jira_base_url}/rest/api/2/issue/{key}");
        let auth = format!("Basic {jira_api_key}");

        let response = match http::send_with_retry(|| {
            http::client()
                .get(&url)
                .header("Accept", "application/json")
                .header("Authorization", &auth)
        })
        .await?
        .error_for_status()
        {
            Ok(res) => res,
            Err(err) => {
                if err.status() == Some(reqwest::StatusCode::NOT_FOUND) {
                    return Ok(None);
                }

                tracing::error!("Error fetching Jira issue: {err}");
                Err(err)
            }?,
        };

        let issue = response.json::<Self>().await?;

        Ok(Some(issue))
    }

    pub fn get_browse_url(&self, jira_base_url: &str) -> String {
        format!("{jira_base_url}/browse/{}", self.key)
    }

    pub fn get_github_hyperlink(&self, jira_base_url: &str) -> String {
        format!(
            "[{} - {}]({})\n",
            self.key,
            self.fields.summary.trim(),
            self.get_browse_url(jira_base_url)
        )
    }
}

#[derive(Deserialize)]
pub struct IssueFields {
    pub summary: String,
    pub description: Option<String>,
}
