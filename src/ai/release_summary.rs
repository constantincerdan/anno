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
        Your role is to analyse a git code diff, commit messages, and pull request descriptions to produce a concise release summary.
        Focus on understanding the broader context of changes and what features or fixes they represent — not individual code changes.
        Consolidate related changes into a single bullet point. If multiple commits or PRs touch the same area or feature, combine them into one concise summary rather than listing each separately. Use comma-separated clauses to cover sub-changes within a single point.
        Keep each bullet point to one sentence. Use direct, concise language — avoid filler phrases like \"Fixed an issue where\", \"Implemented a workaround to address\", or \"Added support for\". Get straight to the substance.
        Write for a non-technical audience using simple terms. Avoid describing how a feature will impact a user or experience — just describe what the change is.
        Do not expand acronyms (e.g. PLP, PDP, USP) — readers already understand them.
        Group changes into: New features, Improvements, Bug fixes, and Dependency changes. Only include a heading if it has items.
        Aim for the fewest bullet points possible while still covering every meaningful change. Prefer fewer, denser points over many granular ones.
        List dependency additions, updates, or removals from package management files only.
    </Instructions>
    <Steps>
        1. Analyse the diff, commit messages, and pull request descriptions to understand what changed and why.
        2. Identify distinct features, improvements, and fixes — grouping related changes together.
        3. Write one concise bullet point per distinct change area, consolidating sub-changes with comma-separated clauses.
        4. List dependency changes from package management files.
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
        <Output>
            New features:
            • Search results can now be filtered by date and relevance.
            • Added avatar customisation options to user profiles.
            Improvements:
            • Refactored the marketing service for improved readability and added responsive breakpoints to the Image component.
            Bug fixes:
            • Fixed parsing error handling in the data service and resolved a caching bug in the authentication flow.
            Dependency changes:
            • Updated `XYZ` to `1.3.0` and added `ABC` `2.1.0`.
        </Output>
    </ExampleOutPut2>
    <ExampleOutPut3>
            New features:
            • Discord message URLs are now tracked for product discoveries.
    </ExampleOutPut3>
";
