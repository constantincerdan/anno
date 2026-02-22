use crate::ai::AiError;
use crate::utils::{env, http};
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
    pub async fn send<T: DeserializeOwned>(self) -> Result<T, AiError> {
        let base_url = env::get("OPENAI_BASE_URL")?;
        let api_key = env::get("AI_API_KEY")?;
        let model = env::get("AI_MODEL")?;

        let body = json!({
            "model": model,
            "temperature": self.temperature.unwrap_or(0.0),
            "frequency_penalty": self.frequency_penalty.unwrap_or(0.3),
            "messages": [
                { "role": "system", "content": self.system_prompt },
                { "role": "user", "content": self.user_prompt }
            ],
            "response_format": self.response_schema
        });

        let resp = http::send_with_retry(|| {
            http::client()
                .post(format!("{base_url}/chat/completions"))
                .header("content-type", "application/json")
                .bearer_auth(&api_key)
                .json(&body)
        })
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();

            match resp.text().await {
                Ok(body) => {
                    tracing::error!("Error making OpenAI request");
                    tracing::error!("Status: {status}");
                    tracing::error!("Response: {body}");

                    if let Some(code) = parse_error_code(&body)
                        && code == "context_length_exceeded"
                    {
                        return Err(AiError::ContextLengthExceeded);
                    }
                }
                Err(e) => {
                    tracing::error!("OpenAI API error - status={status} (failed to read body: {e})")
                }
            }

            return Err(AiError::Other(anyhow::anyhow!(
                "OpenAI API returned non-success status: {status}"
            )));
        }

        let response = resp
            .json::<Response>()
            .await?
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("OpenAI returned empty response"))?
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

fn parse_error_code(body: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(body).ok()?;
    parsed["error"]["code"].as_str().map(String::from)
}
