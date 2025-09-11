use crate::utils::config;
use anyhow::Result;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Issue {
    pub key: String,
    pub fields: IssueFields,
}

impl Issue {
    pub async fn get_by_key(key: &str) -> Result<Option<Self>> {
        let jira_base_url = config::get("JIRA_BASE_URL");
        let jira_api_key = config::get("JIRA_API_KEY");

        tracing::info!("Fetching Jira issue {key}");

        let response = match reqwest::Client::new()
            .get(format!("{jira_base_url}/rest/api/2/issue/{key}"))
            .header("Accept", "application/json")
            .header("Authorization", format!("Basic {jira_api_key}"))
            .send()
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

    pub fn get_browse_url(&self) -> String {
        let jira_base_url = config::get("JIRA_BASE_URL");
        format!("{jira_base_url}/browse/{}", self.key)
    }

    pub fn get_github_hyperlink(&self) -> String {
        format!(
            "[{} - {}]({})\n",
            self.key,
            self.fields.summary.trim(),
            self.get_browse_url()
        )
    }
}

#[derive(Deserialize)]
pub struct IssueFields {
    pub summary: String,
    pub description: Option<String>,
}
