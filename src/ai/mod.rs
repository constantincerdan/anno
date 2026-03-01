pub mod pr_summary;
pub mod release_summary;

pub use pr_summary::*;
pub use release_summary::*;

use crate::services::{anthropic, openai};
use crate::utils::env;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

#[derive(Clone, Copy)]
pub enum AiProvider {
    OpenAi,
    Anthropic,
}

impl AiProvider {
    pub fn from_env() -> anyhow::Result<Self> {
        let value = env::get("AI_PROVIDER")?;

        match value.as_str() {
            "openai" => Ok(Self::OpenAi),
            "anthropic" => Ok(Self::Anthropic),
            other => {
                anyhow::bail!("Invalid ai_provider '{other}': must be 'openai' or 'anthropic'")
            }
        }
    }

    pub async fn send<T: DeserializeOwned>(&self, req: AiRequest) -> Result<T, AiError> {
        match self {
            Self::Anthropic => {
                anthropic::Request {
                    system_prompt: req.system_prompt,
                    user_prompt: req.user_prompt,
                    tool_name: req.schema_name,
                    tool_schema: Self::anthropic_schema(
                        req.schema_name,
                        &req.properties,
                        &req.required,
                    ),
                    max_tokens: req.max_tokens,
                    temperature: req.temperature,
                }
                .send()
                .await
            }
            Self::OpenAi => {
                openai::Request {
                    system_prompt: req.system_prompt,
                    user_prompt: req.user_prompt,
                    response_schema: Self::openai_schema(
                        req.schema_name,
                        &req.properties,
                        &req.required,
                    ),
                    temperature: req.temperature,
                    ..Default::default()
                }
                .send()
                .await
            }
        }
    }

    fn anthropic_schema(name: &str, properties: &Value, required: &[&str]) -> Value {
        json!({
            "name": name,
            "input_schema": {
                "type": "object",
                "properties": properties,
                "required": required,
                "additionalProperties": false
            }
        })
    }

    fn openai_schema(name: &str, properties: &Value, required: &[&str]) -> Value {
        json!({
            "type": "json_schema",
            "json_schema": {
                "name": name,
                "schema": {
                    "type": "object",
                    "properties": properties,
                    "required": required,
                    "additionalProperties": false
                },
                "strict": true
            }
        })
    }
}

pub struct AiRequest {
    pub system_prompt: &'static str,
    pub user_prompt: String,
    pub schema_name: &'static str,
    pub properties: Value,
    pub required: Vec<&'static str>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

impl std::fmt::Display for AiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenAi => write!(f, "openai"),
            Self::Anthropic => write!(f, "anthropic"),
        }
    }
}

pub enum AiError {
    ContextLengthExceeded,
    Other(anyhow::Error),
}

impl From<reqwest::Error> for AiError {
    fn from(e: reqwest::Error) -> Self {
        Self::Other(e.into())
    }
}

impl From<serde_json::Error> for AiError {
    fn from(e: serde_json::Error) -> Self {
        Self::Other(e.into())
    }
}

impl From<anyhow::Error> for AiError {
    fn from(e: anyhow::Error) -> Self {
        Self::Other(e)
    }
}

impl From<AiError> for anyhow::Error {
    fn from(e: AiError) -> Self {
        match e {
            AiError::ContextLengthExceeded => anyhow::anyhow!("AI context length exceeded"),
            AiError::Other(err) => err,
        }
    }
}
