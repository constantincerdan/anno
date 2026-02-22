use regex_lite::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

static ISSUE_NUMBER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#(\d+)").unwrap());

pub fn extract_issue_numbers(bodies: &[&str], commit_messages: &[String]) -> HashSet<u64> {
    let mut numbers = HashSet::new();

    for body in bodies {
        for cap in ISSUE_NUMBER_RE.captures_iter(body) {
            if let Some(num) = cap.get(1).and_then(|m| m.as_str().parse().ok()) {
                numbers.insert(num);
            }
        }
    }

    for message in commit_messages {
        for cap in ISSUE_NUMBER_RE.captures_iter(message) {
            if let Some(num) = cap.get(1).and_then(|m| m.as_str().parse().ok()) {
                numbers.insert(num);
            }
        }
    }

    numbers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_from_body_closes_syntax() {
        let numbers = extract_issue_numbers(&["closes #1234"], &[]);
        assert_eq!(numbers, HashSet::from([1234]));
    }

    #[test]
    fn extract_from_body_fixes_syntax() {
        let numbers = extract_issue_numbers(&["Fixes #42 and resolves #99"], &[]);
        assert_eq!(numbers, HashSet::from([42, 99]));
    }

    #[test]
    fn extract_from_commit_messages() {
        let messages = vec!["fix: resolve issue #567".to_string()];
        let numbers = extract_issue_numbers(&[], &messages);
        assert_eq!(numbers, HashSet::from([567]));
    }

    #[test]
    fn deduplicates_across_sources() {
        let messages = vec!["closes #123".to_string()];
        let numbers = extract_issue_numbers(&["See #123"], &messages);
        assert_eq!(numbers, HashSet::from([123]));
    }

    #[test]
    fn no_match() {
        let numbers = extract_issue_numbers(&["no issues here"], &[]);
        assert!(numbers.is_empty());
    }

    #[test]
    fn multiple_issues_in_single_body() {
        let numbers = extract_issue_numbers(&["closes #1, fixes #2, resolves #3"], &[]);
        assert_eq!(numbers, HashSet::from([1, 2, 3]));
    }
}
