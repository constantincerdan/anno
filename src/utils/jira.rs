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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_from_branch_name() {
        let keys = extract_issue_keys(&["feature/PROJ-123-add-thing"], &[], &[]);
        assert_eq!(keys, HashSet::from(["PROJ-123".to_string()]));
    }

    #[test]
    fn extract_from_body() {
        let keys = extract_issue_keys(&[], &["Fixes PROJ-123 and PROJ-456"], &[]);
        assert_eq!(
            keys,
            HashSet::from(["PROJ-123".to_string(), "PROJ-456".to_string()])
        );
    }

    #[test]
    fn extract_from_commit_messages() {
        let messages = vec!["PROJ-789 fix bug".to_string()];
        let keys = extract_issue_keys(&[], &[], &messages);
        assert_eq!(keys, HashSet::from(["PROJ-789".to_string()]));
    }

    #[test]
    fn uppercases_keys() {
        let keys = extract_issue_keys(&["feature/proj-123"], &[], &[]);
        assert_eq!(keys, HashSet::from(["PROJ-123".to_string()]));
    }

    #[test]
    fn deduplicates_across_sources() {
        let messages = vec!["PROJ-123 fix".to_string()];
        let keys = extract_issue_keys(&["feature/PROJ-123"], &["See PROJ-123"], &messages);
        assert_eq!(keys, HashSet::from(["PROJ-123".to_string()]));
    }

    #[test]
    fn ignores_short_prefixes() {
        // Regex requires 2-10 char prefix, so single-char prefix should not match
        let keys = extract_issue_keys(&["feature/A-123"], &[], &[]);
        assert!(keys.is_empty());
    }

    #[test]
    fn no_match() {
        let keys = extract_issue_keys(&["feature/no-ticket"], &["no tickets here"], &[]);
        assert!(keys.is_empty());
    }

    #[test]
    fn branch_extracts_only_first_match() {
        // Branch extraction uses find (first match only), not find_iter
        let keys = extract_issue_keys(&["feature/PROJ-123-PROJ-456"], &[], &[]);
        assert_eq!(keys, HashSet::from(["PROJ-123".to_string()]));
    }

}
