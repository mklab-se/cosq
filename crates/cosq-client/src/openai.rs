//! Azure OpenAI client for AI-enhanced features
//!
//! Sends chat completion requests to Azure OpenAI using AAD token
//! authentication acquired via the Azure CLI.

use serde_json::Value;
use tracing::debug;

use crate::auth::AzCliAuth;
use crate::error::ClientError;
use cosq_core::config::AiConfig;

/// Cognitive Services resource scope for token acquisition
const COGNITIVE_SERVICES_RESOURCE: &str = "https://cognitiveservices.azure.com";

/// Azure OpenAI client for chat completions
pub struct AzureOpenAIClient {
    http: reqwest::Client,
    token: String,
    endpoint: String,
    deployment: String,
    api_version: String,
}

impl AzureOpenAIClient {
    /// Create a new client from AI config, acquiring a token via Azure CLI.
    pub async fn from_config(config: &AiConfig) -> Result<Self, ClientError> {
        let token = AzCliAuth::get_token(COGNITIVE_SERVICES_RESOURCE).await?;
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;

        Ok(Self {
            http,
            token,
            endpoint: config.openai_endpoint(),
            deployment: config.deployment.clone(),
            api_version: config.api_version.clone(),
        })
    }

    /// Send a chat completion request and return the response text.
    pub async fn chat_completion(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        temperature: f32,
        max_tokens: u32,
    ) -> Result<String, ClientError> {
        let url = format!(
            "{}/openai/deployments/{}/chat/completions?api-version={}",
            self.endpoint, self.deployment, self.api_version
        );
        debug!("Azure OpenAI chat completion: {}", url);

        let body = serde_json::json!({
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_prompt },
            ],
            "temperature": temperature,
            "max_tokens": max_tokens,
        });

        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ClientError::openai(format!(
                "API error ({}): {}",
                status.as_u16(),
                extract_openai_message(&body)
            )));
        }

        let json: Value = response.json().await?;
        let content = json
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        Ok(content)
    }
}

/// Extract a human-readable message from an Azure OpenAI error response.
fn extract_openai_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|json| {
            json.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .map(String::from)
        })
        .unwrap_or_else(|| body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_openai_message_json() {
        let body = r#"{"error": {"message": "Model not found", "code": "model_not_found"}}"#;
        let msg = extract_openai_message(body);
        assert_eq!(msg, "Model not found");
    }

    #[test]
    fn test_extract_openai_message_plain_text() {
        let body = "something went wrong";
        let msg = extract_openai_message(body);
        assert_eq!(msg, "something went wrong");
    }

    #[test]
    fn test_extract_openai_message_no_error_field() {
        let body = r#"{"status": "error"}"#;
        let msg = extract_openai_message(body);
        assert_eq!(msg, body);
    }
}
