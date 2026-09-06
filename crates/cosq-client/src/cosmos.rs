//! Cosmos DB data plane client
//!
//! Executes SQL queries against Cosmos DB containers using the REST API
//! with AAD token authentication. Handles cross-partition queries by
//! fetching partition key ranges and fanning out the query.

use futures_util::{StreamExt, TryStreamExt};
use serde::Deserialize;
use serde_json::Value;
use tracing::debug;

use crate::auth::{AzCliAuth, COSMOS_RESOURCE};
use crate::error::ClientError;

/// Data-plane wire version.
///
/// Spike findings (2026-07-07, live against a serverless account with
/// EnableNoSQLVectorSearch + EnableNoSQLFullTextSearch):
/// - `2018-12-31` and `2020-07-15` behave identically for everything cosq
///   does; we use the newer one.
/// - VectorDistance / ORDER BY RANK / RRF queries are REJECTED by the
///   gateway's naive cross-partition mode ("can not be directly served by
///   the gateway") but EXECUTE FINE per partition-key-range — which is how
///   cosq's fan-out already works — and when pk-scoped.
/// - VectorDistance can be projected (client-side exact merge possible);
///   FullTextScore cannot (SC2240) — cross-partition FTS merges are
///   approximate, pk-scoped/single-partition are exact.
/// - A container with BOTH vector and full-text policies failed to provision
///   on the serverless test account; vector-only and fts-only succeeded.
const API_VERSION: &str = "2020-07-15";

/// Maximum concurrent partition-key-range queries during cross-partition
/// fan-out. Bounded to stay polite to the gateway.
const MAX_PARALLEL_RANGES: usize = 8;

/// Result of a Cosmos DB SQL query
#[derive(Debug)]
pub struct QueryResult {
    pub documents: Vec<Value>,
    pub request_charge: f64,
    /// Per partition-key-range RU charges (range id, RU). Single-partition
    /// and scoped queries have one entry.
    pub per_range: Vec<(String, f64)>,
}

/// Metrics captured from a query for `cosq explain`.
#[derive(Debug, Clone, Default)]
pub struct QueryMetrics {
    /// Per-range raw query metrics (`x-ms-documentdb-query-metrics`,
    /// semicolon-separated key=value pairs).
    pub query_metrics: Vec<(String, String)>,
    /// Per-range index utilization (decoded from base64
    /// `x-ms-cosmos-index-utilization` JSON).
    pub index_metrics: Vec<(String, Value)>,
    pub request_charge: f64,
    pub document_count: usize,
}

/// Execution knobs for queries.
#[derive(Debug, Clone, Default)]
pub struct QueryOptions {
    /// Page size (`x-ms-max-item-count`).
    pub max_item_count: Option<u32>,
    /// Stop after this many documents (across ranges/pages).
    pub first: Option<usize>,
}

/// Container metadata relevant to cosq: partition key + search policies.
#[derive(Debug, Clone, Default)]
pub struct ContainerMeta {
    /// Partition key paths (multiple for hierarchical partition keys).
    pub pk_paths: Vec<String>,
    /// Vector embedding policy entries: (path, dimensions, distance function).
    pub vector_paths: Vec<(String, u32, String)>,
    /// Full-text policy paths.
    pub full_text_paths: Vec<String>,
}

