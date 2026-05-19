use crate::ai::{AiError, AiProvider, AiRequest};
use crate::services::github::PullRequest;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;

const MAX_DIFF_CHARS: usize = 400_000;
const MAX_PR_BODY_BYTES: usize = 500;

#[derive(Deserialize, Serialize, Debug)]
pub struct ReleaseSummary {
    pub items: Vec<SummaryCategory>,
    #[serde(skip)]
    pub fallback_message: Option<String>,
}

impl ReleaseSummary {
    fn sanitize(mut summary: Self) -> Self {
        for category in &mut summary.items {
            for item in &mut category.items {
                *item = item
                    .trim_start_matches(['•', '-', '*', ':', ' '])
                    .to_string();
            }
            category
                .items
                .retain(|item| item.len() > 3 && item.chars().any(char::is_alphanumeric));
        }
        summary.items.retain(|category| !category.items.is_empty());
        summary
    }

    pub async fn new(
        provider: AiProvider,
        diff: &str,
        commit_messages: &[String],
        pull_requests: &[PrContext<'_>],
    ) -> Result<Self> {
        tracing::info!("Generating release summary");

        let diff = strip_binary_diffs(diff);

        if diff.len() > MAX_DIFF_CHARS {
            tracing::warn!(
                "Diff is {} chars, exceeds estimated context limit, using fallback",
                diff.len()
            );
            return Ok(Self {
                items: Vec::new(),
                fallback_message: Some(
                    "The diff was too large to generate an AI summary.".to_string(),
                ),
            });
        }

        let commit_messages = commit_messages.join("\n");

        let pull_requests: String = pull_requests
            .iter()
            .map(|pr| {
                format!(
                    "<PullRequest number=\"{}\" title=\"{}\">{}</PullRequest>",
                    pr.number, pr.title, pr.body
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let user_prompt = format!(
            "<Diff>{diff}</Diff>
             <CommitMessages>{commit_messages}</CommitMessages>
             <PullRequests>{pull_requests}</PullRequests>"
        );

        let properties = json!({
            "items": {
                "type": "array",
                "description": "Categories of changes. Each category must have at least one item. Omit empty categories.",
                "items": {
                    "type": "object",
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "Category heading without trailing punctuation, e.g. \"New features\", \"Improvements\", \"Bug fixes\", \"Dependency changes\"."
                        },
                        "items": {
                            "type": "array",
                            "description": "One plain-text sentence per distinct change. No bullets, no prefixes, no markup beyond backticks for code references.",
                            "items": {
                                "type": "string"
                            }
                        }
                    },
                    "required": ["title", "items"],
                    "additionalProperties": false
                }
            }
        });

        let result = provider
            .send(AiRequest {
                system_prompt: SYSTEM_PROMPT,
                user_prompt,
                schema_name: "release_summary",
                properties,
                required: vec!["items"],
                max_tokens: Some(4096),
                temperature: None,
            })
            .await;

        match result {
            Ok(summary) => Ok(Self::sanitize(summary)),
            Err(AiError::ContextLengthExceeded) => {
                tracing::warn!("Diff too large for AI summary, using fallback message");
                Ok(Self {
                    items: Vec::new(),
                    fallback_message: Some(
                        "The diff was too large to generate an AI summary.".to_string(),
                    ),
                })
            }
            Err(AiError::Other(err)) => Err(err),
        }
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub struct SummaryCategory {
    pub title: String,
    pub items: Vec<String>,
}

pub struct PrContext<'a> {
    pub number: u64,
    pub title: &'a str,
    pub body: &'a str,
}

impl<'a> PrContext<'a> {
    pub fn from_pr(pr: &'a PullRequest) -> Option<Self> {
        let body = pr.body.as_deref().filter(|b| !b.is_empty())?;

        Some(Self {
            number: pr.number,
            title: &pr.title,
            body: truncate_pr_body(body),
        })
    }
}

fn strip_binary_diffs(diff: &str) -> String {
    let mut result = String::new();
    let mut current_section = String::new();

    for line in diff.lines() {
        if line.starts_with("diff --git ") && !current_section.is_empty() {
            if !is_binary_section(&current_section) {
                result.push_str(&current_section);
            }
            current_section.clear();
        }
        current_section.push_str(line);
        current_section.push('\n');
    }

    if !current_section.is_empty() && !is_binary_section(&current_section) {
        result.push_str(&current_section);
    }

    result
}

fn is_binary_section(section: &str) -> bool {
    section.lines().any(|l| l.starts_with("Binary files "))
}

fn truncate_pr_body(body: &str) -> &str {
    if body.len() <= MAX_PR_BODY_BYTES {
        return body;
    }

    // Find a safe UTF-8 byte boundary
    let mut end = MAX_PR_BODY_BYTES;
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }

    // Cut at the last newline for a clean break
    match body[..end].rfind('\n') {
        Some(pos) => &body[..pos],
        None => &body[..end],
    }
}

const SYSTEM_PROMPT: &str = "
    <Instructions>
        Your role is to analyse a git code diff, commit messages, and pull request descriptions to produce a concise release summary.
        Summarise WHAT changed from a user or product perspective — not HOW it was implemented. Describe the outcome, not the technical approach. For example, say \"Redesigned the authorization letter with updated content and styling\" instead of \"Replaced flex layouts with table layouts in the authorization letter PDF template\".
        Focus on understanding the broader context of changes and what features or fixes they represent — not individual code changes. PR descriptions and commit messages often contain technical root-cause analysis, debugging history, or implementation details — extract only the high-level change from them.
        Consolidate related changes into a single bullet point. If multiple changes touch the same feature, module, or area, they should be one bullet point — not separate bullets for each aspect of the change. Use comma-separated clauses to cover sub-changes within a single point. Aim for roughly one bullet point per distinct feature or area that changed.
        Omit incidental or supporting changes that only exist to support a primary change — for example, spell-checker config updates, test file changes, new internal modules, or config tweaks that accompany a feature change. These should not get their own bullet point or be mentioned at all.
        Keep each bullet point to one sentence. Use direct, concise language — avoid filler phrases like \"Fixed an issue where\", \"Implemented a workaround to address\", or \"Added support for\". Get straight to the substance.
        Write for a non-technical audience using simple terms. Avoid describing how a feature will impact a user or experience — just describe what the change is.
        Do not expand acronyms (e.g. PLP, PDP, USP) — readers already understand them.
        Wrap references to code identifiers (classes, functions, variables, enum variants, field names, file paths, package names, version numbers, etc.) in backticks. For example, write `ServiceCoverage` not ServiceCoverage, and `react` `18.3.0` not react 18.3.0. Do NOT backtick-wrap product names, service names, or general terms (e.g. HubSpot, Slack, authorization letter).
        Group changes into: New features, Improvements, Bug fixes, and Dependency changes. Only include a heading if it has items.
        Only classify something as a \"Bug fix\" if it corrects broken or unintended behaviour. Intentional changes to how something works — even if the new behaviour replaces the old — are improvements or new features, not bug fixes.
        Aim for the fewest bullet points possible while still covering every meaningful change. Prefer fewer, denser points over many granular ones. Minor formatting or cosmetic tweaks (e.g. date format changes, whitespace adjustments) should be folded into the parent change rather than listed separately.
        Dependency changes are ONLY additions, updates, or removals of third-party packages and libraries in package management files (e.g. package.json, pyproject.toml, Cargo.toml, requirements.txt, go.mod, Gemfile). Font files, images, static assets, new source code modules, config files, and any other non-package-manager changes are NOT dependency changes.
    </Instructions>
    <Steps>
        1. Analyse the diff, commit messages, and pull request descriptions to understand what changed from a product perspective.
        2. Identify distinct features, improvements, and fixes — grouping related changes and their supporting changes together.
        3. Write one concise bullet point per distinct change area, focusing on the outcome not the implementation.
        4. List dependency changes from package management files only (not fonts, assets, configs, or new source modules).
        5. Only include category headings that have items.
    </Steps>
    <ExampleOutPut1>
        <Input>
            Multiple commits improving HubSpot contact tracking: moved drift tracking to a long queue, added a 60-day filter on contacts, and added email property validation.
        </Input>
        <Output>
            Improvements:
            • Improved HubSpot contact tracking by optimising job scheduling, adding a 60-day contact filter, and requiring email verification.
        </Output>
    </ExampleOutPut1>
    <ExampleOutPut2>
        <Input>
            A PR that redesigns a PDF template with new fonts, CSS changes, layout restructuring, updated Jinja templates, new Python modules for plan service mappings, cspell config updates, test file changes, and factory updates.
            A separate PR that adds validation to prevent archiving on create.
        </Input>
        <Output>
            Improvements:
            • Redesigned the authorization letter with updated content, styling, and coverage logic.
            • Added validation to prevent archiving a clinic prospect on create.
        </Output>
    </ExampleOutPut2>
    <ExampleOutPut3>
        <Output>
            New features:
            • Search results can now be filtered by date and relevance.
            • Added avatar customisation options to user profiles.
            Bug fixes:
            • Fixed parsing error handling in the data service and resolved a caching bug in the authentication flow.
            Dependency changes:
            • Updated `XYZ` to `1.3.0` and added `ABC` `2.1.0`.
        </Output>
    </ExampleOutPut3>
    <ExampleOutPut4>
            New features:
            • Discord message URLs are now tracked for product discoveries.
    </ExampleOutPut4>
";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_pr(number: u64, title: &str, body: Option<&str>) -> PullRequest {
        serde_json::from_value(json!({
            "number": number,
            "title": title,
            "html_url": "https://github.com/test/repo/pull/1",
            "body": body,
            "user": { "type": "User" },
            "head": { "ref": "main" },
            "url": "https://api.github.com/repos/test/repo/pulls/1",
            "commits_url": "https://api.github.com/repos/test/repo/pulls/1/commits"
        }))
        .unwrap()
    }

