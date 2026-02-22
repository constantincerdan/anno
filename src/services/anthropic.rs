use crate::ai::AiError;
use crate::utils::{env, http};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

#[derive(Default)]
pub struct Request {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub system_prompt: &'static str,
    pub user_prompt: String,
    pub tool_schema: Value,
    pub tool_name: &'static str,
}

impl Request {
    pub async fn send<T: DeserializeOwned>(self) -> Result<T, AiError> {
        let base_url = env::get("ANTHROPIC_BASE_URL")?;
        let api_key = env::get("AI_API_KEY")?;
        let model = env::get("AI_MODEL")?;

        let body = json!({
            "model": model,
            "max_tokens": self.max_tokens.unwrap_or(1024),
            "temperature": self.temperature.unwrap_or(0.0),
            "system": self.system_prompt,
            "messages": [{ "role": "user", "content": self.user_prompt }],
            "tools": [self.tool_schema],
            "tool_choice": { "type": "tool", "name": self.tool_name }
        });

        let resp = http::send_with_retry(|| {
            http::client()
                .post(format!("{base_url}/v1/messages"))
                .header("content-type", "application/json")
                .header("anthropic-version", "2023-06-01")
                .header("x-api-key", &api_key)
                .json(&body)
        })
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();

            match resp.text().await {
                Ok(body) => {
                    tracing::error!("Error making Anthropic request");
                    tracing::error!("Status: {status}");
                    tracing::error!("Response: {body}");

                    if is_context_length_error(&body) {
                        return Err(AiError::ContextLengthExceeded);
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "Anthropic API error - status={status} (failed to read body: {e})"
                    )
                }
            }

            return Err(AiError::Other(anyhow::anyhow!(
                "Anthropic API returned non-success status: {status}"
            )));
        }

        let response = resp
            .json::<Response<T>>()
            .await?
            .content
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Anthropic returned empty response"))?
            .input;

        Ok(response)
    }
}

#[derive(Deserialize)]
pub struct Response<T> {
    pub content: Vec<ContentItem<T>>,
}

#[derive(Deserialize)]
pub struct ContentItem<T> {
    pub input: T,
}

fn is_context_length_error(body: &str) -> bool {
    let Ok(parsed) = serde_json::from_str::<Value>(body) else {
        return false;
    };

    parsed["error"]["type"].as_str() == Some("invalid_request_error")
        && parsed["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("too long") || m.contains("too many tokens"))
}
