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
    pub async fn get_token(resource: &str) -> Result<String, ClientError> {
        let output = Command::new("az")
            .args([
                "account",
                "get-access-token",
                "--resource",
                resource,
                "--query",
                "accessToken",
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
                format!("failed to get access token: {}", stderr.trim()),
                "Try running `az login` to refresh your credentials",
            ));
        }

        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if token.is_empty() {
            return Err(ClientError::auth("received empty access token"));
        }

        Ok(token)
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
