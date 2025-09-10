use crate::services::github::{IGNORED_REPO_PATHS, workflows::WorkflowConfig};
use crate::utils::config;
use glob::Pattern;
use regex_lite::Regex;

#[derive(Debug, Default)]
pub struct TargetPaths {
    included: Vec<Pattern>,
    excluded: Vec<Pattern>,
}

impl TargetPaths {
    pub fn new(workflow_config: WorkflowConfig) -> Self {
        if let Some(target_paths) = Self::get_paths_from_action_input() {
            let (included, excluded) = Self::split_paths(&target_paths);

            return Self {
                included: Self::create_patterns(included),
                excluded: Self::create_patterns(excluded),
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
            included: Self::create_patterns(included),
            excluded: Self::create_patterns(excluded),
        }
    }

    pub fn filter_diff(&self, diff: &str) -> String {
        let re = Regex::new(r"b/([^ ]+)").unwrap();

        let mut is_inside_ignored_file = false;
        diff.lines()
            .filter(|line| {
                if line.starts_with("diff --git") {
                    if let Some(caps) = re.captures(line) {
                        let path = caps[1].to_string();

                        let is_ignored_file = IGNORED_REPO_PATHS.iter().any(|p| path.contains(p));
                        let is_non_target_file = !self.is_path_included(&path);

                        is_inside_ignored_file = is_ignored_file || is_non_target_file;
                    }
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
        let target_paths = config::get_optional("PATHS");

        let target_paths = target_paths?
            .split([',', '\n'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Some(target_paths)
    }

    fn split_paths(paths: &[String]) -> (Vec<&String>, Vec<&String>) {
        paths
            .iter()
            .filter(|p| IGNORED_REPO_PATHS.iter().all(|i| !p.contains(i)))
            .partition::<Vec<_>, _>(|p| !p.starts_with('!'))
    }

    fn create_patterns(paths: Vec<&String>) -> Vec<Pattern> {
        paths
            .iter()
            .map(|p| Pattern::new(p.strip_prefix('!').unwrap_or(p)))
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_default()
    }
}
