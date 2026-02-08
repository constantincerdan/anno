use regex_lite::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

static JIRA_KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b([A-Z]{2,10}-\d+)\b").unwrap());

/// Extract unique Jira issue keys from PR branches, bodies, and commit messages.
pub fn extract_issue_keys(
    branches: &[&str],
    bodies: &[&str],
    commit_messages: &[String],
) -> HashSet<String> {
    let mut keys = HashSet::new();

    for branch in branches {
        if let Some(m) = JIRA_KEY_RE.find(branch) {
            keys.insert(m.as_str().to_uppercase());
        }
    }

    for body in bodies {
        for m in JIRA_KEY_RE.find_iter(body) {
            keys.insert(m.as_str().to_uppercase());
        }
    }

    for message in commit_messages {
        for m in JIRA_KEY_RE.find_iter(message) {
            keys.insert(m.as_str().to_uppercase());
        }
    }

    keys
}
