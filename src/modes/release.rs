use crate::{
    ai,
    services::{
        github::{
            PullRequest, Repository,
            workflows::{PrevRuns, WorkflowConfig, WorkflowRun, WorkflowRuns},
        },
        jira::Issue,
        slack,
    },
    utils::{config, git::Git, target_paths::TargetPaths},
};
use anyhow::Result;
use futures::future::{try_join, try_join_all, try_join3};
use regex_lite::Regex;
use std::collections::HashSet;

pub async fn handle_release() -> Result<()> {
    let repo_name = config::get("GITHUB_REPOSITORY");
    let run_id = config::get("GITHUB_RUN_ID");

    let run = WorkflowRun::get_by_id(&repo_name, &run_id).await?;

    if run.has_prev_successful_attempt().await? {
        tracing::warn!("Already previously deployed, skipping");
        return Ok(());
    }

    if let Some(prev_runs) = WorkflowRuns::get_prev_runs_with_last_success_for_branch(&run).await? {
        handle_master_release(run, prev_runs).await
    } else {
        tracing::info!("No previous successful run found for branch, summarising run commit");
        handle_non_master_release(run).await
    }
}

async fn handle_master_release(run: WorkflowRun, prev_runs: PrevRuns) -> Result<()> {
    let repo = run.get_repo().await?;
    let app_name = config::get_optional("APP_NAME").unwrap_or(repo.name.clone());

    let new_commit = &run.head_sha;
    let old_commit = &prev_runs.last_successful.head_sha;

    let mut diff = repo
        .get_diff_between_commits(old_commit, new_commit)
        .await?;

    if diff.is_empty() {
        tracing::warn!("No changes found between commits; skipping");
        return Ok(());
    }

    let target_paths = repo
        .get_file(&run.path)
        .await
        .and_then(WorkflowConfig::from_file)
        .map(TargetPaths::new)?;

    diff = target_paths.filter_diff(&diff);

    if diff.is_empty() {
        tracing::warn!("No changes found for the configured paths; skipping");
        return Ok(());
    }

    let commit_messages =
        Git::init(&repo.full_name)?.get_commit_messages(old_commit, new_commit, &target_paths)?;
    let pull_requests = get_pull_requests(&run, Some(&prev_runs.prev_runs), &repo).await?;

    let (jira_issues, summary) = try_join(
        get_jira_issues(&pull_requests, &commit_messages),
        ai::ReleaseSummary::new(&diff, &commit_messages),
    )
    .await?;

    let diff_url = repo.get_compare_url(old_commit, new_commit);
    let prev_run_url = prev_runs.last_successful.get_run_url();
    let compare_to_master_url = repo.get_compare_to_master_url(new_commit);

    slack::ReleaseSummary {
        app_name,
        diff_url,
        compare_to_master_url,
        prev_run_url: Some(prev_run_url),
        jira_issues,
        pull_requests,
        run: &run,
        summary,
    }
    .send()
    .await
}

async fn handle_non_master_release(run: WorkflowRun) -> Result<()> {
    let repo = run.get_repo().await?;
    let app_name = config::get_optional("APP_NAME").unwrap_or(repo.name.clone());

    let (diff, pull_requests, commit_message) = try_join3(
        repo.get_diff_for_commit(&run.head_sha),
        get_pull_requests(&run, None, &repo),
        repo.get_commit_message(&run.head_sha),
    )
    .await?;

    let prev_run = WorkflowRuns::get_prev_successful_run(&run).await?;
    let prev_run_url = prev_run.as_ref().map(|run| run.get_run_url());
    let diff_url = repo.get_commit_url(&run.head_sha);
    let compare_to_master_url = repo.get_compare_to_master_url(&run.head_sha);

    let (jira_issues, summary) = try_join(
        get_jira_issues(&pull_requests, &[commit_message.clone()]),
        ai::ReleaseSummary::new(&diff, &[commit_message]),
    )
    .await?;

    slack::ReleaseSummary {
        app_name,
        diff_url,
        compare_to_master_url,
        prev_run_url,
        jira_issues,
        pull_requests,
        run: &run,
        summary,
    }
    .send()
    .await
}

async fn get_pull_requests(
    curr_run: &WorkflowRun,
    prev_runs: Option<&[WorkflowRun]>,
    repo: &Repository,
) -> Result<Vec<PullRequest>> {
    let mut pull_requests = repo
        .get_pull_requests_for_commit(&curr_run.head_sha)
        .await?;

    let Some(prev_runs) = prev_runs else {
        return Ok(pull_requests);
    };

    for prev_run in prev_runs {
        let prs = repo
            .get_pull_requests_for_commit(&prev_run.head_sha)
            .await?;

        pull_requests.extend(prs);
    }

    let mut pr_keys = HashSet::new();
    pull_requests.retain(|pr| pr_keys.insert(pr.number));
    pull_requests.sort_by_key(|pr| pr.number);

    Ok(pull_requests)
}

async fn get_jira_issues(
    pull_requests: &[PullRequest],
    commit_messages: &[String],
) -> Result<Vec<Issue>> {
    let jira_enabled = config::get_optional("JIRA_API_KEY").is_some();

    if !jira_enabled {
        return Ok(Vec::new());
    }

    let key_regex = Regex::new(r"\b([A-Z]{2,10})-\d+\b").expect("Valid regex");

    let mut keys = HashSet::new();

    for pr in pull_requests {
        if let Some(key) = key_regex.find(&pr.head.r#ref) {
            keys.insert(key.as_str());
        }

        let Some(body) = &pr.body else {
            continue;
        };

        for key in key_regex.find_iter(body) {
            keys.insert(key.as_str());
        }
    }

    for message in commit_messages {
        if let Some(key) = key_regex.find(message) {
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
