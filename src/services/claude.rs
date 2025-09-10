use crate::utils::config;
use anyhow::Result;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};

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
    pub async fn send<T: DeserializeOwned>(self) -> Result<T> {
        let base_url = config::get("CLAUDE_BASE_URL");
        let api_key = config::get("CLAUDE_API_KEY");
        let model = config::get("CLAUDE_MODEL");

        let response = reqwest::Client::new()
            .post(format!("{base_url}/v1/messages"))
            .header("content-type", "application/json")
            .header("anthropic-version", "2023-06-01")
            .header("x-api-key", api_key)
            .json(&json!({
                "model": model,
                "max_tokens": self.max_tokens.unwrap_or(1024),
                "temperature": self.temperature.unwrap_or(0.0),
                "system": self.system_prompt,
                "messages": [{ "role": "user", "content": self.user_prompt }],
                "tools": [self.tool_schema],
                "tool_choice": { "type": "tool", "name": self.tool_name }
            }))
            .send()
            .await?
            .error_for_status()
            .inspect_err(|e| tracing::error!("Error making Claude request: {e}"))?
            .json::<Response<T>>()
            .await?
            .content
            .into_iter()
            .next()
            .expect("At least one item to be returned")
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
