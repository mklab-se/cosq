//! Configuration file handling for cosq
//!
//! Loads and saves `cosq.yaml` config files, searching the current directory
//! and parent directories.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Config filename
pub const FILENAME: &str = "cosq.yaml";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config: {0}")]
    Read(#[from] std::io::Error),

    #[error("failed to parse config: {0}")]
    Parse(#[from] serde_yaml::Error),

    #[error("no {FILENAME} found in current or parent directories")]
    NotFound,
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

/// Top-level cosq configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Cosmos DB account details
    pub account: AccountConfig,
}

impl Config {
    /// Load config from a specific path.
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path)?;
        let config: Config = serde_yaml::from_str(&contents)?;
        Ok(config)
    }

    /// Save config to a specific path.
    pub fn save_to(&self, path: &Path) -> Result<(), ConfigError> {
        let yaml = serde_yaml::to_string(self)?;
        std::fs::write(path, yaml)?;
        Ok(())
    }

    /// Save config to `cosq.yaml` in the given directory.
    pub fn save(&self, dir: &Path) -> Result<PathBuf, ConfigError> {
        let path = dir.join(FILENAME);
        self.save_to(&path)?;
        Ok(path)
    }

    /// Search for `cosq.yaml` starting from `start_dir` and walking up to parent directories.
    /// Returns the parsed config and the path where it was found.
    pub fn find(start_dir: &Path) -> Result<(Self, PathBuf), ConfigError> {
        let mut dir = start_dir.to_path_buf();
        loop {
            let candidate = dir.join(FILENAME);
            if candidate.is_file() {
                let config = Self::load_from(&candidate)?;
                return Ok((config, candidate));
            }
            if !dir.pop() {
                break;
            }
        }
        Err(ConfigError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_roundtrip() {
        let config = Config {
            account: AccountConfig {
                name: "test-account".into(),
                subscription: "sub-123".into(),
                resource_group: "rg-test".into(),
                endpoint: "https://test-account.documents.azure.com:443/".into(),
            },
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: Config = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.account.name, "test-account");
        assert_eq!(
            parsed.account.endpoint,
            "https://test-account.documents.azure.com:443/"
        );
    }

    #[test]
    fn test_config_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            account: AccountConfig {
                name: "my-cosmos".into(),
                subscription: "sub-abc".into(),
                resource_group: "rg-prod".into(),
                endpoint: "https://my-cosmos.documents.azure.com:443/".into(),
            },
        };

        let path = config.save(dir.path()).unwrap();
        assert!(path.exists());

        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.account.name, "my-cosmos");
    }

    #[test]
    fn test_config_find_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let result = Config::find(dir.path());
        assert!(result.is_err());
    }
}