impl ContainerMeta {
    pub fn from_response(raw: &Value) -> Self {
        let pk_paths = raw
            .pointer("/partitionKey/paths")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let vector_paths = raw
            .pointer("/vectorEmbeddingPolicy/vectorEmbeddings")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|e| {
                        Some((
                            e.get("path")?.as_str()?.to_string(),
                            e.get("dimensions")?.as_u64()? as u32,
                            e.get("distanceFunction")
                                .and_then(Value::as_str)
                                .unwrap_or("cosine")
                                .to_string(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let full_text_paths = raw
            .pointer("/fullTextPolicy/fullTextPaths")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|e| e.get("path").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        ContainerMeta {
            pk_paths,
            vector_paths,
            full_text_paths,
        }
    }
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
#[derive(Clone)]
pub struct CosmosClient {
    http: reqwest::Client,
    endpoint: String,
    token: String,
}

impl CosmosClient {
    /// Create a new Cosmos client, acquiring a Cosmos DB token via the Azure CLI.
    pub async fn new(endpoint: &str) -> Result<Self, ClientError> {
        let token = AzCliAuth::get_token(COSMOS_RESOURCE).await?;
        Ok(Self::with_token(endpoint, token))
    }

    /// Create a client with a pre-acquired token (tests, alternate auth).
    pub fn with_token(endpoint: &str, token: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            endpoint: endpoint.trim_end_matches('/').to_string(),
            token,
        }
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

    /// Container metadata (partition key paths + search policies).
    pub async fn get_container(
        &self,
        database: &str,
        container: &str,
    ) -> Result<ContainerMeta, ClientError> {
        let url = format!("{}/dbs/{}/colls/{}", self.endpoint, database, container);
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
        let raw: Value = resp.json().await?;
        Ok(ContainerMeta::from_response(&raw))
    }

    /// Execute a query scoped to a single logical partition — no fan-out.
    pub async fn query_scoped(
        &self,
        database: &str,
        container: &str,
        sql: &str,
        parameters: Vec<Value>,
        pk_value: &Value,
        opts: &QueryOptions,
    ) -> Result<QueryResult, ClientError> {
        debug!(
            database,
            container,
            sql,
            ?pk_value,
            "executing partition-scoped query"
        );
        let url = format!(
            "{}/dbs/{}/colls/{}/docs",
            self.endpoint, database, container
        );
        let body = serde_json::json!({"query": sql, "parameters": parameters});
        let pk_header = serde_json::to_string(&vec![pk_value.clone()])
            .map_err(|e| ClientError::Other(format!("cannot encode partition key: {e}")))?;
        let (documents, charge) = self
            .query_partition_with(&url, &body, opts, |req| {
                req.header("x-ms-documentdb-partitionkey", &pk_header)
            })
            .await?;
        Ok(QueryResult {
            documents,
            request_charge: charge,
            per_range: vec![("scoped".to_string(), charge)],
        })
    }

    /// Execute a query once per partition-key-range, capturing query metrics
    /// and index utilization headers (for `cosq explain`).
    pub async fn query_with_metrics(
        &self,
        database: &str,
        container: &str,
        sql: &str,
        parameters: Vec<Value>,
    ) -> Result<QueryMetrics, ClientError> {
        let url = format!(
            "{}/dbs/{}/colls/{}/docs",
            self.endpoint, database, container
        );
        let body = serde_json::json!({"query": sql, "parameters": parameters});
        let ranges = self.get_partition_key_ranges(database, container).await?;

        let mut metrics = QueryMetrics::default();
        for range_id in ranges {
            let date = Self::date_header();
            let resp = self
                .http
                .post(&url)
                .header("Authorization", self.auth_header())
                .header("x-ms-date", &date)
                .header("x-ms-version", API_VERSION)
                .header("x-ms-documentdb-isquery", "True")
                .header("x-ms-documentdb-query-enablecrosspartition", "True")
                .header("x-ms-documentdb-partitionkeyrangeid", &range_id)
                .header("x-ms-documentdb-populatequerymetrics", "true")
                .header("x-ms-cosmos-populateindexmetrics", "true")
                .header("Content-Type", "application/query+json")
                .json(&body)
                .send()
                .await?;
            let status = resp.status();
            if !status.is_success() {
                let text = resp.text().await.unwrap_or_default();
                return Err(ClientError::api(status.as_u16(), text));
            }
            if let Some(qm) = resp
                .headers()
                .get("x-ms-documentdb-query-metrics")
                .and_then(|v| v.to_str().ok())
            {
                metrics
                    .query_metrics
                    .push((range_id.clone(), qm.to_string()));
            }
            if let Some(im) = resp
                .headers()
                .get("x-ms-cosmos-index-utilization")
                .and_then(|v| v.to_str().ok())
            {
                use base64::Engine;
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(im)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                    .unwrap_or(Value::String(im.to_string()));
                metrics.index_metrics.push((range_id.clone(), decoded));
            }
            metrics.request_charge += resp
                .headers()
                .get("x-ms-request-charge")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            let parsed: QueryResponse = resp.json().await?;
            metrics.document_count += parsed.documents.len();
        }
        Ok(metrics)
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
        opts: &QueryOptions,
        partition_key_range_id: &str,
    ) -> Result<(Vec<Value>, f64), ClientError> {
        self.query_partition_with(url, body, opts, |req| {
            req.header(
                "x-ms-documentdb-partitionkeyrangeid",
                partition_key_range_id,
            )
        })
        .await
    }

    /// Query with caller-supplied scoping headers (range id or partition key),
    /// following continuation tokens.
    async fn query_partition_with(
        &self,
        url: &str,
        body: &Value,
        opts: &QueryOptions,
        scope: impl Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<(Vec<Value>, f64), ClientError> {
        let mut documents = Vec::new();
        let mut total_charge = 0.0_f64;
        let mut continuation: Option<String> = None;

        loop {
            let date = Self::date_header();
            let mut request = scope(
                self.http
                    .post(url)
                    .header("Authorization", self.auth_header())
                    .header("x-ms-date", &date)
                    .header("x-ms-version", API_VERSION)
                    .header("x-ms-documentdb-isquery", "True")
                    .header("x-ms-documentdb-query-enablecrosspartition", "True")
                    .header("Content-Type", "application/query+json"),
            )
            .json(body);
            if let Some(count) = opts.max_item_count {
                request = request.header("x-ms-max-item-count", count.to_string());
            }

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

            if let Some(first) = opts.first
                && documents.len() >= first
            {
                documents.truncate(first);
                break;
            }

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
        self.query_with_params(
            database,
            container,
            sql,
            Vec::new(),
            &QueryOptions::default(),
        )
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
        opts: &QueryOptions,
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

        // Get partition key ranges and fan out the query — in parallel,
        // bounded, preserving range order in the collected output.
        let ranges = self.get_partition_key_ranges(database, container).await?;
        debug!(count = ranges.len(), "querying across partition key ranges");

        let results: Vec<(String, Vec<Value>, f64)> =
            futures_util::stream::iter(ranges.into_iter().map(|range_id| {
                let client = self.clone();
                let url = url.clone();
                let body = body.clone();
                let opts = opts.clone();
                async move {
                    let (docs, charge) = client
                        .query_partition(&url, &body, &opts, &range_id)
                        .await?;
                    debug!(
                        range_id,
                        docs = docs.len(),
                        charge,
                        "partition query complete"
                    );
                    Ok::<_, ClientError>((range_id, docs, charge))
                }
            }))
            .buffered(MAX_PARALLEL_RANGES)
            .try_collect()
            .await?;

        let mut all_documents = Vec::new();
        let mut total_charge = 0.0_f64;
        let mut per_range = Vec::new();
        for (range_id, docs, charge) in results {
            all_documents.extend(docs);
            total_charge += charge;
            per_range.push((range_id, charge));
        }
        if let Some(first) = opts.first {
            all_documents.truncate(first);
        }

        debug!(
            count = all_documents.len(),
            request_charge = total_charge,
            "query complete"
        );

        Ok(QueryResult {
            documents: all_documents,
            request_charge: total_charge,
            per_range,
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
