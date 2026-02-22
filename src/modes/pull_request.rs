use crate::{
    ai::{self, AiProvider},
    services::{
        github::{GitHubClient, PullRequest},
        jira::Issue,
    },
    utils::{config, jira as jira_utils},
};
use anyhow::{Context, Result};
use futures::stream::{self, StreamExt, TryStreamExt};

pub async fn summarise(gh: &GitHubClient, provider: AiProvider) -> Result<()> {
    let repo_name = config::get("GITHUB_REPOSITORY")?;
    let ref_name = config::get("GITHUB_REF_NAME")?;

    let pr_number = ref_name
        .split('/')
        .next()
        .context("PR number not found in GITHUB_REF_NAME")?;

    let pr = PullRequest::get(gh, &repo_name, pr_number).await?;

    if pr.user.is_bot() {
        tracing::info!("Is a bot, skipping");
        return Ok(());
    }

    generate_and_set_summary(gh, pr, provider).await
}

async fn generate_and_set_summary(
    gh: &GitHubClient,
    pr: PullRequest,
    provider: AiProvider,
) -> Result<()> {
    let diff = pr.get_diff(gh).await?;
    let commit_messages = pr.get_commit_messages(gh).await?;
    let issues = get_jira_issues(&pr).await?;

    let summary = ai::PrSummary::new(provider, &diff, &commit_messages, &issues).await?;

    let jira_base_url = config::get_optional("JIRA_BASE_URL");
    let pr_body = get_pr_body(&summary, &pr, &issues, jira_base_url.as_deref());
    pr.set_body(gh, pr_body).await?;

    Ok(())
}

async fn get_jira_issues(pr: &PullRequest) -> Result<Vec<Issue>> {
    let jira_enabled = config::get_optional("JIRA_API_KEY").is_some();

    if !jira_enabled {
        return Ok(Vec::new());
    }

    let branches = [pr.head.r#ref.as_str()];
    let bodies: Vec<&str> = pr.body.as_deref().into_iter().collect();

    let keys = jira_utils::extract_issue_keys(&branches, &bodies, &[]);

    let mut issues: Vec<_> = stream::iter(keys)
        .map(async |key| Issue::get_by_key(&key).await)
        .buffer_unordered(5)
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .flatten()
        .collect();

    issues.sort_by(|a, b| a.key.cmp(&b.key));

    Ok(issues)
}

fn get_pr_body(
    summary: &ai::PrSummary,
    pr: &PullRequest,
    issues: &[Issue],
    jira_base_url: Option<&str>,
) -> String {
    let mut body = String::new();

    if let Some(existing_body) = &pr.body {
        body = format!("{existing_body}<hr>\n{body}\n");
    }

    if !issues.is_empty() {
        body.push_str("**Tickets**\n");

        for issue in issues {
            body.push_str(&format!(
                "- {}\n",
                issue.get_github_hyperlink(jira_base_url.unwrap_or_default())
            ));
        }
    }

    body.push_str(&format!("**Summary**\n\n{}", summary.summary));

    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_pr(body: Option<&str>) -> PullRequest {
        serde_json::from_value(json!({
            "number": 1,
            "title": "Test PR",
            "html_url": "https://github.com/test/repo/pull/1",
            "body": body,
            "user": { "type": "User" },
            "head": { "ref": "feature/test" },
            "url": "https://api.github.com/repos/test/repo/pulls/1",
            "commits_url": "https://api.github.com/repos/test/repo/pulls/1/commits"
        }))
        .unwrap()
    }

    fn make_summary(text: &str) -> ai::PrSummary {
        serde_json::from_value(json!({ "summary": text })).unwrap()
    }

    fn make_issue(key: &str, summary: &str) -> Issue {
        Issue {
            key: key.to_string(),
            fields: crate::services::jira::IssueFields {
                summary: summary.to_string(),
                description: None,
            },
        }
    }

    #[test]
    fn body_with_no_existing_body_no_issues() {
        let pr = make_pr(None);
        let summary = make_summary("Changes were made");
        let result = get_pr_body(&summary, &pr, &[], None);
        assert_eq!(result, "**Summary**\n\nChanges were made");
    }

    #[test]
    fn body_prepends_existing_body() {
        let pr = make_pr(Some("Existing description"));
        let summary = make_summary("Changes were made");
        let result = get_pr_body(&summary, &pr, &[], None);
        assert!(result.starts_with("Existing description<hr>"));
        assert!(result.contains("**Summary**\n\nChanges were made"));
    }

    #[test]
    fn body_includes_jira_tickets() {
        let pr = make_pr(None);
        let summary = make_summary("Changes were made");
        let issues = vec![
            make_issue("PROJ-1", "First"),
            make_issue("PROJ-2", "Second"),
        ];
        let result = get_pr_body(&summary, &pr, &issues, Some("https://jira.example.com"));
        assert!(result.contains("**Tickets**"));
        assert!(result.contains("PROJ-1 - First"));
        assert!(result.contains("PROJ-2 - Second"));
        assert!(result.contains("**Summary**\n\nChanges were made"));
    }

    #[test]
    fn body_with_no_jira_base_url() {
        let pr = make_pr(None);
        let summary = make_summary("Changes were made");
        let issues = vec![make_issue("PROJ-1", "First")];
        let result = get_pr_body(&summary, &pr, &issues, None);
        assert!(result.contains("PROJ-1 - First"));
        assert!(result.contains("/browse/PROJ-1"));
    }
}
