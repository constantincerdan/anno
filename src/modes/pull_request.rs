use crate::{
    ai,
    services::{github::PullRequest, jira::Issue},
    utils::config,
};
use anyhow::Result;
use futures::future::{try_join, try_join_all};
use regex_lite::Regex;
use std::collections::HashSet;

pub async fn handle_pr(mode: &str) -> Result<()> {
    let repo_name = config::get("GITHUB_REPOSITORY");
    let ref_name = config::get("GITHUB_REF_NAME");

    let pr_number = ref_name
        .split("/")
        .next()
        .expect("PR number to be in GITHUB_REF_NAME environment variable");

    let pr = PullRequest::get(&repo_name, pr_number).await?;

    if pr.user.is_bot() {
        tracing::info!("Is a bot, skipping");
        return Ok(());
    }

    if mode == "pr-summary" {
        return handle_pr_summary(pr).await;
    }

    if mode == "pr-review" {
        return handle_pr_review(pr).await;
    }

    Ok(())
}

async fn handle_pr_summary(pr: PullRequest) -> Result<()> {
    let diff = pr.get_diff().await?;
    let commit_messages = pr.get_commit_messages().await?;
    let issues = get_jira_issues(&pr).await?;

    let summary = ai::PrSummary::new(&diff, &commit_messages, &issues).await?;

    let pr_body = get_pr_body(summary, &pr, &issues);
    pr.set_body(pr_body).await?;

    Ok(())
}

async fn handle_pr_review(pr: PullRequest) -> Result<()> {
    let diff = pr.get_diff().await?;
    let commit_messages = pr.get_commit_messages().await?;

    let review = ai::PrReview::new(&diff, &commit_messages).await?;

    let anno_comments = pr.get_anno_comments().await?;
    let is_prev_positive = anno_comments.first().is_some_and(|c| c.is_positive());

    if review.is_positive() && is_prev_positive {
        return Ok(());
    }

    try_join(
        pr.clear_prev_comments(&anno_comments),
        pr.add_comment(&review.feedback),
    )
    .await?;
    Ok(())
}

async fn get_jira_issues(pr: &PullRequest) -> Result<Vec<Issue>> {
    let jira_enabled = config::get_optional("JIRA_API_KEY").is_some();

    if !jira_enabled {
        return Ok(Vec::new());
    }

    let key_regex = Regex::new(r"\b([A-Z]{2,10})-\d+\b").expect("Valid regex");

    let mut keys = HashSet::new();

    if let Some(key) = key_regex.find(&pr.head.r#ref) {
        keys.insert(key.as_str());
    }

    if let Some(body) = &pr.body {
        for key in key_regex.find_iter(body) {
            keys.insert(key.as_str());
        }
    }

    let requests = keys.into_iter().map(Issue::get_by_key).collect::<Vec<_>>();

    let mut issues: Vec<_> = try_join_all(requests)
        .await?
        .into_iter()
        .flatten()
        .collect();

    issues.sort_by(|a, b| a.key.cmp(&b.key));

    Ok(issues)
}

fn get_pr_body(summary: ai::PrSummary, pr: &PullRequest, issues: &[Issue]) -> String {
    let mut body = String::new();

    if let Some(existing_body) = &pr.body {
        body = format!("{existing_body}<hr>\n{body}\n");
    }

    if !issues.is_empty() {
        body.push_str("**Tickets**\n");

        for issue in issues {
            body.push_str(&format!("- {}\n", issue.get_github_hyperlink()));
        }
    }

    body.push_str(&format!("**Summary**\n\n{}", summary.summary));

    body
}
