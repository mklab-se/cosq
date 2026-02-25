//! Azure Resource Manager (ARM) client for discovering Cosmos DB accounts

use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::auth::{ARM_RESOURCE, AzCliAuth};
use crate::error::ClientError;

const ARM_SUBSCRIPTIONS_API_VERSION: &str = "2024-11-01";
const COSMOS_DB_API_VERSION: &str = "2025-04-15";
const ARM_BASE_URL: &str = "https://management.azure.com";

/// An Azure subscription
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub subscription_id: String,
    pub display_name: String,
    pub state: String,
}

#[derive(Debug, Deserialize)]
struct SubscriptionListResponse {
    value: Vec<Subscription>,
}

/// A Cosmos DB account discovered via ARM
#[derive(Debug, Clone)]
pub struct CosmosAccount {
    pub name: String,
    pub location: String,
    pub kind: Option<String>,
    pub endpoint: String,
    pub resource_group: String,
    pub id: String,
}

#[derive(Debug, Deserialize)]
struct CosmosAccountListResponse {
    value: Vec<CosmosAccountResource>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CosmosAccountResource {
    id: String,
    name: String,
    location: String,
    kind: Option<String>,
    properties: CosmosAccountProperties,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CosmosAccountProperties {
    document_endpoint: Option<String>,
}

/// ARM client for discovering Azure resources.
pub struct ArmClient {
    http: reqwest::Client,
    token: String,
}

impl ArmClient {
    /// Create a new ARM client, acquiring a token via the Azure CLI.
    pub async fn new() -> Result<Self, ClientError> {
        let token = AzCliAuth::get_token(ARM_RESOURCE).await?;
        Ok(Self {
            http: reqwest::Client::new(),
            token,
        })
    }

    /// List all enabled Azure subscriptions.
    pub async fn list_subscriptions(&self) -> Result<Vec<Subscription>, ClientError> {
        debug!("listing Azure subscriptions");

        let url =
            format!("{ARM_BASE_URL}/subscriptions?api-version={ARM_SUBSCRIPTIONS_API_VERSION}");
        let resp = self.http.get(&url).bearer_auth(&self.token).send().await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::api(status.as_u16(), body));
        }

        let list: SubscriptionListResponse = resp.json().await?;
        let enabled: Vec<Subscription> = list
            .value
            .into_iter()
            .filter(|s| s.state == "Enabled")
            .collect();

        debug!(count = enabled.len(), "found enabled subscriptions");
        Ok(enabled)
    }

    /// List Cosmos DB accounts in a given subscription.
    pub async fn list_cosmos_accounts(
        &self,
        subscription_id: &str,
    ) -> Result<Vec<CosmosAccount>, ClientError> {
        debug!(subscription_id, "listing Cosmos DB accounts");

        let url = format!(
            "{ARM_BASE_URL}/subscriptions/{subscription_id}/providers/Microsoft.DocumentDB/databaseAccounts?api-version={COSMOS_DB_API_VERSION}"
        );

        let resp = self.http.get(&url).bearer_auth(&self.token).send().await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            if status.as_u16() == 403 {
                return Err(ClientError::forbidden(
                    body,
                    "You may not have Reader access on this subscription. Check your Azure RBAC roles.",
                ));
            }
            return Err(ClientError::api(status.as_u16(), body));
        }

        let list: CosmosAccountListResponse = resp.json().await?;
        let accounts: Vec<CosmosAccount> = list
            .value
            .into_iter()
            .map(|r| {
                // Extract resource group from the resource ID
                // Format: /subscriptions/.../resourceGroups/<rg>/providers/...
                let resource_group =
                    r.id.split('/')
                        .collect::<Vec<_>>()
                        .windows(2)
                        .find(|w| w[0].eq_ignore_ascii_case("resourceGroups"))
                        .map(|w| w[1].to_string())
                        .unwrap_or_default();

                CosmosAccount {
                    name: r.name,
                    location: r.location,
                    kind: r.kind,
                    endpoint: r.properties.document_endpoint.unwrap_or_default(),
                    resource_group,
                    id: r.id,
                }
            })
            .collect();

        debug!(count = accounts.len(), "found Cosmos DB accounts");
        Ok(accounts)
    }

    /// Check if a principal has any Cosmos DB SQL role assignment on the account.
    pub async fn has_cosmos_data_role(
        &self,
        account_resource_id: &str,
        principal_id: &str,
    ) -> Result<bool, ClientError> {
        debug!(principal_id, "checking Cosmos DB SQL role assignments");

        let url = format!(
            "{ARM_BASE_URL}{account_resource_id}/sqlRoleAssignments?api-version={COSMOS_DB_API_VERSION}"
        );
        let resp = self.http.get(&url).bearer_auth(&self.token).send().await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::api(status.as_u16(), body));
        }

        let list: SqlRoleAssignmentListResponse = resp.json().await?;
        let has_role = list
            .value
            .iter()
            .any(|a| a.properties.principal_id == principal_id);

        debug!(has_role, "data plane role check complete");
        Ok(has_role)
    }

    /// Assign the Cosmos DB Built-in Data Contributor role to a principal.
    pub async fn assign_cosmos_data_contributor(
        &self,
        account_resource_id: &str,
        principal_id: &str,
    ) -> Result<(), ClientError> {
        debug!(principal_id, "assigning Cosmos DB data contributor role");

        let assignment_id = uuid::Uuid::new_v4().to_string();
        let url = format!(
            "{ARM_BASE_URL}{account_resource_id}/sqlRoleAssignments/{assignment_id}?api-version={COSMOS_DB_API_VERSION}"
        );

        let body = SqlRoleAssignmentCreateBody {
            properties: SqlRoleAssignmentCreateProperties {
                role_definition_id: format!(
                    "{account_resource_id}/sqlRoleDefinitions/{COSMOS_DATA_CONTRIBUTOR_ROLE}"
                ),
                scope: account_resource_id.to_string(),
                principal_id: principal_id.to_string(),
            },
        };

        let resp = self
            .http
            .put(&url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let resp_body = resp.text().await.unwrap_or_default();
            if status.as_u16() == 403 {
                return Err(ClientError::forbidden(
                    resp_body,
                    "You need Owner or User Access Administrator role on the Cosmos DB account to assign data plane roles.",
                ));
            }
            return Err(ClientError::api(status.as_u16(), resp_body));
        }

        debug!("data contributor role assigned successfully");
        Ok(())
    }
}

/// Cosmos DB Built-in Data Contributor role definition ID
const COSMOS_DATA_CONTRIBUTOR_ROLE: &str = "00000000-0000-0000-0000-000000000002";

#[derive(Debug, Deserialize)]
struct SqlRoleAssignmentListResponse {
    value: Vec<SqlRoleAssignment>,
}

#[derive(Debug, Deserialize)]
struct SqlRoleAssignment {
    properties: SqlRoleAssignmentProperties,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SqlRoleAssignmentProperties {
    principal_id: String,
}

#[derive(Debug, Serialize)]
struct SqlRoleAssignmentCreateBody {
    properties: SqlRoleAssignmentCreateProperties,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SqlRoleAssignmentCreateProperties {
    role_definition_id: String,
    scope: String,
    principal_id: String,
}
