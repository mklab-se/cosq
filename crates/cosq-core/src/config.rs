//! Configuration file handling for cosq
//!
//! Config is stored at `~/.config/cosq/config.yaml` (or the platform equivalent
//! via `dirs::config_dir()`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Config filename within the cosq config directory
const FILENAME: &str = "config.yaml";

/// Application directory name
const APP_DIR: &str = "cosq";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config: {0}")]
    Read(#[from] std::io::Error),

    #[error("failed to parse config: {0}")]
    Parse(#[from] serde_yaml::Error),

    #[error("config not found — run `cosq init` to get started")]
    NotFound,

    #[error("could not determine config directory")]
    NoConfigDir,
}

/// Cosmos DB account configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    /// Cosmos DB account name
    pub name: String,

    /// Azure subscription ID
    pub subscription: String,

    /// Azure resource group name
    pub resource_group: String,

    /// Cosmos DB account endpoint URL
    pub endpoint: String,
}

/// AI provider backend for query generation
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiProvider {
    /// Azure OpenAI API (requires Azure subscription and deployment)
    #[default]
    AzureOpenai,
    /// Anthropic Claude via local CLI
    Claude,
    /// OpenAI Codex via local CLI
    Codex,
    /// GitHub Copilot via local CLI
    Copilot,
    /// Local LLMs via Ollama
    Ollama,
}

impl std::fmt::Display for AiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}

impl AiProvider {
    /// Human-readable name for display
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::AzureOpenai => "Azure OpenAI",
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Copilot => "Copilot",
            Self::Ollama => "Ollama",
        }
    }

    /// Short description of the provider
    pub fn description(&self) -> &'static str {
        match self {
            Self::AzureOpenai => "Azure OpenAI API (requires Azure subscription)",
            Self::Claude => "Anthropic Claude via local CLI",
            Self::Codex => "OpenAI Codex via local CLI",
            Self::Copilot => "GitHub Copilot via local CLI",
            Self::Ollama => "Local LLMs via Ollama",
        }
    }

    /// Binary name to check for availability (None for API-only providers)
    pub fn binary_name(&self) -> Option<&'static str> {
        match self {
            Self::AzureOpenai => None,
            Self::Claude => Some("claude"),
            Self::Codex => Some("codex"),
            Self::Copilot => Some("copilot"),
            Self::Ollama => Some("ollama"),
        }
    }

    /// Default model recommendation for this provider (None if user must choose)
    pub fn default_model(&self) -> Option<&'static str> {
        match self {
            Self::AzureOpenai => None, // uses deployment name from config
            Self::Claude => Some("sonnet"),
            Self::Codex => Some("o4-mini"),
            Self::Copilot => Some("gpt-4.1"),
            Self::Ollama => None, // user must select from installed models
        }
    }

    /// All known providers
    pub fn all() -> &'static [AiProvider] {
        &[
            Self::Claude,
            Self::Codex,
            Self::Copilot,
            Self::Ollama,
            Self::AzureOpenai,
        ]
    }
}

/// AI configuration for natural language query generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    /// AI provider backend
    #[serde(default)]
    pub provider: AiProvider,

    /// Model to use (optional, provider-specific defaults applied at runtime)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Azure AI Services account name (Azure OpenAI only)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,

    /// Model deployment name, e.g., "gpt-4o-mini" (Azure OpenAI only)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment: Option<String>,

    /// Custom endpoint URL — overrides name-based URL (Azure OpenAI only)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,

    /// Azure subscription ID (for ARM discovery during `ai init`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription: Option<String>,

    /// Azure resource group (for ARM discovery)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_group: Option<String>,

    /// Azure OpenAI API version
    #[serde(default = "default_openai_api_version")]
    pub api_version: String,

    /// Ollama server URL (defaults to http://localhost:11434)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ollama_url: Option<String>,
}

fn default_openai_api_version() -> String {
    "2024-12-01-preview".to_string()
}

impl AiConfig {
    /// Get the Azure OpenAI endpoint URL (only valid for Azure OpenAI provider)
    pub fn openai_endpoint(&self) -> Option<String> {
        if let Some(ref ep) = self.endpoint {
            Some(ep.trim_end_matches('/').to_string())
        } else {
            self.account
                .as_ref()
                .map(|a| format!("https://{a}.openai.azure.com"))
        }
    }

    /// Get the effective model: configured model or provider default
    pub fn effective_model(&self) -> Option<String> {
        self.model
            .clone()
            .or_else(|| self.provider.default_model().map(String::from))
    }

    /// Get the Ollama base URL (defaults to localhost)
    pub fn ollama_base_url(&self) -> String {
        self.ollama_url
            .clone()
            .unwrap_or_else(|| "http://localhost:11434".to_string())
    }
}

/// Top-level cosq configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Cosmos DB account details
    pub account: AccountConfig,

    /// Default database name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,

    /// Default container name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,

    /// AI configuration for natural language query generation
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<AiConfig>,
}

