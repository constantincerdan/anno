use crate::utils::{config, target_paths::TargetPaths};
use anyhow::Result;
use git2::{Commit, Oid, build::RepoBuilder};
use std::path::Path;

pub struct Git {
    repo: git2::Repository,
}

impl Git {
    pub fn init(full_name: &str) -> Result<Self> {
        let repos_dir = config::get("REPOS_DIR")?;
        let gh_token = config::get("GITHUB_TOKEN")?;

        let repo_name = full_name.split('/').next_back().unwrap_or(full_name);
        let repo_url = format!("https://github.com/{full_name}");
        let repo_disk_path = format!("{repos_dir}/{}", repo_name.replace('-', "_"));

        let repo = if let Ok(repo) = git2::Repository::open(&repo_disk_path) {
            tracing::info!("{full_name} repository already cloned, pulling latest changes");

            repo.find_remote("origin")?.fetch(
                &[] as &[&str],
                Some(&mut Self::get_fetch_options(&gh_token)),
                None,
            )?;

            repo
        } else {
            tracing::info!("Cloning {full_name} repository");
            RepoBuilder::new()
                .fetch_options(Self::get_fetch_options(&gh_token))
                .clone(&repo_url, Path::new(&repo_disk_path))?
        };

        Ok(Self { repo })
    }

    fn get_fetch_options(gh_token: &str) -> git2::FetchOptions<'_> {
        let mut callbacks = git2::RemoteCallbacks::new();

        callbacks.credentials(move |_url, _username_from_url, _allowed_types| {
            git2::Cred::userpass_plaintext("x-access-token", gh_token)
        });

        let mut fetch_options = git2::FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);

        fetch_options
    }

    pub fn get_commit_messages(
        &self,
        start_commit: &str,
        end_commit: &str,
        target_paths: &TargetPaths,
    ) -> Result<Vec<String>> {
        tracing::info!("Getting commit messages between commits {start_commit} and {end_commit}");

        let mut revwalk = self.repo.revwalk()?;
        revwalk.set_sorting(git2::Sort::TIME)?;

        let commit_range = &format!("{start_commit}..{end_commit}");
        revwalk.push_range(commit_range)?;

        let mut messages = Vec::new();
        for oid in revwalk {
            if let Some(message) = self.get_commit_message(oid?, target_paths)? {
                messages.push(message);
            }
        }

        Ok(messages)
    }

    fn get_commit_message(
        &self,
        commit: Oid,
        target_paths: &TargetPaths,
    ) -> Result<Option<String>> {
        let commit = self.repo.find_commit(commit)?;

        let affected_files = self.get_affected_files(&commit)?;

        if !affected_files
            .iter()
            .any(|p| target_paths.is_path_included(p))
        {
            return Ok(None);
        }

        let message = commit.message().unwrap_or_default().to_string();

        Ok(Some(message))
    }

    fn get_affected_files(&self, commit: &Commit) -> Result<Vec<String>> {
        let tree = commit.tree()?;
        let parent_tree = if commit.parent_count() > 0 {
            Some(commit.parent(0)?.tree()?)
        } else {
            None
        };

        let diff = self
            .repo
            .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;

        let mut file_paths = Vec::new();
        diff.foreach(
            &mut |delta, _| {
                let file_path = delta
                    .old_file()
                    .path()
                    .map(|p| p.to_string_lossy().into_owned());

                file_paths.push(file_path.unwrap());

                true
            },
            None,
            None,
            None,
        )?;

        Ok(file_paths)
    }
}
