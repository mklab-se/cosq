//! Cosmos DB data plane client
//!
//! Executes SQL queries against Cosmos DB containers using the REST API
//! with AAD token authentication. Handles cross-partition queries by
//! fetching partition key ranges and fanning out the query.

use serde::Deserialize;
use serde_json::Value;
use tracing::debug;

use crate::auth::{AzCliAuth, COSMOS_RESOURCE};
use crate::error::ClientError;

const API_VERSION: &str = "2018-12-31";

/// Result of a Cosmos DB SQL query
#[derive(Debug)]
pub struct QueryResult {
    pub documents: Vec<Value>,
    pub request_charge: f64,
}

/// Cosmos DB REST API response for queries
#[derive(Debug, Deserialize)]
struct QueryResponse {
    #[serde(rename = "Documents")]
    documents: Vec<Value>,
}

/// Cosmos DB REST API response for listing databases
#[derive(Debug, Deserialize)]
struct DatabaseListResponse {
    #[serde(rename = "Databases")]
    databases: Vec<DatabaseEntry>,
}

#[derive(Debug, Deserialize)]
struct DatabaseEntry {
    id: String,
}

/// Cosmos DB REST API response for listing collections
#[derive(Debug, Deserialize)]
struct CollectionListResponse {
    #[serde(rename = "DocumentCollections")]
    document_collections: Vec<CollectionEntry>,
}

#[derive(Debug, Deserialize)]
struct CollectionEntry {
    id: String,
}

/// Partition key range info from the pkranges endpoint
#[derive(Debug, Deserialize)]
struct PartitionKeyRangesResponse {
    #[serde(rename = "PartitionKeyRanges")]
    partition_key_ranges: Vec<PartitionKeyRange>,
}

#[derive(Debug, Deserialize)]
struct PartitionKeyRange {
    id: String,
}

/// Client for the Cosmos DB data plane REST API.
pub struct CosmosClient {
    http: reqwest::Client,
    endpoint: String,
    token: String,
}

impl CosmosClient {
    /// Create a new Cosmos client, acquiring a Cosmos DB token via the Azure CLI.
    pub async fn new(endpoint: &str) -> Result<Self, ClientError> {
        let token = AzCliAuth::get_token(COSMOS_RESOURCE).await?;
        let endpoint = endpoint.trim_end_matches('/').to_string();
        Ok(Self {
            http: reqwest::Client::new(),
            endpoint,
            token,
        })
    }

    /// Build the Authorization header value for AAD token auth.
    fn auth_header(&self) -> String {
        let sig = urlencoding::encode(&self.token);
        format!("type%3Daad%26ver%3D1.0%26sig%3D{sig}")
    }

    /// Build the x-ms-date header value in RFC 1123 format.
    fn date_header() -> String {
        chrono::Utc::now()
            .format("%a, %d %b %Y %H:%M:%S GMT")
            .to_string()
    }

    /// List all databases in the Cosmos DB account.
    pub async fn list_databases(&self) -> Result<Vec<String>, ClientError> {
        debug!("listing databases");
        let url = format!("{}/dbs", self.endpoint);
        let date = Self::date_header();

        let resp = self
            .http
            .get(&url)
            .header("Authorization", self.auth_header())
            .header("x-ms-date", &date)
            .header("x-ms-version", API_VERSION)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            if status.as_u16() == 403 {
                return Err(ClientError::forbidden(
                    body,
                    "You may not have data plane access. Check your Cosmos DB RBAC roles.",
                ));
            }
            return Err(ClientError::api(status.as_u16(), body));
        }