impl Config {
    /// Return the path to the config file: `<config_dir>/cosq/config.yaml`.
    pub fn path() -> Result<PathBuf, ConfigError> {
        dirs::config_dir()
            .map(|d| d.join(APP_DIR).join(FILENAME))
            .ok_or(ConfigError::NoConfigDir)
    }

    /// Load the config from the standard location.
    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::path()?;
        Self::load_from(&path)
    }

    /// Load config from a specific path.
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ConfigError::NotFound
            } else {
                ConfigError::Read(e)
            }
        })?;
        let config: Config = serde_yaml::from_str(&contents)?;
        Ok(config)
    }

    /// Save the config to the standard location, creating the directory if needed.
    pub fn save(&self) -> Result<PathBuf, ConfigError> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let yaml = serde_yaml::to_string(self)?;
        std::fs::write(&path, yaml)?;
        Ok(path)
    }

    /// Save config to a specific path.
    pub fn save_to(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let yaml = serde_yaml::to_string(self)?;
        std::fs::write(path, yaml)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_path_is_under_config_dir() {
        let path = Config::path().unwrap();
        assert!(path.ends_with("cosq/config.yaml"));
    }

    #[test]
    fn test_config_roundtrip() {
        let config = Config {
            account: AccountConfig {
                name: "test-account".into(),
                subscription: "sub-123".into(),
                resource_group: "rg-test".into(),
                endpoint: "https://test-account.documents.azure.com:443/".into(),
            },
            database: None,
            container: None,
            ai: None,
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: Config = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.account.name, "test-account");
        assert_eq!(
            parsed.account.endpoint,
            "https://test-account.documents.azure.com:443/"
        );
        assert!(parsed.database.is_none());
        assert!(parsed.container.is_none());
    }

    #[test]
    fn test_config_roundtrip_with_database_container() {
        let config = Config {
            account: AccountConfig {
                name: "test-account".into(),
                subscription: "sub-123".into(),
                resource_group: "rg-test".into(),
                endpoint: "https://test-account.documents.azure.com:443/".into(),
            },
            database: Some("mydb".into()),
            container: Some("users".into()),
            ai: None,
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: Config = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.database.as_deref(), Some("mydb"));
        assert_eq!(parsed.container.as_deref(), Some("users"));
    }

    #[test]
    fn test_config_backward_compat() {
        let yaml = r#"
account:
  name: old-account
  subscription: sub-old
  resource_group: rg-old
  endpoint: https://old-account.documents.azure.com:443/
"#;
        let parsed: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(parsed.account.name, "old-account");
        assert!(parsed.database.is_none());
        assert!(parsed.container.is_none());
    }

    #[test]
    fn test_config_skip_serializing_none() {
        let config = Config {
            account: AccountConfig {
                name: "test".into(),
                subscription: "sub".into(),
                resource_group: "rg".into(),
                endpoint: "https://test.documents.azure.com:443/".into(),
            },
            database: None,
            container: None,
            ai: None,
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(!yaml.contains("database"));
        assert!(!yaml.contains("container"));
    }

    #[test]
    fn test_config_save_and_load_from() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let config = Config {
            account: AccountConfig {
                name: "my-cosmos".into(),
                subscription: "sub-abc".into(),
                resource_group: "rg-prod".into(),
                endpoint: "https://my-cosmos.documents.azure.com:443/".into(),
            },
            database: Some("testdb".into()),
            container: None,
            ai: None,
        };

        config.save_to(&path).unwrap();
        assert!(path.exists());

        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.account.name, "my-cosmos");
        assert_eq!(loaded.database.as_deref(), Some("testdb"));
        assert!(loaded.container.is_none());
    }

    #[test]
    fn test_config_load_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.yaml");
        let result = Config::load_from(&path);
        assert!(matches!(result, Err(ConfigError::NotFound)));
    }

    #[test]
    fn test_config_save_to_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("dir").join("config.yaml");
        let config = Config {
            account: AccountConfig {
                name: "test".into(),
                subscription: "sub".into(),
                resource_group: "rg".into(),
                endpoint: "https://test.documents.azure.com:443/".into(),
            },
            database: None,
            container: None,
            ai: None,
        };

        config.save_to(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_config_with_ai() {
        let config = Config {
            account: AccountConfig {
                name: "test".into(),
                subscription: "sub".into(),
                resource_group: "rg".into(),
                endpoint: "https://test.documents.azure.com:443/".into(),
            },
            database: None,
            container: None,
            ai: Some(AiConfig {
                provider: AiProvider::AzureOpenai,
                model: None,
                account: Some("my-ai".into()),
                deployment: Some("gpt-4o-mini".into()),
                endpoint: None,
                subscription: None,
                resource_group: None,
                api_version: "2024-12-01-preview".into(),
                ollama_url: None,
            }),
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("ai:"));
        assert!(yaml.contains("gpt-4o-mini"));

        let parsed: Config = serde_yaml::from_str(&yaml).unwrap();
        let ai = parsed.ai.unwrap();
        assert_eq!(ai.account.as_deref(), Some("my-ai"));
        assert_eq!(ai.deployment.as_deref(), Some("gpt-4o-mini"));
    }

    #[test]
    fn test_ai_config_endpoint() {
        let config = AiConfig {
            provider: AiProvider::AzureOpenai,
            model: None,
            account: Some("my-ai-services".into()),
            deployment: Some("gpt-4o-mini".into()),
            endpoint: None,
            subscription: None,
            resource_group: None,
            api_version: "2024-12-01-preview".into(),
            ollama_url: None,
        };
        assert_eq!(
            config.openai_endpoint().as_deref(),
            Some("https://my-ai-services.openai.azure.com")
        );
    }

    #[test]
    fn test_ai_config_endpoint_override() {
        let config = AiConfig {
            provider: AiProvider::AzureOpenai,
            model: None,
            account: Some("my-ai-services".into()),
            deployment: Some("gpt-4o-mini".into()),
            endpoint: Some("https://custom.openai.azure.com/".into()),
            subscription: None,
            resource_group: None,
            api_version: "2024-12-01-preview".into(),
            ollama_url: None,
        };
        assert_eq!(
            config.openai_endpoint().as_deref(),
            Some("https://custom.openai.azure.com")
        );
    }

    #[test]
    fn test_config_backward_compat_no_ai() {
        let yaml = r#"
account:
  name: old-account
  subscription: sub-old
  resource_group: rg-old
  endpoint: https://old-account.documents.azure.com:443/
database: mydb
"#;
        let parsed: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(parsed.ai.is_none());
    }

    #[test]
    fn test_config_backward_compat_azure_ai() {
        // Existing configs without provider field should default to azure-openai
        let yaml = r#"
account:
  name: test
  subscription: sub
  resource_group: rg
  endpoint: https://test.documents.azure.com:443/
ai:
  account: my-ai
  deployment: gpt-4o-mini
"#;
        let parsed: Config = serde_yaml::from_str(yaml).unwrap();
        let ai = parsed.ai.unwrap();
        assert_eq!(ai.provider, AiProvider::AzureOpenai);
        assert_eq!(ai.account.as_deref(), Some("my-ai"));
        assert_eq!(ai.deployment.as_deref(), Some("gpt-4o-mini"));
    }

    #[test]
    fn test_config_local_agent_provider() {
        let yaml = r#"
account:
  name: test
  subscription: sub
  resource_group: rg
  endpoint: https://test.documents.azure.com:443/
ai:
  provider: claude
  model: sonnet
"#;
        let parsed: Config = serde_yaml::from_str(yaml).unwrap();
        let ai = parsed.ai.unwrap();
        assert_eq!(ai.provider, AiProvider::Claude);
        assert_eq!(ai.effective_model().as_deref(), Some("sonnet"));
        assert!(ai.account.is_none());
    }

    #[test]
    fn test_config_ollama_provider() {
        let yaml = r#"
account:
  name: test
  subscription: sub
  resource_group: rg
  endpoint: https://test.documents.azure.com:443/
ai:
  provider: ollama
  model: gemma3:4b
  ollama_url: http://my-server:11434
"#;
        let parsed: Config = serde_yaml::from_str(yaml).unwrap();
        let ai = parsed.ai.unwrap();
        assert_eq!(ai.provider, AiProvider::Ollama);
        assert_eq!(ai.effective_model().as_deref(), Some("gemma3:4b"));
        assert_eq!(ai.ollama_base_url(), "http://my-server:11434");
    }

    #[test]
    fn test_ai_provider_defaults() {
        assert_eq!(AiProvider::Claude.default_model(), Some("sonnet"));
        assert_eq!(AiProvider::Codex.default_model(), Some("o4-mini"));
        assert_eq!(AiProvider::Copilot.default_model(), Some("gpt-4.1"));
        assert_eq!(AiProvider::Ollama.default_model(), None);
        assert_eq!(AiProvider::AzureOpenai.default_model(), None);
    }

    #[test]
    fn test_effective_model_configured() {
        let config = AiConfig {
            provider: AiProvider::Claude,
            model: Some("opus".into()),
            account: None,
            deployment: None,
            endpoint: None,
            subscription: None,
            resource_group: None,
            api_version: "2024-12-01-preview".into(),
            ollama_url: None,
        };
        assert_eq!(config.effective_model().as_deref(), Some("opus"));
    }

    #[test]
    fn test_effective_model_default() {
        let config = AiConfig {
            provider: AiProvider::Claude,
            model: None,
            account: None,
            deployment: None,
            endpoint: None,
            subscription: None,
            resource_group: None,
            api_version: "2024-12-01-preview".into(),
            ollama_url: None,
        };
        assert_eq!(config.effective_model().as_deref(), Some("sonnet"));
    }
}
