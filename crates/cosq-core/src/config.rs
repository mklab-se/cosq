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

    #[error(
        "no profile selected and no default set (available: {0}) — pass --profile or set default_profile"
    )]
    NoProfile(String),

    #[error("unknown profile '{0}' (available: {1})")]
    UnknownProfile(String, String),

    #[error(
        "the config format changed in cosq 1.0 (named profiles) — run `cosq init` to recreate ~/.config/cosq/config.yaml"
    )]
    OldFormat,
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

/// One named account profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// Cosmos DB account details
    pub account: AccountConfig,

    /// Default database name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,

    /// Default container name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,

    /// Per-container ailloy embedding-node mapping (used by `cosq search`).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub embed_models: std::collections::BTreeMap<String, String>,
}

/// Top-level cosq configuration: named profiles + a default selection.
///
/// Profile resolution: `--profile` flag > `COSQ_PROFILE` env > `default_profile`
/// > the sole profile when exactly one exists.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,

    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub profiles: std::collections::BTreeMap<String, Profile>,
}

impl Config {
    /// Resolve the active profile.
    pub fn active(&self, selected: Option<&str>) -> Result<(&str, &Profile), ConfigError> {
        let from_env = std::env::var("COSQ_PROFILE").ok();
        let name = selected
            .map(str::to_string)
            .or(from_env)
            .or_else(|| self.default_profile.clone())
            .or_else(|| {
                (self.profiles.len() == 1).then(|| self.profiles.keys().next().unwrap().clone())
            })
            .ok_or_else(|| ConfigError::NoProfile(self.profile_names()))?;
        let (key, profile) = self
            .profiles
            .get_key_value(&name)
            .ok_or_else(|| ConfigError::UnknownProfile(name.clone(), self.profile_names()))?;
        Ok((key.as_str(), profile))
    }

    /// Mutable access to the active profile (same resolution as [`active`]).
    pub fn active_mut(
        &mut self,
        selected: Option<&str>,
    ) -> Result<(String, &mut Profile), ConfigError> {
        let (name, _) = self.active(selected)?;
        let name = name.to_string();
        let profile = self.profiles.get_mut(&name).expect("resolved above");
        Ok((name, profile))
    }

    fn profile_names(&self) -> String {
        if self.profiles.is_empty() {
            "none configured — run `cosq init`".to_string()
        } else {
            self.profiles.keys().cloned().collect::<Vec<_>>().join(", ")
        }
    }
}

impl Config {
    /// Return the path to the config file: `<config_dir>/cosq/config.yaml`.
    /// `COSQ_CONFIG_DIR` overrides the directory (tests, isolated setups).
    pub fn path() -> Result<PathBuf, ConfigError> {
        if let Ok(dir) = std::env::var("COSQ_CONFIG_DIR") {
            return Ok(PathBuf::from(dir).join(FILENAME));
        }
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
        // Old (pre-1.0) config had a top-level `account:` — point users at init.
        if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&contents) {
            if value.get("account").is_some() && value.get("profiles").is_none() {
                return Err(ConfigError::OldFormat);
            }
        }
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
    fn test_config_load_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.yaml");
        let result = Config::load_from(&path);
        assert!(matches!(result, Err(ConfigError::NotFound)));
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;

    fn cfg(names: &[&str], default: Option<&str>) -> Config {
        let mut profiles = std::collections::BTreeMap::new();
        for n in names {
            profiles.insert(
                n.to_string(),
                Profile {
                    account: AccountConfig {
                        name: format!("{n}-acct"),
                        subscription: "s".into(),
                        resource_group: "rg".into(),
                        endpoint: format!("https://{n}.documents.azure.com"),
                    },
                    database: None,
                    container: None,
                    embed_models: Default::default(),
                },
            );
        }
        Config {
            default_profile: default.map(str::to_string),
            profiles,
        }
    }

    #[test]
    fn explicit_selection_wins() {
        let c = cfg(&["work", "demo"], Some("work"));
        assert_eq!(c.active(Some("demo")).unwrap().0, "demo");
    }

    #[test]
    fn default_profile_used_when_unselected() {
        let c = cfg(&["work", "demo"], Some("demo"));
        assert_eq!(c.active(None).unwrap().0, "demo");
    }

    #[test]
    fn sole_profile_is_implicit_default() {
        let c = cfg(&["only"], None);
        assert_eq!(c.active(None).unwrap().0, "only");
    }

    #[test]
    fn errors_list_available_profiles() {
        let c = cfg(&["work", "demo"], None);
        let err = c.active(Some("nope")).unwrap_err().to_string();
        assert!(err.contains("demo") && err.contains("work"));
        let err = c.active(None).unwrap_err().to_string();
        assert!(err.contains("--profile"));
    }

    #[test]
    fn old_format_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.yaml");
        std::fs::write(
            &path,
            "account:\n  name: a\n  subscription: s\n  resource_group: r\n  endpoint: e\n",
        )
        .unwrap();
        let err = Config::load_from(&path).unwrap_err().to_string();
        assert!(err.contains("cosq init"), "{err}");
    }

    #[test]
    fn round_trip() {
        let c = cfg(&["work"], Some("work"));
        let yaml = serde_yaml::to_string(&c).unwrap();
        let back: Config = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back.default_profile.as_deref(), Some("work"));
        assert!(back.profiles.contains_key("work"));
    }
}
