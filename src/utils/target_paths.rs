use crate::services::github::{IGNORED_REPO_PATHS, workflows::WorkflowConfig};
use crate::utils::env;
use glob::Pattern;
use regex_lite::Regex;
use std::sync::LazyLock;

static DIFF_PATH_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"b/([^ ]+)").unwrap());

#[derive(Debug, Default)]
pub struct TargetPaths {
    included: Vec<Pattern>,
    excluded: Vec<Pattern>,
}

impl TargetPaths {
    pub fn new(workflow_config: &WorkflowConfig) -> Self {
        if let Some(target_paths) = Self::get_paths_from_action_input() {
            let (included, excluded) = Self::split_paths(&target_paths);

            return Self {
                included: Self::create_patterns(&included),
                excluded: Self::create_patterns(&excluded),
            };
        }

        let Some(push_config) = workflow_config.push_config() else {
            return Self::default();
        };

        let paths = push_config.paths.as_deref().unwrap_or_default();
        let ignored_paths = push_config.paths_ignore.as_deref().unwrap_or_default();

        if paths.is_empty() && ignored_paths.is_empty() {
            return Self::default();
        }

        let (included, mut excluded) = Self::split_paths(paths);

        for path in ignored_paths {
            excluded.push(path);
        }

        Self {
            included: Self::create_patterns(&included),
            excluded: Self::create_patterns(&excluded),
        }
    }

    pub fn filter_diff(&self, diff: &str) -> String {
        let mut is_inside_ignored_file = false;
        diff.lines()
            .filter(|line| {
                if line.starts_with("diff --git")
                    && let Some(caps) = DIFF_PATH_RE.captures(line)
                {
                    let path = caps[1].to_string();

                    let is_ignored_file = IGNORED_REPO_PATHS.iter().any(|p| path.contains(p));
                    let is_non_target_file = !self.is_path_included(&path);

                    is_inside_ignored_file = is_ignored_file || is_non_target_file;
                }

                !is_inside_ignored_file
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn is_path_included(&self, path: &str) -> bool {
        let is_included = self.included.is_empty() || self.included.iter().any(|p| p.matches(path));
        let is_excluded = self.excluded.iter().any(|p| p.matches(path));

        is_included && !is_excluded
    }

    fn get_paths_from_action_input() -> Option<Vec<String>> {
        let target_paths = env::get_optional("PATHS");

        let target_paths = target_paths?
            .split([',', '\n'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Some(target_paths)
    }

    fn split_paths(paths: &[String]) -> (Vec<&str>, Vec<&str>) {
        paths
            .iter()
            .map(String::as_str)
            .filter(|p| IGNORED_REPO_PATHS.iter().all(|i| !p.contains(i)))
            .partition::<Vec<_>, _>(|p| !p.starts_with('!'))
    }

    fn create_patterns(paths: &[&str]) -> Vec<Pattern> {
        paths
            .iter()
            .map(|p| Pattern::new(p.strip_prefix('!').unwrap_or(p)))
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target_paths(included: &[&str], excluded: &[&str]) -> TargetPaths {
        TargetPaths {
            included: included.iter().map(|p| Pattern::new(p).unwrap()).collect(),
            excluded: excluded.iter().map(|p| Pattern::new(p).unwrap()).collect(),
        }
    }

    #[test]
    fn default_includes_everything() {
        let tp = TargetPaths::default();
        assert!(tp.is_path_included("src/main.rs"));
        assert!(tp.is_path_included("anything/at/all"));
    }

    #[test]
    fn included_patterns_filter() {
        let tp = target_paths(&["src/**"], &[]);
        assert!(tp.is_path_included("src/main.rs"));
        assert!(tp.is_path_included("src/utils/config.rs"));
        assert!(!tp.is_path_included("tests/test.rs"));
    }

    #[test]
    fn excluded_patterns_filter() {
        let tp = target_paths(&[], &["*.lock"]);
        assert!(tp.is_path_included("src/main.rs"));
        assert!(!tp.is_path_included("Cargo.lock"));
    }

    #[test]
    fn included_and_excluded() {
        let tp = target_paths(&["src/**"], &["src/generated/**"]);
        assert!(tp.is_path_included("src/main.rs"));
        assert!(!tp.is_path_included("src/generated/types.rs"));
        assert!(!tp.is_path_included("tests/test.rs"));
    }

    #[test]
    fn filter_diff_removes_ignored_paths() {
        let tp = TargetPaths::default();
        let diff = "diff --git a/src/main.rs b/src/main.rs\n\
                     +code\n\
                     diff --git a/node_modules/foo b/node_modules/foo\n\
                     +ignored\n\
                     diff --git a/src/lib.rs b/src/lib.rs\n\
                     +more code";

        let filtered = tp.filter_diff(diff);
        assert!(filtered.contains("src/main.rs"));
        assert!(filtered.contains("src/lib.rs"));
        assert!(!filtered.contains("node_modules"));
    }

    #[test]
    fn filter_diff_removes_non_target_paths() {
        let tp = target_paths(&["src/**"], &[]);
        let diff = "diff --git a/src/main.rs b/src/main.rs\n\
                     +code\n\
                     diff --git a/tests/test.rs b/tests/test.rs\n\
                     +test code\n\
                     diff --git a/src/lib.rs b/src/lib.rs\n\
                     +more code";

        let filtered = tp.filter_diff(diff);
        assert!(filtered.contains("src/main.rs"));
        assert!(filtered.contains("src/lib.rs"));
        assert!(!filtered.contains("tests/test.rs"));
    }

    #[test]
    fn filter_diff_empty_input() {
        let tp = TargetPaths::default();
        assert_eq!(tp.filter_diff(""), "");
    }

    #[test]
    fn split_paths_separates_include_and_exclude() {
        let paths = vec![
            "src/**".to_string(),
            "!tests/**".to_string(),
            "lib/**".to_string(),
        ];
        let (included, excluded) = TargetPaths::split_paths(&paths);
        assert_eq!(included, vec!["src/**", "lib/**"]);
        assert_eq!(excluded, vec!["!tests/**"]);
    }

    #[test]
    fn split_paths_filters_ignored_repo_paths() {
        let paths = vec!["src/**".to_string(), "node_modules/**".to_string()];
        let (included, excluded) = TargetPaths::split_paths(&paths);
        assert_eq!(included, vec!["src/**"]);
        assert!(excluded.is_empty());
    }
}
