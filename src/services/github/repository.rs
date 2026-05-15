use super::GitHubClient;
use super::pull_request::PullRequest;
use crate::utils::http;
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
pub struct Repository {
    pub full_name: String,
    pub name: String,
    pub html_url: String,
    compare_url: String,
    contents_url: String,
    commits_url: String,
    pub default_branch: String,
}

impl Repository {
    pub fn get_compare_url(&self, old_sha: &str, new_sha: &str) -> String {
        format!(
            "https://github.com/{}/compare/{}...{}",
            self.full_name, old_sha, new_sha
        )
    }

    pub fn get_commit_url(&self, sha: &str) -> String {
        format!("{}/commit/{sha}", self.html_url)
    }

    pub async fn get_pull_requests_for_commit(
        &self,
        gh: &GitHubClient,
        sha: &str,
    ) -> Result<Vec<PullRequest>> {
        tracing::info!("Fetching associated pull requests for commit {sha}");

        let url = self.commits_url.replace("{/sha}", &format!("/{sha}/pulls"));

        let response = http::send_with_retry(|| gh.get(&url).header("Accept", "application/json"))
            .await?
            .error_for_status()
            .inspect_err(|e| tracing::error!("Error getting associated PRs: {e}"))?
            .json::<Vec<PullRequest>>()
            .await?;

        Ok(response)
    }

    pub async fn get_file(&self, gh: &GitHubClient, path: &str) -> Result<RepoFile> {
        tracing::info!("Fetching file {path}");

        let url = self.contents_url.replace("{+path}", path);

        let response = http::send_with_retry(|| gh.get(&url).header("Accept", "application/json"))
            .await?
            .error_for_status()
            .inspect_err(|e| tracing::error!("Error getting repo file: {e}"))?
            .json::<RepoFile>()
            .await?;

        Ok(response)
    }

    pub async fn get_diff_for_commit(&self, gh: &GitHubClient, sha: &str) -> Result<String> {
        tracing::info!("Fetching diff for commit {sha}");

        let url = self.commits_url.replace("{/sha}", &format!("/{sha}"));

        let diff =
            http::send_with_retry(|| gh.get(&url).header("Accept", "application/vnd.github.diff"))
                .await?
                .error_for_status()
                .inspect_err(|e| tracing::error!("Error getting commit diff: {e}"))?
                .text()
                .await?;

        Ok(diff)
    }

    pub async fn get_commit_message(&self, gh: &GitHubClient, sha: &str) -> Result<String> {
        tracing::info!("Fetching commit message for commit {sha}");

        let url = self.commits_url.replace("{/sha}", &format!("/{sha}"));

        let message = http::send_with_retry(|| gh.get(&url).header("Accept", "application/json"))
            .await?
            .error_for_status()
            .inspect_err(|e| tracing::error!("Error getting commit message: {e}"))?
            .json::<Commit>()
            .await?
            .commit
            .message;

        Ok(message)
    }

    pub async fn get_diff_between_commits(
        &self,
        gh: &GitHubClient,
        old_sha: &str,
        new_sha: &str,
    ) -> Result<String> {
        tracing::info!("Fetching diff between commits {old_sha} and {new_sha}");

        let url = self
            .compare_url
            .replace("{base}...{head}", &format!("{old_sha}...{new_sha}"));

        let diff =
            http::send_with_retry(|| gh.get(&url).header("Accept", "application/vnd.github.diff"))
                .await?
                .error_for_status()
                .inspect_err(|e| tracing::error!("Error fetching repo diff: {e}"))?
                .text()
                .await?;

        Ok(diff)
    }

    pub async fn get_contributors_between_commits(
        &self,
        gh: &GitHubClient,
        old_sha: &str,
        new_sha: &str,
    ) -> Result<Vec<CommitAuthor>> {
        tracing::info!("Fetching contributors between {old_sha} and {new_sha}");

        let url = self
            .compare_url
            .replace("{base}...{head}", &format!("{old_sha}...{new_sha}"));

        let response = http::send_with_retry(|| gh.get(&url).header("Accept", "application/json"))
            .await?
            .error_for_status()
            .inspect_err(|e| tracing::error!("Error fetching compare data: {e}"))?
            .json::<CompareResponse>()
            .await?;

        Ok(unique_contributors(
            response.commits.into_iter().filter_map(|c| c.author),
        ))
    }

