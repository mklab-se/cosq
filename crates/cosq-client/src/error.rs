//! Error types for cosq-client

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("authentication failed: {message}")]
    Auth { message: String },

    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("API error ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("access denied: {message}\n\nHint: {hint}")]
    Forbidden { message: String, hint: String },

    #[error("not found: {message}")]
    NotFound { message: String },

    #[error("Azure CLI error: {message}\n\nHint: {hint}")]
    AzCli { message: String, hint: String },

    #[error("{0}")]
    Other(String),
}

impl ClientError {
    pub fn auth(msg: impl Into<String>) -> Self {
        Self::Auth {
            message: msg.into(),
        }
    }

    pub fn az_cli(msg: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::AzCli {
            message: msg.into(),
            hint: hint.into(),
        }
    }

    pub fn forbidden(msg: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::Forbidden {
            message: extract_message(msg.into()),
            hint: hint.into(),
        }
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound {
            message: msg.into(),
        }
    }

    pub fn api(status: u16, body: impl Into<String>) -> Self {
        Self::Api {
            status,
            message: extract_message(body.into()),
        }
    }
}

/// Try to extract a human-readable message from a Cosmos DB JSON error body.
/// Falls back to the raw string if parsing fails.
fn extract_message(body: String) -> String {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
        if let Some(msg) = json["message"].as_str().or(json["Message"].as_str()) {
            // Cosmos DB often appends "\r\nActivityId: ..." — strip that
            let clean = msg.split("\r\nActivityId:").next().unwrap_or(msg).trim();
            return clean.to_string();
        }
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_message_cosmos_json() {
        let body = r#"{"code":"Forbidden","message":"Request blocked by Auth mklabcosdb : Request is blocked because principal [abc-123] does not have required RBAC permissions to perform action [Microsoft.DocumentDB/databaseAccounts/readMetadata] on any scope. Learn more: https://aka.ms/cosmos-native-rbac.\r\nActivityId: c93b2c4e-faf8-4a23-848e-1f03c0e0d8a7, Microsoft.Azure.Documents.Common/2.14.0"}"#;
        let msg = extract_message(body.to_string());
        assert!(msg.starts_with("Request blocked by Auth"));
        assert!(msg.contains("readMetadata"));
        assert!(!msg.contains("ActivityId:"));
    }

    #[test]
    fn test_extract_message_plain_text() {
        let body = "something went wrong";
        let msg = extract_message(body.to_string());
        assert_eq!(msg, "something went wrong");
    }

    #[test]
    fn test_extract_message_json_without_message_field() {
        let body = r#"{"error": "oops"}"#;
        let msg = extract_message(body.to_string());
        assert_eq!(msg, body);
    }

    #[test]
    fn test_extract_message_capital_message() {
        let body = r#"{"Message": "Something failed"}"#;
        let msg = extract_message(body.to_string());
        assert_eq!(msg, "Something failed");
    }
}
