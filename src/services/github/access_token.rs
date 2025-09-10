use crate::utils::config;
use anyhow::Result;
use base64::prelude::*;
use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm::RS256, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

#[derive(Deserialize)]
pub struct AccessToken {
    token: String,
}

pub static GITHUB_ACCESS_TOKEN: OnceCell<String> = OnceCell::const_new();

impl AccessToken {
    pub async fn get() -> Result<&'static String> {
        GITHUB_ACCESS_TOKEN.get_or_try_init(Self::fetch).await
    }

    async fn fetch() -> Result<String> {
        // Use token set by Github in an action context if available
        if let Some(github_token) = config::get_optional("GITHUB_TOKEN") {
            return Ok(github_token);
        }

        let gh_base_url = config::get("GITHUB_BASE_URL");
        let gh_app_install_id = config::get("GITHUB_APP_INSTALLATION_ID");

        let jwt_token = create_jwt_token();
        let url = format!("{gh_base_url}/app/installations/{gh_app_install_id}/access_tokens");

        let access_token = reqwest::Client::new()
            .post(url)
            .bearer_auth(jwt_token)
            .header("Accept", "application/json")
            .header("User-Agent", "Anno")
            .send()
            .await?
            .error_for_status()
            .inspect_err(|e| tracing::error!("Error fetching GitHub access token: {e}"))?
            .json::<Self>()
            .await?
            .token;

        Ok(access_token)
    }
}

pub fn create_jwt_token() -> String {
    let gh_app_id = config::get("GITHUB_APP_ID");
    let gh_app_private_key_base64 = config::get("GITHUB_APP_PRIVATE_KEY_BASE64");

    let private_key = BASE64_STANDARD
        .decode(gh_app_private_key_base64)
        .expect("Private key to be base64");

    let now = Utc::now();
    let claims = Claims {
        iss: gh_app_id,
        iat: (now - Duration::seconds(60)).timestamp(),
        exp: (now + Duration::minutes(10)).timestamp(),
    };

    let key = EncodingKey::from_rsa_pem(&private_key).expect("Private key to be valid");

    jsonwebtoken::encode(&Header::new(RS256), &claims, &key).expect("Encoding to be successful")
}

#[derive(Serialize, Deserialize)]
struct Claims {
    iss: String,
    iat: i64,
    exp: i64,
}
