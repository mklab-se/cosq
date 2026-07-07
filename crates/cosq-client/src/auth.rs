//! Azure authentication via the Azure CLI
//!
//! Uses `az account get-access-token` to acquire tokens for Azure Resource Manager
//! and Cosmos DB data plane access.

use serde::Deserialize;
use tokio::process::Command;

use crate::error::ClientError;

/// Cosmos DB data plane resource scope
pub const COSMOS_RESOURCE: &str = "https://cosmos.azure.com";

/// Azure Resource Manager resource scope
pub const ARM_RESOURCE: &str = "https://management.azure.com";

/// Status of the current Azure CLI authentication session
#[derive(Debug, Clone)]
pub struct AuthStatus {
    pub logged_in: bool,
    pub user: Option<String>,
    pub subscription_name: Option<String>,
    pub subscription_id: Option<String>,
    pub tenant_id: Option<String>,
}

/// Azure CLI account info
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AzAccountInfo {
    user: AzUser,
    name: String,
    id: String,
    tenant_id: String,
}

#[derive(Debug, Deserialize)]
struct AzUser {
    name: String,
}

/// Azure CLI-based authentication provider.
pub struct AzCliAuth;

impl AzCliAuth {
    /// Check the current Azure CLI login status.
    pub async fn check_status() -> Result<AuthStatus, ClientError> {
        let output = Command::new("az")
            .args(["account", "show", "--output", "json"])
            .output()
            .await
            .map_err(|e| {
                ClientError::az_cli(
                    format!("failed to run `az` command: {e}"),
                    "Install the Azure CLI: https://aka.ms/install-azure-cli",
                )
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("az login") || stderr.contains("not logged in") {
                return Ok(AuthStatus {
                    logged_in: false,
                    user: None,
                    subscription_name: None,
                    subscription_id: None,
                    tenant_id: None,
                });
            }
            return Err(ClientError::az_cli(
                stderr.trim().to_string(),
                "Try running `az login` first",
            ));
        }

        let info: AzAccountInfo =
            serde_json::from_slice(&output.stdout).map_err(|e| ClientError::auth(e.to_string()))?;

        Ok(AuthStatus {
            logged_in: true,
            user: Some(info.user.name),
            subscription_name: Some(info.name),
            subscription_id: Some(info.id),
            tenant_id: Some(info.tenant_id),
        })
    }

    /// Get an access token for the specified resource.
    /// Acquire a token for `resource`, using an on-disk cache
    /// (`~/.cache/cosq/tokens.json`, 0600) so `az` is invoked only when the
    /// cached token is missing or within 5 minutes of expiry. Override the
    /// cache directory with `COSQ_CACHE_DIR`; force a fresh token by deleting
    /// the cache file.
    pub async fn get_token(resource: &str) -> Result<String, ClientError> {
        Self::get_token_with(resource, &AzTokenSource).await
    }

    /// `get_token` with an injectable source (tests).
    pub async fn get_token_with(
        resource: &str,
        source: &dyn TokenSource,
    ) -> Result<String, ClientError> {
        let cache_path = token_cache_path();
        if let Some(token) = read_cached_token(&cache_path, resource) {
            return Ok(token);
        }
        let info = source.fetch(resource).await?;
        write_cached_token(&cache_path, resource, &info);
        Ok(info.token)
    }

    /// Run `az login` interactively.
    pub async fn login() -> Result<(), ClientError> {
        let status = Command::new("az")
            .args(["login"])
            .status()
            .await
            .map_err(|e| {
                ClientError::az_cli(
                    format!("failed to run `az login`: {e}"),
                    "Install the Azure CLI: https://aka.ms/install-azure-cli",
                )
            })?;

        if !status.success() {
            return Err(ClientError::auth("az login failed"));
        }

        Ok(())
    }

    /// Get the signed-in user's principal (object) ID from Azure AD.
    pub async fn get_principal_id() -> Result<String, ClientError> {
        let output = Command::new("az")
            .args([
                "ad",
                "signed-in-user",
                "show",
                "--query",
                "id",
                "--output",
                "tsv",
            ])
            .output()
            .await
            .map_err(|e| {
                ClientError::az_cli(
                    format!("failed to run `az` command: {e}"),
                    "Install the Azure CLI: https://aka.ms/install-azure-cli",
                )
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ClientError::az_cli(
                format!("failed to get principal ID: {}", stderr.trim()),
                "Try running `az login` to refresh your credentials",
            ));
        }

        let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if id.is_empty() {
            return Err(ClientError::auth("received empty principal ID"));
        }

        Ok(id)
    }

    /// Run `az logout`.
    pub async fn logout() -> Result<(), ClientError> {
        let status = Command::new("az")
            .args(["logout"])
            .status()
            .await
            .map_err(|e| {
                ClientError::az_cli(
                    format!("failed to run `az logout`: {e}"),
                    "Install the Azure CLI: https://aka.ms/install-azure-cli",
                )
            })?;

        if !status.success() {
            return Err(ClientError::auth("az logout failed"));
        }

        Ok(())
    }
}

/// A fetched token and its expiry (unix seconds).
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct TokenInfo {
    pub token: String,
    pub expires_on: i64,
}

/// Source of fresh tokens — the seam that lets tests avoid the az CLI.
#[async_trait::async_trait]
pub trait TokenSource: Send + Sync {
    async fn fetch(&self, resource: &str) -> Result<TokenInfo, ClientError>;
}

/// The real source: `az account get-access-token`.
pub struct AzTokenSource;

#[async_trait::async_trait]
impl TokenSource for AzTokenSource {
    async fn fetch(&self, resource: &str) -> Result<TokenInfo, ClientError> {
        let output = Command::new("az")
            .args([
                "account",
                "get-access-token",
                "--resource",
                resource,
                "--output",
                "json",
            ])
            .output()
            .await
            .map_err(|e| {
                ClientError::az_cli(
                    format!("failed to run `az` command: {e}"),
                    "Install the Azure CLI: https://aka.ms/install-azure-cli",
                )
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ClientError::az_cli(
                format!("failed to get access token: {}", stderr.trim()),
                "Try running `az login` to refresh your credentials",
            ));
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct AzToken {
            access_token: String,
            expires_on: Option<i64>,
        }
        let parsed: AzToken = serde_json::from_slice(&output.stdout)
            .map_err(|e| ClientError::auth(format!("unexpected az token output: {e}")))?;
        if parsed.access_token.is_empty() {
            return Err(ClientError::auth(
                "az returned an empty access token".to_string(),
            ));
        }
        Ok(TokenInfo {
            token: parsed.access_token,
            // fall back to a 50-minute assumption when az omits expiry
            expires_on: parsed
                .expires_on
                .unwrap_or_else(|| chrono::Utc::now().timestamp() + 3000),
        })
    }
}

/// Expiry skew: refresh tokens this many seconds before they actually expire.
const EXPIRY_SKEW_SECS: i64 = 300;

fn token_cache_path() -> std::path::PathBuf {
    let dir = std::env::var("COSQ_CACHE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::cache_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("cosq")
        });
    dir.join("tokens.json")
}

fn read_cached_token(path: &std::path::Path, resource: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let map: std::collections::BTreeMap<String, TokenInfo> = serde_json::from_str(&text).ok()?;
    let info = map.get(resource)?;
    let now = chrono::Utc::now().timestamp();
    (info.expires_on - EXPIRY_SKEW_SECS > now).then(|| info.token.clone())
}

fn write_cached_token(path: &std::path::Path, resource: &str, info: &TokenInfo) {
    let mut map: std::collections::BTreeMap<String, TokenInfo> = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    map.insert(resource.to_string(), info.clone());
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(&map) {
        let _ = std::fs::write(path, json);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
    }
}

#[cfg(test)]
mod token_cache_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingSource {
        calls: AtomicUsize,
        expires_on: i64,
    }

