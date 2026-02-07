pub mod pr_summary;
pub mod release_summary;

pub use pr_summary::*;
pub use release_summary::*;

use crate::utils::config;

#[derive(Clone, Copy)]
pub enum AiProvider {
    OpenAi,
    Anthropic,
}

impl AiProvider {
    pub fn from_config() -> anyhow::Result<Self> {
        let value = config::get("AI_PROVIDER");

        match value.as_str() {
            "openai" => Ok(Self::OpenAi),
            "anthropic" => Ok(Self::Anthropic),
            other => anyhow::bail!(
                "Invalid AI_PROVIDER '{other}': must be 'openai' or 'anthropic'"
            ),
        }
    }
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
