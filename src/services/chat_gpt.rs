use crate::utils::config;
use anyhow::Result;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

#[derive(Default)]
pub struct Request {
    pub temperature: Option<f32>,
    pub frequency_penalty: Option<f32>,
    pub system_prompt: &'static str,
    pub user_prompt: String,
    pub response_schema: Value,
}

impl Request {
    pub async fn send<T: DeserializeOwned>(self) -> Result<T> {
        let base_url = config::get("CHAT_GPT_BASE_URL");
        let api_key = config::get("CHAT_GPT_API_KEY");
        let model = config::get_optional("CHAT_GPT_MODEL");

        let req = reqwest::Client::new()
            .post(format!("{base_url}/chat/completions"))
            .header("content-type", "application/json")
            .bearer_auth(api_key)
            .json(&json!({
                "model": model.as_deref().unwrap_or("gpt-4o"),
                "temperature": self.temperature.unwrap_or(0.0),
                "frequency_penalty": self.frequency_penalty.unwrap_or(0.3),
                "messages": [
                    { "role": "system", "content": self.system_prompt },
                    { "role": "user", "content": self.user_prompt }
                ],
                "response_format": self.response_schema
            }));

        let resp = req.send().await?;

        if !resp.status().is_success() {
            let status = resp.status();

            match resp.text().await {
                Ok(body) => {
                    tracing::error!("Error making ChatGPT request");
                    tracing::error!("Status: {status}");
                    tracing::error!("Response: {body}")
                }
                Err(e) => {
                    tracing::error!(
                        "ChatGPT API error - status={status} (failed to read body: {e})"
                    )
                }
            }

            anyhow::bail!("ChatGPT API returned non-success status: {status}");
        }

        let response = resp
            .json::<Response>()
            .await?
            .choices
            .into_iter()
            .next()
            .expect("At least one choice to be returned")
            .message
            .content;

        let parsed_response: T = serde_json::from_str(&response)?;

        Ok(parsed_response)
    }
}

#[derive(Deserialize)]
pub struct Response {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: String,
}
