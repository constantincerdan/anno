use crate::utils::{env, http};
use anyhow::Result;
use serde::Deserialize;
use serde_json::json;

const LINEAR_API_URL: &str = "https://api.linear.app/graphql";

#[derive(Deserialize)]
pub struct LinearIssue {
    pub identifier: String,
    pub title: String,
    pub description: Option<String>,
    pub url: String,
}

#[derive(Deserialize)]
struct GraphQlResponse {
    data: Option<GraphQlData>,
}

#[derive(Deserialize)]
struct GraphQlData {
    issue: Option<LinearIssue>,
}

impl LinearIssue {
    pub async fn get_by_key(key: &str) -> Result<Option<Self>> {
        let linear_api_key = env::get("LINEAR_API_KEY")?;

        tracing::info!("Fetching Linear issue {key}");

        let query = json!({
            "query": "query IssueByIdentifier($id: String!) { issue(id: $id) { identifier title description url } }",
            "variables": { "id": key },
        });

        let response = http::send_with_retry(|| {
            http::client()
                .post(LINEAR_API_URL)
                .header("Content-Type", "application/json")
                .header("Authorization", &linear_api_key)
                .json(&query)
        })
        .await?
        .error_for_status()
        .inspect_err(|err| tracing::error!("Error fetching Linear issue: {err}"))?;

        // Linear returns HTTP 200 with a null issue when the key does not exist,
        // so a missing/typo'd key resolves to `None` rather than failing the run.
        let body = response.json::<GraphQlResponse>().await?;

        Ok(body.data.and_then(|data| data.issue))
    }

    pub fn get_github_hyperlink(&self) -> String {
        format!("[{} - {}]({})\n", self.identifier, self.title.trim(), self.url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_issue(identifier: &str, title: &str) -> LinearIssue {
        LinearIssue {
            identifier: identifier.to_string(),
            title: title.to_string(),
            description: None,
            url: format!("https://linear.app/acme/issue/{identifier}"),
        }
    }

    #[test]
    fn github_hyperlink() {
        let issue = make_issue("ENG-123", "Fix bug");
        assert_eq!(
            issue.get_github_hyperlink(),
            "[ENG-123 - Fix bug](https://linear.app/acme/issue/ENG-123)\n"
        );
    }

    #[test]
    fn github_hyperlink_trims_title() {
        let issue = make_issue("ENG-123", "  Fix bug  ");
        assert_eq!(
            issue.get_github_hyperlink(),
            "[ENG-123 - Fix bug](https://linear.app/acme/issue/ENG-123)\n"
        );
    }
}