    pub async fn get_commit_contributors(
        &self,
        gh: &GitHubClient,
        sha: &str,
    ) -> Result<Vec<CommitAuthor>> {
        tracing::info!("Fetching contributor for commit {sha}");

        let url = self.commits_url.replace("{/sha}", &format!("/{sha}"));

        let commit = http::send_with_retry(|| gh.get(&url).header("Accept", "application/json"))
            .await?
            .error_for_status()
            .inspect_err(|e| tracing::error!("Error fetching commit: {e}"))?
            .json::<Commit>()
            .await?;

        Ok(commit.author.into_iter().collect())
    }

    pub fn get_compare_to_default_branch_url(&self, commit: &str) -> String {
        format!(
            "{}/compare/{}...{}",
            self.html_url, commit, self.default_branch
        )
    }
}

#[derive(Deserialize)]
pub struct RepoFile {
    pub content: String,
}

#[derive(Deserialize)]
pub struct Commit {
    pub commit: CommitDetails,
    pub author: Option<CommitAuthor>,
}

#[derive(Deserialize)]
pub struct CommitDetails {
    pub message: String,
}

#[derive(Deserialize)]
pub struct CommitAuthor {
    pub login: Option<String>,
    pub avatar_url: String,
}

#[derive(Deserialize)]
struct CompareResponse {
    commits: Vec<Commit>,
}

fn unique_contributors(authors: impl Iterator<Item = CommitAuthor>) -> Vec<CommitAuthor> {
    let mut seen = HashSet::new();
    authors.filter(|a| seen.insert(a.login.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_repo() -> Repository {
        serde_json::from_value(json!({
            "full_name": "owner/repo",
            "name": "repo",
            "html_url": "https://github.com/owner/repo",
            "compare_url": "https://api.github.com/repos/owner/repo/compare/{base}...{head}",
            "contents_url": "https://api.github.com/repos/owner/repo/contents/{+path}",
            "commits_url": "https://api.github.com/repos/owner/repo/commits{/sha}",
            "default_branch": "main"
        }))
        .unwrap()
    }

    fn make_author(login: Option<&str>) -> CommitAuthor {
        CommitAuthor {
            login: match login {
                Some(l) => Some(l.to_string()),
                None => None,
            },
            avatar_url: format!(
                "https://avatars.githubusercontent.com/{login}",
                login = login.unwrap_or("unknown")
            ),
        }
    }

    #[test]
    fn compare_url() {
        let repo = make_repo();
        assert_eq!(
            repo.get_compare_url("abc123", "def456"),
            "https://github.com/owner/repo/compare/abc123...def456"
        );
    }

    #[test]
    fn commit_url() {
        let repo = make_repo();
        assert_eq!(
            repo.get_commit_url("abc123"),
            "https://github.com/owner/repo/commit/abc123"
        );
    }

    #[test]
    fn compare_to_default_branch_url() {
        let repo = make_repo();
        assert_eq!(
            repo.get_compare_to_default_branch_url("abc123"),
            "https://github.com/owner/repo/compare/abc123...main"
        );
    }

    #[test]
    fn unique_contributors_deduplicates() {
        let authors = vec![
            make_author(Some("alice")),
            make_author(Some("bob")),
            make_author(None),
            make_author(Some("alice")),
            make_author(Some("charlie")),
            make_author(None),
            make_author(Some("bob")),
        ];
        let result = unique_contributors(authors.into_iter());
        let logins: Vec<_> = result
            .iter()
            .map(|a| a.login.as_deref().unwrap_or("unknown"))
            .collect();
        assert_eq!(logins, vec!["alice", "bob", "unknown", "charlie"]);
    }

    #[test]
    fn unique_contributors_preserves_order() {
        let authors = vec![
            make_author(None),
            make_author(Some("charlie")),
            make_author(Some("alice")),
            make_author(Some("charlie")),
        ];
        let result = unique_contributors(authors.into_iter());
        let logins: Vec<_> = result
            .iter()
            .map(|a| a.login.as_deref().unwrap_or("unknown"))
            .collect();
        assert_eq!(logins, vec!["unknown", "charlie", "alice"]);
    }

    #[test]
    fn unique_contributors_empty() {
        let result = unique_contributors(std::iter::empty());
        assert!(result.is_empty());
    }
}
