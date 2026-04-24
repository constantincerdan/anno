use super::GitHubClient;
use super::repository::{RepoFile, Repository};
use crate::utils::http;
use anyhow::Result;
use base64::prelude::*;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct WorkflowRuns {
    workflow_runs: Vec<WorkflowRun>,
}

impl WorkflowRuns {
    pub async fn get_prev_runs_with_last_success_for_branch(
        gh: &GitHubClient,
        run: &WorkflowRun,
    ) -> Result<Option<PrevRuns>> {
        tracing::info!(
            "Fetching previous and last successful workflow runs for {} branch",
            run.head_branch
        );

        let mut all_prev_runs: Vec<WorkflowRun> = Vec::new();
        let mut page = 1;
        loop {
            let prev_runs = Self::get_prev_runs(gh, run, true, page)
                .await?
                .workflow_runs;

            if prev_runs.is_empty() {
                return Ok(None);
            }

            for prev_run in prev_runs {
                if prev_run.path != run.path {
                    continue;
                }

                if prev_run.has_successful_attempt(gh).await? {
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

    pub async fn get_prev_successful_run(
        gh: &GitHubClient,
        run: &WorkflowRun,
    ) -> Result<Option<WorkflowRun>> {
        tracing::info!("Fetching last successful run for workflow");

        let mut page = 1;
        loop {
            let prev_runs = Self::get_prev_runs(gh, run, false, page)
                .await?
                .workflow_runs;

            if prev_runs.is_empty() {
                return Ok(None);
            }

            for prev_run in prev_runs {
                if prev_run.path != run.path {
                    continue;
                }

                if prev_run.has_successful_attempt(gh).await? {
                    return Ok(Some(prev_run));
                }
            }

            page += 1;
        }
    }

    async fn get_prev_runs(
        gh: &GitHubClient,
        run: &WorkflowRun,
        for_run_branch: bool,
        page: u8,
    ) -> Result<Self> {
        let url = format!(
            "{}/repos/{}/actions/runs",
            gh.base_url(),
            run.repository.full_name
        );

        let created = format!("<{}", run.created_at);
        let page_str = page.to_string();
        let branch = run.head_branch.clone();

        let runs = http::send_with_retry(|| {
            let mut req = gh
                .get(&url)
                .header("Accept", "application/json")
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
    pub path: String,
    created_at: String,
    conclusion: Option<String>,
    html_url: String,
    previous_attempt_url: Option<String>,
}

impl WorkflowRun {
    pub async fn get_by_id(gh: &GitHubClient, repo_name: &str, run_id: &str) -> Result<Self> {
        tracing::info!("Fetching workflow run {run_id}");

        let url = format!("https://api.github.com/repos/{repo_name}/actions/runs/{run_id}");

        let workflow_run =
            http::send_with_retry(|| gh.get(&url).header("Accept", "application/json"))
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

    pub async fn has_successful_attempt(&self, gh: &GitHubClient) -> Result<bool> {
        Ok(self.is_successful_attempt() || self.get_prev_successful_attempt(gh).await?.is_some())
    }

    pub async fn has_prev_successful_attempt(&self, gh: &GitHubClient) -> Result<bool> {
        Ok(self.get_prev_successful_attempt(gh).await?.is_some())
    }

    async fn get_prev_successful_attempt(&self, gh: &GitHubClient) -> Result<Option<WorkflowRun>> {
        let mut possible_prev_attempt = self.get_prev_attempt(gh).await?;

        while let Some(prev_attempt) = possible_prev_attempt {
            if prev_attempt.is_successful_attempt() {
                return Ok(Some(prev_attempt));
            }

            possible_prev_attempt = prev_attempt.get_prev_attempt(gh).await?;
        }

        Ok(None)
    }

    async fn get_prev_attempt(&self, gh: &GitHubClient) -> Result<Option<WorkflowRun>> {
        let Some(prev_attempt_url) = &self.previous_attempt_url else {
            return Ok(None);
        };

        let workflow_run = http::send_with_retry(|| {
            gh.get(prev_attempt_url)
                .header("Accept", "application/json")
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

    pub async fn get_repo(&self, gh: &GitHubClient) -> Result<Repository> {
        tracing::info!("Fetching workflow repository");

        let repo = http::send_with_retry(|| {
            gh.get(&self.repository.url)
                .header("Accept", "application/json")
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
    pub fn from_file(file: &RepoFile) -> Result<Self> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_run(conclusion: Option<&str>) -> WorkflowRun {
        serde_json::from_value(json!({
            "head_sha": "abc123",
            "head_branch": "main",
            "repository": { "url": "https://api.github.com/repos/owner/repo", "full_name": "owner/repo" },
            "path": ".github/workflows/deploy.yml",
            "created_at": "2024-01-01T00:00:00Z",
            "conclusion": conclusion,
            "html_url": "https://github.com/owner/repo/actions/runs/123",
            "previous_attempt_url": null
        }))
        .unwrap()
    }

    #[test]
    fn is_successful_with_success_conclusion() {
        let run = make_run(Some("success"));
        assert!(run.is_successful_attempt());
    }

    #[test]
    fn is_not_successful_with_failure_conclusion() {
        let run = make_run(Some("failure"));
        assert!(!run.is_successful_attempt());
    }

    #[test]
    fn is_not_successful_with_no_conclusion() {
        let run = make_run(None);
        assert!(!run.is_successful_attempt());
    }

    #[test]
    fn workflow_config_from_file_with_paths() {
        let yaml = "on:\n  push:\n    paths:\n      - src/**\n    paths-ignore:\n      - docs/**\n";
        let encoded = BASE64_STANDARD.encode(yaml);
        let file = RepoFile { content: encoded };

        let config = WorkflowConfig::from_file(&file).unwrap();
        let push = config.push_config().unwrap();
        assert_eq!(push.paths.as_deref().unwrap(), &["src/**"]);
        assert_eq!(push.paths_ignore.as_deref().unwrap(), &["docs/**"]);
    }

    #[test]
    fn workflow_config_from_file_without_push() {
        let yaml = "on:\n  pull_request:\n    branches:\n      - main\n";
        let encoded = BASE64_STANDARD.encode(yaml);
        let file = RepoFile { content: encoded };

        let config = WorkflowConfig::from_file(&file).unwrap();
        assert!(config.push_config().is_none());
    }

    #[test]
    fn workflow_config_from_file_without_paths() {
        let yaml = "on:\n  push:\n    branches:\n      - main\n";
        let encoded = BASE64_STANDARD.encode(yaml);
        let file = RepoFile { content: encoded };

        let config = WorkflowConfig::from_file(&file).unwrap();
        let push = config.push_config().unwrap();
        assert!(push.paths.is_none());
        assert!(push.paths_ignore.is_none());
    }
}
