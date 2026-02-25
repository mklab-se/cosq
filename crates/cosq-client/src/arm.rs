//! Azure Resource Manager (ARM) client for discovering Cosmos DB accounts

use serde::Deserialize;
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
            return Err(ClientError::Api {
                status: status.as_u16(),
                message: body,
            });
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
            return Err(ClientError::Api {
                status: status.as_u16(),
                message: body,
            });
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
}
