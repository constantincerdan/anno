use crate::ai;
use crate::services::{
    github::{CommitAuthor, GitHubIssue, PullRequest, workflows::WorkflowRun},
    jira::Issue,
};
use crate::utils::{diff::DiffStats, env, http};
use anyhow::Error;
use serde_json::{Value, json};

pub struct ReleaseSummary<'a> {
    pub app_name: String,
    pub jira_base_url: Option<String>,
    pub github_issues: Vec<GitHubIssue>,
    pub jira_issues: Vec<Issue>,
    pub diff_url: String,
    pub diff_stats: DiffStats,
    pub compare_to_default_branch_url: String,
    pub default_branch: String,
    pub prev_run_url: Option<&'a str>,
    pub pull_requests: Vec<PullRequest>,
    pub contributors: Vec<CommitAuthor>,
    pub run: &'a WorkflowRun,
    pub summary: ai::ReleaseSummary,
}

impl ReleaseSummary<'_> {
    pub async fn send(&self) -> Result<(), Error> {
        let send_slack_msg = env::get_optional("SLACK_MESSAGE_ENABLED").as_deref() == Some("true");

        if !send_slack_msg {
            println!("{:#?}", self.summary);
            return Ok(());
        }

        tracing::info!("Posting release summary to Slack");

        let mut message_blocks = vec![self.get_header_block(), json!({ "type": "divider" })];

        message_blocks.extend(self.get_summary_block());

        if !self.jira_issues.is_empty()
            || !self.github_issues.is_empty()
            || !self.pull_requests.is_empty()
        {
            message_blocks.push(json!({ "type": "divider" }));
        }

        if !self.pull_requests.is_empty() {
            message_blocks.push(self.get_pull_requests_block());
        }

        if !self.github_issues.is_empty() {
            message_blocks.push(self.get_github_issues_block());
        }

        if !self.jira_issues.is_empty() {
            message_blocks.push(self.get_jira_tickets_block());
        }

        message_blocks.push(self.get_actions_block());
        message_blocks.push(json!({ "type": "divider" }));
        message_blocks.push(self.get_metadata_block());

        let webhook_url = env::get("SLACK_WEBHOOK_URL")?;
        let payload = json!({"blocks": json!(message_blocks)});

        http::send_with_retry(|| http::client().post(&webhook_url).json(&payload))
            .await?
            .error_for_status()
            .inspect_err(|e| tracing::error!("Error posting Slack message: {e}"))?;

        Ok(())
    }

    fn get_header_block(&self) -> Value {
        json!({
            "type": "header",
            "text": {
                "type": "plain_text",
                "text": format!("{} release :rocket:", self.app_name),
                "emoji": true
            }
        })
    }

    fn get_summary_block(&self) -> Vec<Value> {
        if let Some(fallback) = &self.summary.fallback_message {
            return vec![json!({
                "type": "section",
                "text": {
                    "type": "mrkdwn",
                    "text": format!("_{fallback}_"),
                }
            })];
        }

        let mut blocks = Vec::new();

        for category in &self.summary.items {
            let items = category
                .items
                .iter()
                .map(|note| format!(r"  •  {note}"))
                .collect::<Vec<_>>()
                .join("\n");

            blocks.push(json!({
                "type": "section",
                "text": {
                    "type": "mrkdwn",
                    "text": format!("*{}*\n{items}", category.title),
                }
            }));
        }

        blocks
    }

    fn get_pull_requests_block(&self) -> Value {
        json!({
            "type": "rich_text",
            "elements": [
                {
                    "type": "rich_text_section",
                    "elements": [
                        {
                            "type": "text",
                            "text": "Pull requests",
                            "style": {
                                "bold": true
                            }
                        }
                    ]
                },
                {
                    "type": "rich_text_list",
                    "style": "bullet",
                    "elements": self.pull_requests
                    .iter()
                    .map(|pr| {
                        json!({
                            "type": "rich_text_section",
                            "elements": [
                                {
                                    "type": "link",
                                    "text": format!("#{} {}", pr.number, pr.title),
                                    "url": pr.html_url,
                                }
                            ]
                        })
                    })
                    .collect::<Vec<_>>()
                }
            ]
        })
    }

    fn get_github_issues_block(&self) -> Value {
        json!({
            "type": "rich_text",
            "elements": [
                {
                    "type": "rich_text_section",
                    "elements": [
                        {
                            "type": "text",
                            "text": "Issues",
                            "style": {
                                "bold": true
                            }
                        }
                    ]
                },
                {
                    "type": "rich_text_list",
                    "style": "bullet",
                    "elements": self.github_issues
                    .iter()
                    .map(|issue| {
                        json!({
                            "type": "rich_text_section",
                            "elements": [
                                {
                                    "type": "link",
                                    "text": format!("#{} {}", issue.number, issue.title),
                                    "url": issue.html_url,
                                }
                            ]
                        })
                    })
                    .collect::<Vec<_>>()
                }
            ]
        })
    }

    fn get_jira_tickets_block(&self) -> Value {
        json!({
            "type": "rich_text",
            "elements": [
                {
                    "type": "rich_text_section",
                    "elements": [
                        {
                            "type": "text",
                            "text": "Jira tickets",
                            "style": {
                                "bold": true
                            }
                        }
                    ]
                },
                {
                    "type": "rich_text_list",
                    "style": "bullet",
                    "elements": self.jira_issues
                    .iter()
                    .map(|issue| {
                        json!({
                            "type": "rich_text_section",
                            "elements": [
                                {
                                    "type": "link",
                                    "text": format!("{} {}", issue.key, issue.fields.summary),
                                    "url": issue.get_browse_url(self.jira_base_url.as_deref().unwrap_or_default()),
                                }
                            ]
                        })
                    })
                    .collect::<Vec<_>>()
                }
            ]
        })
    }

    fn get_actions_block(&self) -> Value {
        let mut overflow_options = vec![json!({
            "text": {
                "type": "plain_text",
                "text": format!("Compare to {}", self.default_branch),
            },
            "url": self.compare_to_default_branch_url
        })];

        if let Some(prev_run_url) = self.prev_run_url {
            overflow_options.push(json!({
                "text": {
                    "type": "plain_text",
                    "text": "Previous deployment",
                },
                "url": prev_run_url
            }));
        }

        json!({
            "type": "actions",
            "elements": [
                {
                    "type": "button",
                    "text": {
                        "type": "plain_text",
                        "text": "Deployment",
                    },
                    "url": self.run.get_run_url()
                },
                {
                    "type": "button",
                    "text": {
                        "type": "plain_text",
                        "text": "Diff",
                    },
                    "url": self.diff_url
                },
                {
                    "type": "overflow",
                    "options": overflow_options
                }
            ]
        })
    }

    fn get_metadata_block(&self) -> Value {
        let mut elements: Vec<Value> = self
            .contributors
            .iter()
            .map(|c| {
                json!({
                    "type": "image",
                    "image_url": c.avatar_url,
                    "alt_text": c.login
                })
            })
            .collect();

        let names = self
            .contributors
            .iter()
            .map(|c| format!("*{}*", c.login))
            .collect::<Vec<_>>()
            .join(", ");

        elements.push(json!({
            "type": "mrkdwn",
            "text": format!("Contributors: {names}")
        }));

        elements.push(json!({
            "type": "mrkdwn",
            "text": format!("🪧 Branch: *{}*", self.run.head_branch)
        }));

        elements.push(json!({
            "type": "mrkdwn",
            "text": format!("✏️ Changes: {}", self.diff_stats)
        }));

        elements.push(json!({
            "type": "mrkdwn",
            "text": format!("🔖 Commit: `{}`", &self.run.head_sha[..7])
        }));

        json!({
            "type": "context",
            "elements": elements
        })
    }
}
