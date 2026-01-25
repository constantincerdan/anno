use crate::services::{anthropic, jira::Issue};
use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
pub struct PrSummary {
    pub summary: String,
}

impl PrSummary {
    pub async fn new(diff: &str, commit_messages: &[String], issues: &[Issue]) -> Result<Self> {
        tracing::info!("Generating PR summary");

        let commit_messages = commit_messages.join("\n");
        let issues = issues
            .iter()
            .filter(|i| i.fields.description.is_some())
            .map(|i| {
                format!(
                    "- [{}] {}\n{}",
                    i.key,
                    i.fields.summary,
                    i.fields
                        .description
                        .as_ref()
                        .expect("Description to exist because of filter")
                )
            })
            .collect::<Vec<String>>()
            .join("\n");

        let user_prompt = format!(
            "<Diff>{diff}</Diff>
             <CommitMessages>{commit_messages}</CommitMessages>
             <JiraIssues>{issues}</JiraIssues>"
        );

        anthropic::Request {
            user_prompt,
            system_prompt: SYSTEM_PROMPT,
            tool_schema: response_schema(),
            tool_name: "pr_summary",
            ..Default::default()
        }
        .send()
        .await
    }
}

fn response_schema() -> Value {
    let summary = json!({
      "type": "string",
      "description": "A block of text summarising the pull request."
    });

    json!({
      "name": "pr_summary",
      "input_schema": {
        "type": "object",
        "properties": {
          "summary": summary,
        },
        "required": [
          "summary",
        ],
        "additionalProperties": false
      },
    })
}

const SYSTEM_PROMPT: &str = "
    <Instructions>
        Your task is to summarise a pull request to make it easier for other team members to understand the changes before reviewing.
        Use the diff, commit messages, and Jira issues (if provided) to summarise the code changes and how they relate to the feature or bug described in the issues.
        Keep your summary very short, clear and concise so that it provides a high-level overview of the changes and their impact.
        Use direct language and avoid redundant phrases; the fewer words you use, the clearer your summary will be.
        Avoid including any personal opinions or feedback in your summary, as this is a factual summary of the changes.
        Provide the summary without any introductory or concluding statements.
    </Instructions>
    <Steps>
        - Review the diff to understand the changes made in the pull request.
        - Review the commit messages to understand the context of the changes.
        - Review the Jira issues to understand the feature or bug being addressed.
        - Write a summary that explains the changes made in the pull request and how they relate to the feature or bug.
        - Keep your summary clear and concise, focusing on the high-level changes made in the pull request.
        - Provide the summary without any personal opinions or feedback, as this is a factual summary of the changes.
    </Steps>
";
