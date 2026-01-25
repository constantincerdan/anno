use crate::services::openai::{self, OpenAiError};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Deserialize, Serialize, Debug)]
pub struct ReleaseSummary {
    pub items: Vec<SummaryCategory>,
    #[serde(skip)]
    pub fallback_message: Option<String>,
}

impl ReleaseSummary {
    pub async fn new(diff: &str, commit_messages: &[String]) -> Result<Self> {
        tracing::info!("Generating release summary");

        let commit_messages = commit_messages.join("\n");

        let user_prompt = format!(
            "<Diff>{diff}</Diff>
             <CommitMessages>{commit_messages}</CommitMessages>"
        );

        let result = openai::Request {
            user_prompt,
            system_prompt: SYSTEM_PROMPT,
            response_schema: response_schema(),
            ..Default::default()
        }
        .send()
        .await;

        match result {
            Ok(summary) => Ok(summary),
            Err(OpenAiError::ContextLengthExceeded) => {
                tracing::warn!("Diff too large for AI summary, using fallback message");
                Ok(Self {
                    items: Vec::new(),
                    fallback_message: Some(
                        "The diff was too large to generate an AI summary.".to_string(),
                    ),
                })
            }
            Err(OpenAiError::Other(err)) => Err(err),
        }
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub struct SummaryCategory {
    pub title: String,
    pub items: Vec<String>,
}

fn response_schema() -> Value {
    let category_title = json!({
        "type": "string",
        "description": "The title of the JSON object."
    });

    let category_items = json!({
        "type": "array",
        "description": "An array of strings.",
        "items": {
            "type": "string"
        }
    });

    let category = json!({
        "type": "object",
        "properties": {
            "title": category_title,
            "items": category_items
        },
        "required": [
            "title",
            "items"
        ],
        "additionalProperties": false
    });

    let categories = json!({
        "type": "array",
        "description": "An array of JSON objects where each object has a title and an items array.",
        "items": category
    });

    let schema = json!({
        "name": "json_objects_array",
        "schema": {
            "type": "object",
            "properties": {
                "items": categories
            },
            "required": [
                "items"
            ],
            "additionalProperties": false
        },
        "strict": true
    });

    json!({
        "type": "json_schema",
        "json_schema": schema
    })
}

const SYSTEM_PROMPT: &str = "
    <Instructions>
        Your role is to analyse a git code diff and related commit messages to identify and summarise the features that have been released.
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
