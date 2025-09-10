use crate::services::claude;
use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
pub struct PrReview {
    verdict: Verdict,
    pub feedback: String,
}

impl PrReview {
    pub async fn new(diff: &str, commit_messages: &[String]) -> Result<Self> {
        tracing::info!("Generating PR analysis");

        let commit_messages = commit_messages.join("\n");

        let user_prompt = format!(
            "<Diff>{diff}</Diff>
             <CommitMessages>{commit_messages}</CommitMessages>"
        );

        claude::Request {
            user_prompt,
            system_prompt: SYSTEM_PROMPT,
            tool_schema: response_schema(),
            tool_name: "pr_review",
            ..Default::default()
        }
        .send()
        .await
    }

    pub fn is_positive(&self) -> bool {
        matches!(self.verdict, Verdict::Positive)
    }
}

#[derive(Deserialize, Debug)]
enum Verdict {
    Positive,
    Negative,
}

fn response_schema() -> Value {
    let verdict = json!({
      "type": "string",
      "enum": [
        "Positive",
        "Negative"
      ],
      "description": "The verdict which can either be Positive or Negative."
    });

    let feedback = json!({
      "type": "string",
      "description": "A markdown block of text providing feedback."
    });

    json!({
      "name": "pr_review",
      "input_schema": {
        "type": "object",
        "properties": {
          "verdict": verdict,
          "feedback": feedback
        },
        "required": [
          "verdict",
          "feedback"
        ],
        "additionalProperties": false
      },
    })
}

const SYSTEM_PROMPT: &str = "
    <Instructions>
        Your role is to analyse the code diff and commit messages of pull requests to identify bugs.
        Pay attention to what has been deleted (denoted by '-') or added (denoted by '+') to ensure you don't mention bugs in code that are no longer present.
        If code or logic was been removed, accept that it is intentional and focus on the remaining code; avoid speculating on the removed code and the impact it may have.
        The bugs you identify should only affect the code that you can see in the pull request.
        Keep your response short and to the point, focusing on the key points of the bugs and explaining why they are bugs.
        You can mention multiple bugs in your response, but make sure they are explicitly present in the pull request and are not just general observations.
        It's important that you are absolutely certain any bugs you mention are in fact bugs and not just ifs, could-bes or maybes.
        When listing a bug, provide a snippet of the code that is causing the bug if possible and explain how it's a bug.
        If the pull request has bugs, start your response with 'This PR may contain the following bugs:'.
        Format your response as a list of bugs that are present in the pull request in markdown.
        Double check your output and ensure that it is valid markdown.
        Avoid instructing the developer to fix the bugs, just providing the bugs is enough.
        If the pull request does not contain any bugs, simply state 'LGTM 👍'.
        Your response should be placed in <Output> tags.
    </Instructions>
    <Steps>
        Analyze the Code Diff: Examine the code changes in the pull request to understand the modifications.
        Review the Code Changes: Pay attention to what has been deleted (-) or added (+) to ensure you don't mention bugs or issues in code that are no longer present.
        Analyze Commit Messages: Review the commit messages to gain context and further insights into the changes.
        Identify Bugs: Determine which code changes have introduced bugs or issues.
        Summarize in Markdown: List the bugs that are present in the pull request in markdown format.
        Provide Feedback: Deliver the feedback to the developer.
    </Steps>
";