        let list: DatabaseListResponse = resp.json().await?;
        let names: Vec<String> = list.databases.into_iter().map(|d| d.id).collect();
        debug!(count = names.len(), "found databases");
        Ok(names)
    }

    /// List all containers in a database.
    pub async fn list_containers(&self, database: &str) -> Result<Vec<String>, ClientError> {
        debug!(database, "listing containers");
        let url = format!("{}/dbs/{}/colls", self.endpoint, database);
        let date = Self::date_header();

        let resp = self
            .http
            .get(&url)
            .header("Authorization", self.auth_header())
            .header("x-ms-date", &date)
            .header("x-ms-version", API_VERSION)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::api(status.as_u16(), body));
        }

        let list: CollectionListResponse = resp.json().await?;
        let names: Vec<String> = list
            .document_collections
            .into_iter()
            .map(|c| c.id)
            .collect();
        debug!(count = names.len(), "found containers");
        Ok(names)
    }

    /// Get partition key ranges for a container.
    async fn get_partition_key_ranges(
        &self,
        database: &str,
        container: &str,
    ) -> Result<Vec<String>, ClientError> {
        let url = format!(
            "{}/dbs/{}/colls/{}/pkranges",
            self.endpoint, database, container
        );
        let date = Self::date_header();

        let resp = self
            .http
            .get(&url)
            .header("Authorization", self.auth_header())
            .header("x-ms-date", &date)
            .header("x-ms-version", API_VERSION)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::api(status.as_u16(), body));
        }

        let ranges: PartitionKeyRangesResponse = resp.json().await?;
        let ids: Vec<String> = ranges
            .partition_key_ranges
            .into_iter()
            .map(|r| r.id)
            .collect();
        debug!(count = ids.len(), "found partition key ranges");
        Ok(ids)
    }

    /// Execute a SQL query against a single partition key range, handling pagination.
    async fn query_partition(
        &self,
        url: &str,
        body: &Value,
        partition_key_range_id: &str,
    ) -> Result<(Vec<Value>, f64), ClientError> {
        let mut documents = Vec::new();
        let mut total_charge = 0.0_f64;
        let mut continuation: Option<String> = None;

        loop {
            let date = Self::date_header();
            let mut request = self
                .http
                .post(url)
                .header("Authorization", self.auth_header())
                .header("x-ms-date", &date)
                .header("x-ms-version", API_VERSION)
                .header("x-ms-documentdb-isquery", "True")
                .header("x-ms-documentdb-query-enablecrosspartition", "True")
                .header(
                    "x-ms-documentdb-partitionkeyrangeid",
                    partition_key_range_id,
                )
                .header("Content-Type", "application/query+json")
                .json(body);

            if let Some(ref token) = continuation {
                request = request.header("x-ms-continuation", token);
            }

            let resp = request.send().await?;
            let status = resp.status();

            if !status.is_success() {
                let body_text = resp.text().await.unwrap_or_default();
                if status.as_u16() == 403 {
                    return Err(ClientError::forbidden(
                        body_text,
                        "You may not have data plane access. Check your Cosmos DB RBAC roles.",
                    ));
                }
                return Err(ClientError::api(status.as_u16(), body_text));
            }

            let next_continuation = resp
                .headers()
                .get("x-ms-continuation")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            let charge: f64 = resp
                .headers()
                .get("x-ms-request-charge")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0);
            total_charge += charge;

            let query_resp: QueryResponse = resp.json().await?;
            documents.extend(query_resp.documents);

            match next_continuation {
                Some(token) if !token.is_empty() => {
                    debug!("continuing with pagination token");
                    continuation = Some(token);
                }
                _ => break,
            }
        }

        Ok((documents, total_charge))
    }

    /// Execute a SQL query against a container, handling cross-partition fanout and pagination.
    pub async fn query(
        &self,
        database: &str,
        container: &str,
        sql: &str,
    ) -> Result<QueryResult, ClientError> {
        self.query_with_params(database, container, sql, Vec::new())
            .await
    }

    /// Execute a parameterized SQL query against a container.
    ///
    /// Parameters should be in Cosmos DB format:
    /// `[{"name": "@param", "value": ...}, ...]`
    pub async fn query_with_params(
        &self,
        database: &str,
        container: &str,
        sql: &str,
        parameters: Vec<Value>,
    ) -> Result<QueryResult, ClientError> {
        debug!(database, container, sql, params = ?parameters, "executing query");

        let url = format!(
            "{}/dbs/{}/colls/{}/docs",
            self.endpoint, database, container
        );
        let body = serde_json::json!({
            "query": sql,
            "parameters": parameters
        });

        // Get partition key ranges and fan out the query
        let ranges = self.get_partition_key_ranges(database, container).await?;
        debug!(count = ranges.len(), "querying across partition key ranges");

        let mut all_documents = Vec::new();
        let mut total_charge = 0.0_f64;

        for range_id in &ranges {
            let (docs, charge) = self.query_partition(&url, &body, range_id).await?;
            debug!(
                range_id,
                docs = docs.len(),
                charge,
                "partition query complete"
            );
            all_documents.extend(docs);
            total_charge += charge;
        }

        debug!(
            count = all_documents.len(),
            request_charge = total_charge,
            "query complete"
        );

        Ok(QueryResult {
            documents: all_documents,
            request_charge: total_charge,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_header_format() {
        let client = CosmosClient {
            http: reqwest::Client::new(),
            endpoint: "https://test.documents.azure.com".into(),
            token: "eyJ0eXAi.test.token".into(),
        };
        let header = client.auth_header();
        assert!(header.starts_with("type%3Daad%26ver%3D1.0%26sig%3D"));
        assert!(header.contains("eyJ0eXAi"));
    }

    #[test]
    fn test_date_header_format() {
        let date = CosmosClient::date_header();
        // Should match RFC 1123 format: "Wed, 09 Nov 2023 12:34:56 GMT"
        assert!(date.ends_with("GMT"));
        assert!(date.len() > 20);
    }

    #[test]
    fn test_query_response_deserialization() {
        let json = r#"{"Documents": [{"id": "1", "name": "Alice"}, {"id": "2", "name": "Bob"}], "_count": 2}"#;
        let resp: QueryResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.documents.len(), 2);
        assert_eq!(resp.documents[0]["id"], "1");
        assert_eq!(resp.documents[1]["name"], "Bob");
    }

    #[test]
    fn test_query_response_empty() {
        let json = r#"{"Documents": [], "_count": 0}"#;
        let resp: QueryResponse = serde_json::from_str(json).unwrap();
        assert!(resp.documents.is_empty());
    }

    #[test]
    fn test_database_list_deserialization() {
        let json = r#"{"Databases": [{"id": "db1", "_rid": "r1"}, {"id": "db2", "_rid": "r2"}]}"#;
        let resp: DatabaseListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.databases.len(), 2);
        assert_eq!(resp.databases[0].id, "db1");
        assert_eq!(resp.databases[1].id, "db2");
    }

    #[test]
    fn test_collection_list_deserialization() {
        let json = r#"{"DocumentCollections": [{"id": "coll1", "_rid": "r1"}, {"id": "coll2", "_rid": "r2"}]}"#;
        let resp: CollectionListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.document_collections.len(), 2);
        assert_eq!(resp.document_collections[0].id, "coll1");
        assert_eq!(resp.document_collections[1].id, "coll2");
    }

    #[test]
    fn test_partition_key_ranges_deserialization() {
        let json =
            r#"{"PartitionKeyRanges": [{"id": "0", "minInclusive": "", "maxExclusive": "FF"}]}"#;
        let resp: PartitionKeyRangesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.partition_key_ranges.len(), 1);
        assert_eq!(resp.partition_key_ranges[0].id, "0");
    }
}
