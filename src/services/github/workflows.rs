use crate::services::github::repository::{RepoFile, Repository};
use crate::utils::{config, http};
use anyhow::Result;
use base64::prelude::*;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct WorkflowRuns {
    workflow_runs: Vec<WorkflowRun>,
}

impl WorkflowRuns {
    pub async fn get_prev_runs_with_last_success_for_branch(
        run: &WorkflowRun,
    ) -> Result<Option<PrevRuns>> {
        tracing::info!(
            "Fetching previous and last successful workflow runs for {} branch",
            run.head_branch
        );

        let mut all_prev_runs: Vec<WorkflowRun> = Vec::new();
        let mut page = 1;
        loop {
            let prev_runs = Self::get_prev_runs(run, true, page).await?.workflow_runs;

            if prev_runs.is_empty() {
                return Ok(None);
            }

            for prev_run in prev_runs {
                if prev_run.path != run.path {
                    continue;
                }

                if prev_run.has_successful_attempt().await? {
                    return Ok(Some(PrevRuns {
                        last_successful: prev_run,
                        prev_runs: all_prev_runs,
                    }));
                }

                all_prev_runs.push(prev_run);
            }

            page += 1;
        }
    }

    pub async fn get_prev_successful_run(run: &WorkflowRun) -> Result<Option<WorkflowRun>> {
        tracing::info!("Fetching last successful run for workflow");

        let mut page = 1;
        loop {
            let prev_runs = Self::get_prev_runs(run, false, page).await?.workflow_runs;

            if prev_runs.is_empty() {
                return Ok(None);
            }

            for prev_run in prev_runs {
                if prev_run.path != run.path {
                    continue;
                }

                if prev_run.has_successful_attempt().await? {
                    return Ok(Some(prev_run));
                }
            }

            page += 1;
        }
    }

    async fn get_prev_runs(run: &WorkflowRun, for_run_branch: bool, page: u8) -> Result<Self> {
        let gh_base_url = config::get("GITHUB_BASE_URL")?;
        let gh_token = config::get("GITHUB_TOKEN")?;

        let url = format!(
            "{}/repos/{}/actions/runs",
            gh_base_url, run.repository.full_name
        );

        let created = format!("<{}", run.created_at);
        let page_str = page.to_string();
        let branch = run.head_branch.clone();

        let runs = http::send_with_retry(|| {
            let mut req = http::client()
                .get(&url)
                .bearer_auth(&gh_token)
                .header("Accept", "application/json")
                .header("User-Agent", "Anno")
                .query(&[("created", &created), ("page", &page_str)]);

            if for_run_branch {
                req = req.query(&[("branch", &branch)]);
            }

            req
        })
        .await?
        .error_for_status()
        .inspect_err(|e| tracing::error!("Error getting previous workflow runs: {e}"))?
        .json::<Self>()
        .await?;

        Ok(runs)
    }
}

#[derive(Deserialize)]
pub struct WorkflowRepo {
    url: String,
    full_name: String,
}

pub struct PrevRuns {
    pub last_successful: WorkflowRun,
    pub prev_runs: Vec<WorkflowRun>,
}

#[derive(Deserialize)]
pub struct WorkflowRun {
    pub head_sha: String,
    pub head_branch: String,
    pub repository: WorkflowRepo,
    pub actor: WorkflowRunActor,
    pub path: String,
    created_at: String,
    conclusion: Option<String>,
    html_url: String,
    previous_attempt_url: Option<String>,
}

impl WorkflowRun {
    pub async fn get_by_id(repo_name: &str, run_id: &str) -> Result<Self> {
        tracing::info!("Fetching workflow run {run_id}");

        let gh_token = config::get("GITHUB_TOKEN")?;
        let url = format!("https://api.github.com/repos/{repo_name}/actions/runs/{run_id}");

        let workflow_run = http::send_with_retry(|| {
            http::client()
                .get(&url)
                .bearer_auth(&gh_token)
                .header("Accept", "application/json")
                .header("User-Agent", "Anno")
        })
        .await?
        .error_for_status()
        .inspect_err(|e| tracing::error!("Error getting workflow run: {e}"))?
        .json::<Self>()
        .await?;

        Ok(workflow_run)
    }

    pub fn is_successful_attempt(&self) -> bool {
        self.conclusion.as_ref().is_some_and(|c| c == "success")
    }

    pub async fn has_successful_attempt(&self) -> Result<bool> {
        Ok(self.is_successful_attempt() || self.get_prev_successful_attempt().await?.is_some())
    }

    pub async fn has_prev_successful_attempt(&self) -> Result<bool> {
        Ok(self.get_prev_successful_attempt().await?.is_some())
    }

    async fn get_prev_successful_attempt(&self) -> Result<Option<WorkflowRun>> {
        let mut possible_prev_attempt = self.get_prev_attempt().await?;

        loop {
            let Some(prev_attempt) = possible_prev_attempt else {
                break;
            };

            if prev_attempt.is_successful_attempt() {
                return Ok(Some(prev_attempt));
            }

            possible_prev_attempt = prev_attempt.get_prev_attempt().await?;
        }

        Ok(None)
    }

    async fn get_prev_attempt(&self) -> Result<Option<WorkflowRun>> {
        let gh_token = config::get("GITHUB_TOKEN")?;

        let Some(prev_attempt_url) = &self.previous_attempt_url else {
            return Ok(None);
        };

        let workflow_run = http::send_with_retry(|| {
            http::client()
                .get(prev_attempt_url)
                .bearer_auth(&gh_token)
                .header("Accept", "application/json")
                .header("User-Agent", "Anno")
        })
        .await?
        .error_for_status()
        .inspect_err(|e| tracing::error!("Error getting previous workflow attempt: {e}"))?
        .json::<WorkflowRun>()
        .await?;

        Ok(Some(workflow_run))
    }

    pub fn get_run_url(&self) -> &str {
        &self.html_url
    }

    pub async fn get_repo(&self) -> Result<Repository> {
        tracing::info!("Fetching workflow repository");

        let gh_token = config::get("GITHUB_TOKEN")?;

        let repo = http::send_with_retry(|| {
            http::client()
                .get(&self.repository.url)
                .bearer_auth(&gh_token)
                .header("Accept", "application/json")
                .header("User-Agent", "Anno")
        })
        .await?
        .error_for_status()
        .inspect_err(|e| tracing::error!("Error getting workflow run: {e}"))?
        .json::<Repository>()
        .await?;

        Ok(repo)
    }
}

#[derive(Deserialize)]
pub struct WorkflowConfig {
    pub on: Option<WorkflowOnConfig>,
}

impl WorkflowConfig {
    pub fn from_file(file: RepoFile) -> Result<Self> {
        let decoded_config = BASE64_STANDARD.decode(file.content.replace('\n', ""))?;
        let config_content = String::from_utf8(decoded_config)?;
        let config = serde_yaml::from_str(&config_content)?;

        Ok(config)
    }

    pub fn push_config(&self) -> Option<&WorkflowOnPushConfig> {
        self.on.as_ref()?.push.as_ref()
    }
}

#[derive(Deserialize)]
pub struct WorkflowOnConfig {
    pub push: Option<WorkflowOnPushConfig>,
}

#[derive(Deserialize)]
pub struct WorkflowOnPushConfig {
    pub paths: Option<Vec<String>>,
    #[serde(rename = "paths-ignore")]
    pub paths_ignore: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct WorkflowRunActor {
    pub login: String,
    pub avatar_url: String,
}
