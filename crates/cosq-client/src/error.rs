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
            message: msg.into(),
            hint: hint.into(),
        }
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound {
            message: msg.into(),
        }
    }
}
