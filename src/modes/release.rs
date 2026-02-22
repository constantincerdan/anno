use crate::{
    ai::{self, AiProvider, release_summary::PrContext},
    services::{
        github::{
            GitHubClient, GitHubIssue, PullRequest, Repository,
            workflows::{PrevRuns, WorkflowConfig, WorkflowRun, WorkflowRuns},
        },
        jira::Issue,
        slack,
    },
    utils::{
        config, git::Git, github as github_utils, jira as jira_utils, target_paths::TargetPaths,
    },
};
use anyhow::Result;
use futures::future::{try_join3, try_join4};
use futures::stream::{self, StreamExt, TryStreamExt};
use std::collections::HashSet;

pub async fn handle_release(gh: &GitHubClient, provider: AiProvider) -> Result<()> {
    let repo_name = config::get("GITHUB_REPOSITORY")?;
    let run_id = config::get("GITHUB_RUN_ID")?;

    let run = WorkflowRun::get_by_id(gh, &repo_name, &run_id).await?;

    if run.has_prev_successful_attempt(gh).await? {
        tracing::warn!("Already previously deployed, skipping");
        return Ok(());
    }

    let repo = run.get_repo(gh).await?;

    if run.head_branch == repo.default_branch {
        if let Some(prev_runs) =
            WorkflowRuns::get_prev_runs_with_last_success_for_branch(gh, &run).await?
        {
            handle_default_branch_release(gh, run, repo, prev_runs, provider).await
        } else {
            tracing::info!("First deploy on default branch, summarising run commit");
            handle_non_default_branch_release(gh, run, repo, provider).await
        }
    } else {
        tracing::info!("Non-default branch deploy, summarising run commit");
        handle_non_default_branch_release(gh, run, repo, provider).await
    }
}

async fn handle_default_branch_release(
    gh: &GitHubClient,
    run: WorkflowRun,
    repo: Repository,
    prev_runs: PrevRuns,
    provider: AiProvider,
) -> Result<()> {
    let app_name = config::get_optional("APP_NAME").unwrap_or_else(|| repo.name.clone());

    let new_commit = &run.head_sha;
    let old_commit = &prev_runs.last_successful.head_sha;

    let mut diff = repo
        .get_diff_between_commits(gh, old_commit, new_commit)
        .await?;

    if diff.is_empty() {
        tracing::warn!("No changes found between commits; skipping");
        return Ok(());
    }

    let target_paths = repo
        .get_file(gh, &run.path)
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
    let pull_requests = get_pull_requests(gh, &run, Some(&prev_runs.prev_runs), &repo).await?;

    let pr_contexts: Vec<_> = pull_requests
        .iter()
        .filter_map(PrContext::from_pr)
        .collect();

    let (jira_issues, github_issues, summary, contributors) = try_join4(
        get_jira_issues(&pull_requests, &commit_messages),
        get_github_issues(gh, &repo.full_name, &pull_requests, &commit_messages),
        ai::ReleaseSummary::new(provider, &diff, &commit_messages, &pr_contexts),
        repo.get_contributors_between_commits(gh, old_commit, new_commit),
    )
    .await?;

    let diff_url = repo.get_compare_url(old_commit, new_commit);
    let prev_run_url = prev_runs.last_successful.get_run_url();
    let compare_to_default_branch_url = repo.get_compare_to_default_branch_url(new_commit);

    slack::ReleaseSummary {
        app_name,
        jira_base_url: config::get_optional("JIRA_BASE_URL"),
        diff_url,
        compare_to_default_branch_url,
        default_branch: repo.default_branch,
        prev_run_url: Some(prev_run_url),
        contributors,
        github_issues,
        jira_issues,
        pull_requests,
        run: &run,
        summary,
    }
    .send()
    .await
}

async fn handle_non_default_branch_release(
    gh: &GitHubClient,
    run: WorkflowRun,
    repo: Repository,
    provider: AiProvider,
) -> Result<()> {
    let app_name = config::get_optional("APP_NAME").unwrap_or_else(|| repo.name.clone());

    let (diff, pull_requests, commit_message, contributors) = try_join4(
        repo.get_diff_for_commit(gh, &run.head_sha),
        get_pull_requests(gh, &run, None, &repo),
        repo.get_commit_message(gh, &run.head_sha),
        repo.get_commit_contributors(gh, &run.head_sha),
    )
    .await?;

    let prev_run = WorkflowRuns::get_prev_successful_run(gh, &run).await?;
    let prev_run_url = prev_run.as_ref().map(|run| run.get_run_url());
    let diff_url = repo.get_commit_url(&run.head_sha);
    let compare_to_default_branch_url = repo.get_compare_to_default_branch_url(&run.head_sha);
    let default_branch = repo.default_branch;

    let commit_messages = std::slice::from_ref(&commit_message);
    let pr_contexts: Vec<_> = pull_requests
        .iter()
        .filter_map(PrContext::from_pr)
        .collect();

    let (jira_issues, github_issues, summary) = try_join3(
        get_jira_issues(&pull_requests, commit_messages),
        get_github_issues(gh, &repo.full_name, &pull_requests, commit_messages),
        ai::ReleaseSummary::new(provider, &diff, commit_messages, &pr_contexts),
    )
    .await?;

    slack::ReleaseSummary {
        app_name,
        jira_base_url: config::get_optional("JIRA_BASE_URL"),
        diff_url,
        compare_to_default_branch_url,
        default_branch,
        prev_run_url,
        contributors,
        github_issues,
        jira_issues,
        pull_requests,
        run: &run,
        summary,
    }
    .send()
    .await
}

async fn get_pull_requests(
    gh: &GitHubClient,
    curr_run: &WorkflowRun,
    prev_runs: Option<&[WorkflowRun]>,
    repo: &Repository,
) -> Result<Vec<PullRequest>> {
    let mut pull_requests = repo
        .get_pull_requests_for_commit(gh, &curr_run.head_sha)
        .await?;

    let Some(prev_runs) = prev_runs else {
        return Ok(pull_requests);
    };

    for prev_run in prev_runs {
        let prs = repo
            .get_pull_requests_for_commit(gh, &prev_run.head_sha)
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

    let branches: Vec<&str> = pull_requests
        .iter()
        .map(|pr| pr.head.r#ref.as_str())
        .collect();
    let bodies: Vec<&str> = pull_requests
        .iter()
        .filter_map(|pr| pr.body.as_deref())
        .collect();

    let keys = jira_utils::extract_issue_keys(&branches, &bodies, commit_messages);

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

async fn get_github_issues(
    gh: &GitHubClient,
    repo_name: &str,
    pull_requests: &[PullRequest],
    commit_messages: &[String],
) -> Result<Vec<GitHubIssue>> {
    let bodies: Vec<&str> = pull_requests
        .iter()
        .filter_map(|pr| pr.body.as_deref())
        .collect();

    let pr_numbers: HashSet<u64> = pull_requests.iter().map(|pr| pr.number).collect();

    let numbers: Vec<u64> = github_utils::extract_issue_numbers(&bodies, commit_messages)
        .into_iter()
        .filter(|n| !pr_numbers.contains(n))
        .collect();

    let mut issues: Vec<_> = stream::iter(numbers)
        .map(async |number| GitHubIssue::get(gh, repo_name, number).await)
        .buffer_unordered(5)
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .flatten()
        .collect();

    issues.sort_by_key(|issue| issue.number);

    Ok(issues)
}
