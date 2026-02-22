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

pub async fn handle_pr(gh: &GitHubClient, provider: AiProvider) -> Result<()> {
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

    handle_pr_summary(gh, pr, provider).await
}

async fn handle_pr_summary(gh: &GitHubClient, pr: PullRequest, provider: AiProvider) -> Result<()> {
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
