use crate::ai::{AiError, AiProvider, AiRequest};
use crate::services::github::PullRequest;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;

const MAX_DIFF_CHARS: usize = 400_000;

#[derive(Deserialize, Serialize, Debug)]
pub struct ReleaseSummary {
    pub items: Vec<SummaryCategory>,
    #[serde(skip)]
    pub fallback_message: Option<String>,
}

impl ReleaseSummary {
    pub async fn new(
        provider: AiProvider,
        diff: &str,
        commit_messages: &[String],
        pull_requests: &[PrContext<'_>],
    ) -> Result<Self> {
        tracing::info!("Generating release summary");

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
                "description": "An array of JSON objects where each object has a title and an items array.",
                "items": {
                    "type": "object",
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "The title of the JSON object."
                        },
                        "items": {
                            "type": "array",
                            "description": "An array of strings.",
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
            Ok(summary) => Ok(summary),
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
            body,
        })
    }
}

const SYSTEM_PROMPT: &str = "
    <Instructions>
        Your role is to analyse a git code diff, commit messages, and pull request descriptions to identify and summarise the features that have been released.
        Avoid describing each individual code change. Instead, focus on understanding the broader context of the changes and what features they translate into.
        Keep your description of each feature concise and non-technical, so that a non-technical team member can understand the change in simple terms.
        Avoid listing every commit message or code change. Instead, group the changes into categories like New features, Improvements, Bug fixes and Dependency changes.
        Avoid describing how a feature will impact a user or experience, just describe what the feature is and what it does.
        Avoid expanding acronyms, for example PLP, PDP or USP, to their full meanings because the users understand those.
        List any dependency additions, updates, or removals that were made in the package management files only.
    </Instructions>
    <Steps>
        Analyse the Diff: Examine the git code diff to understand the changes in the codebase.
        Analyse Commit Messages: Review the commit messages to gain context and further insights into the changes.
        Analyse Pull Request Descriptions: Review the pull request descriptions for additional context about the motivation and scope of changes.
        Identify User-Facing Features: Determine which changes correspond to new features, enhancements, or bug fixes that would be noticeable to the end-users.
        Summarise in Non-Technical Terms: Write a summary of these features in a way that a non-technical team can understand, but no longer than a sentence.
        List Dependency Changes: Identify any dependency changes made in the package management files (e.g., new libraries, updated versions) and list them.
        Exclude Unchanged Sections: Only include headings for New features, Improvements, Bug fixes, and Dependency changes if there are updates to list for those headings.
    </Steps>
    <ExampleOutPut1>
        <Output>
            New features:
            • Search results can now be filtered by date and relevance.
            • New avatar customisation options have been added to user profiles.
            Improvements:
            • Refactored the marketing service to improve readability.
            • Added more breakpoints to the Image component.
            Bug fixes:
            • Fixed an issue where the data service was not guarding against unexpected parsing errors.
            • Implemented a workaround to address the caching bug in the user authentication flow.
            Dependency changes:
            • Updated Library `XYZ` to version `1.3.0`.
            • Added library `ABC` version `2.1.0`.
        </Output>
    </ExampleOutPut1>
    <ExampleOutPut2>
            New features
            • Added support for tracking URLs in Discord messages for new product discoveries.
    </ExampleOutPut2>
    <ExampleOutPut3>
            Bug fixes
            • Fixed an issue where the Twitter hyperlink was not displaying properly.
    </ExampleOutPut3>
";