    #[test]
    fn from_pr_with_body() {
        let pr = make_pr(1, "Add feature", Some("This adds a new feature"));
        let ctx = PrContext::from_pr(&pr).unwrap();
        assert_eq!(ctx.number, 1);
        assert_eq!(ctx.title, "Add feature");
        assert_eq!(ctx.body, "This adds a new feature");
    }

    #[test]
    fn from_pr_with_empty_body() {
        let pr = make_pr(1, "Add feature", Some(""));
        assert!(PrContext::from_pr(&pr).is_none());
    }

    #[test]
    fn from_pr_with_no_body() {
        let pr = make_pr(1, "Add feature", None);
        assert!(PrContext::from_pr(&pr).is_none());
    }

    #[test]
    fn strip_binary_diffs_removes_binary_sections() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
index abc..def 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
+use foo;
diff --git a/fonts/Bold.otf b/fonts/Bold.otf
new file mode 100644
index 000..abc
Binary files /dev/null and b/fonts/Bold.otf differ
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1 @@
-old
+new
";
        let result = strip_binary_diffs(diff);
        assert!(result.contains("src/main.rs"));
        assert!(result.contains("src/lib.rs"));
        assert!(!result.contains("fonts/Bold.otf"));
        assert!(!result.contains("Binary files"));
    }

    #[test]
    fn strip_binary_diffs_keeps_all_text_diffs() {
        let diff = "\
diff --git a/a.rs b/a.rs
+code
diff --git a/b.rs b/b.rs
+more code
";
        let result = strip_binary_diffs(diff);
        assert!(result.contains("a.rs"));
        assert!(result.contains("b.rs"));
    }

    #[test]
    fn strip_binary_diffs_removes_all_when_only_binary() {
        let diff = "\
diff --git a/img.png b/img.png
Binary files /dev/null and b/img.png differ
";
        let result = strip_binary_diffs(diff);
        assert!(result.is_empty());
    }

    #[test]
    fn sanitize_removes_garbage_items() {
        let summary = ReleaseSummary {
            items: vec![
                SummaryCategory {
                    title: "New features".into(),
                    items: vec![":{".into(), "Added search filtering".into()],
                },
                SummaryCategory {
                    title: "Bug fixes".into(),
                    items: vec!["•".into(), "".into()],
                },
            ],
            fallback_message: None,
        };
        let result = ReleaseSummary::sanitize(summary);
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].title, "New features");
        assert_eq!(result.items[0].items, vec!["Added search filtering"]);
    }

    #[test]
    fn sanitize_strips_leading_prefixes() {
        let summary = ReleaseSummary {
            items: vec![SummaryCategory {
                title: "Bug fixes".into(),
                items: vec![
                    ": Corrected the database configuration".into(),
                    "• Added a new feature".into(),
                    "- Removed old code".into(),
                    "Normal item without prefix".into(),
                ],
            }],
            fallback_message: None,
        };
        let result = ReleaseSummary::sanitize(summary);
        assert_eq!(
            result.items[0].items,
            vec![
                "Corrected the database configuration",
                "Added a new feature",
                "Removed old code",
                "Normal item without prefix",
            ]
        );
    }

    #[test]
    fn truncate_pr_body_short_body_unchanged() {
        let body = "Short PR description";
        assert_eq!(truncate_pr_body(body), body);
    }

    #[test]
    fn truncate_pr_body_cuts_at_last_newline() {
        let mut body = "Summary line\n\nDetails here\n\n".to_string();
        body.push_str(&"x".repeat(2000));
        let result = truncate_pr_body(&body);
        assert!(result.len() <= MAX_PR_BODY_BYTES);
        assert!(result.ends_with('\n'));
        assert!(result.contains("Summary line"));
    }

    #[test]
    fn truncate_pr_body_exact_limit() {
        let body = "a".repeat(MAX_PR_BODY_BYTES);
        assert_eq!(truncate_pr_body(&body), body.as_str());
    }
}