    #[async_trait::async_trait]
    impl TokenSource for CountingSource {
        async fn fetch(&self, _resource: &str) -> Result<TokenInfo, ClientError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(TokenInfo {
                token: format!("token-{n}"),
                expires_on: self.expires_on,
            })
        }
    }

    #[tokio::test]
    async fn caches_until_expiry_and_refreshes_after() {
        let tmp = tempfile::tempdir().unwrap();
        // scoped env var: serialize via unique resource names instead of env races
        unsafe { std::env::set_var("COSQ_CACHE_DIR", tmp.path()) };

        let fresh = CountingSource {
            calls: AtomicUsize::new(0),
            expires_on: chrono::Utc::now().timestamp() + 3600,
        };
        let t1 = AzCliAuth::get_token_with("res-a", &fresh).await.unwrap();
        let t2 = AzCliAuth::get_token_with("res-a", &fresh).await.unwrap();
        assert_eq!(t1, "token-1");
        assert_eq!(t2, "token-1", "second call must hit the cache");
        assert_eq!(fresh.calls.load(Ordering::SeqCst), 1);

        // an (almost) expired token is refreshed
        let stale = CountingSource {
            calls: AtomicUsize::new(0),
            expires_on: chrono::Utc::now().timestamp() + 10, // inside skew
        };
        let t3 = AzCliAuth::get_token_with("res-b", &stale).await.unwrap();
        let t4 = AzCliAuth::get_token_with("res-b", &stale).await.unwrap();
        assert_eq!(t3, "token-1");
        assert_eq!(t4, "token-2", "stale token must refetch");

        // permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(tmp.path().join("tokens.json"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        unsafe { std::env::remove_var("COSQ_CACHE_DIR") };
    }
}
