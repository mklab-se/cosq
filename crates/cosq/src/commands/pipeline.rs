//! Multi-step query pipeline execution
//!
//! Executes multi-step stored queries by:
//! 1. Building a dependency graph from `@step.field` references
//! 2. Executing steps in topological order (parallel where possible)
//! 3. Resolving step references by injecting actual values as parameters

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use colored::Colorize;
use cosq_client::cosmos::CosmosClient;
use cosq_core::stored_query::StoredQuery;
use serde_json::Value;

/// Results from executing all steps of a multi-step query
pub struct PipelineResult {
    /// Results keyed by step name (each step's documents array)
    pub step_results: BTreeMap<String, Vec<Value>>,
    /// Total request charge across all steps
    pub total_charge: f64,
}

/// Execute a multi-step stored query.
///
/// Steps are executed in dependency order — steps that only reference `@param`
/// parameters run in parallel, while steps referencing `@step.field` wait for
/// that step to complete first.
pub async fn execute(
    client: &CosmosClient,
    database: &str,
    query: &StoredQuery,
    params: &BTreeMap<String, Value>,
    quiet: bool,
) -> Result<PipelineResult> {
    let steps = query
        .metadata
        .steps
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("not a multi-step query"))?;

    let layers = query
        .execution_order()
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut step_results: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut total_charge = 0.0;

    for layer in &layers {
        if layer.len() == 1 {
            // Single step in this layer — execute directly
            let step_name = &layer[0];
            let step_def = steps.iter().find(|s| s.name == *step_name).unwrap();
            let sql = &query.step_queries[step_name];

            if !quiet {
                eprintln!(
                    "  {} {} ({})",
                    "▸".dimmed(),
                    step_name.cyan(),
                    step_def.container.dimmed()
                );
            }

            let cosmos_params = build_step_params(sql, query, params, &step_results)?;
            let result = client
                .query_with_params(database, &step_def.container, sql, cosmos_params)
                .await
                .with_context(|| format!("step '{step_name}' failed"))?;

            total_charge += result.request_charge;
            step_results.insert(step_name.clone(), result.documents);
        } else {
            // Multiple steps in this layer — execute in parallel
            let mut handles = Vec::new();

            for step_name in layer {
                let step_def = steps.iter().find(|s| s.name == *step_name).unwrap();
                let sql = query.step_queries[step_name].clone();

                if !quiet {
                    eprintln!(
                        "  {} {} ({})",
                        "▸".dimmed(),
                        step_name.cyan(),
                        step_def.container.dimmed()
                    );
                }

                let cosmos_params = build_step_params(&sql, query, params, &step_results)?;

                let container = step_def.container.clone();
                let db = database.to_string();
                let name = step_name.clone();
                let client = client.clone();

                handles.push(tokio::spawn(async move {
                    let result = client
                        .query_with_params(&db, &container, &sql, cosmos_params)
                        .await;
                    (name, result)
                }));
            }

            for handle in handles {
                let (name, result) = handle.await.context("step task panicked")?;
                let result = result.with_context(|| format!("step '{name}' failed"))?;
                total_charge += result.request_charge;
                step_results.insert(name, result.documents);
            }
        }
    }

    Ok(PipelineResult {
        step_results,
        total_charge,
    })
}

/// Build Cosmos DB parameters for a step, resolving both regular @params
/// and @step.field references from previously completed steps.
fn build_step_params(
    sql: &str,
    query: &StoredQuery,
    params: &BTreeMap<String, Value>,
    step_results: &BTreeMap<String, Vec<Value>>,
) -> Result<Vec<Value>> {
    let step_names: Vec<String> = query
        .metadata
        .steps
        .as_ref()
        .map(|s| s.iter().map(|s| s.name.clone()).collect())
        .unwrap_or_default();

    // Start with regular parameters
    let mut cosmos_params: Vec<Value> = params
        .iter()
        .map(|(name, value)| {
            serde_json::json!({
                "name": format!("@{name}"),
                "value": value
            })
        })
        .collect();

    // Resolve step references (@step.field)
    let refs = StoredQuery::find_step_references(sql, &step_names);
    for (step_name, field_name) in &refs {
        let docs = step_results.get(step_name).ok_or_else(|| {
            anyhow::anyhow!("step '{step_name}' has not been executed yet (dependency error)")
        })?;

        if docs.is_empty() {
            bail!(
                "Step '{}' returned no results — cannot resolve @{}.{}",
                step_name,
                step_name,
                field_name
            );
        }

        let value = docs[0].get(field_name).ok_or_else(|| {
            anyhow::anyhow!(
                "Field '{}' not found in step '{}' result. Available fields: {}",
                field_name,
                step_name,
                docs[0]
                    .as_object()
                    .map(|o| o.keys().cloned().collect::<Vec<_>>().join(", "))
                    .unwrap_or_else(|| "none".to_string())
            )
        })?;

        // The SQL uses @step.field but Cosmos DB params use @name format.
        // We pass the full @step.field as the parameter name.
        cosmos_params.push(serde_json::json!({
            "name": format!("@{step_name}.{field_name}"),
            "value": value
        }));
    }

    Ok(cosmos_params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_build_step_params_regular_only() {
        let contents = r#"---
description: test
params:
  - name: orderId
    type: string
steps:
  - name: header
    container: orders
---
-- step: header
SELECT * FROM c WHERE c.orderId = @orderId
"#;
        let query = StoredQuery::parse("test", contents).unwrap();
        let mut params = BTreeMap::new();
        params.insert("orderId".to_string(), json!("123"));
        let step_results = BTreeMap::new();

        let cosmos_params = build_step_params(
            &query.step_queries["header"],
            &query,
            &params,
            &step_results,
        )
        .unwrap();
        assert_eq!(cosmos_params.len(), 1);
        assert_eq!(cosmos_params[0]["name"], "@orderId");
        assert_eq!(cosmos_params[0]["value"], "123");
    }

    #[test]
    fn test_build_step_params_with_step_ref() {
        let contents = r#"---
description: test
params:
  - name: name
    type: string
steps:
  - name: customer
    container: customers
  - name: orders
    container: orders
---
-- step: customer
SELECT TOP 1 * FROM c WHERE c.name = @name

-- step: orders
SELECT * FROM c WHERE c.customerId = @customer.id
"#;
        let query = StoredQuery::parse("test", contents).unwrap();
        let mut params = BTreeMap::new();
        params.insert("name".to_string(), json!("Alice"));

        let mut step_results = BTreeMap::new();
        step_results.insert(
            "customer".to_string(),
            vec![json!({"id": "cust-42", "name": "Alice"})],
        );

        let cosmos_params = build_step_params(
            &query.step_queries["orders"],
            &query,
            &params,
            &step_results,
        )
        .unwrap();

        // Should have @name and @customer.id
        assert_eq!(cosmos_params.len(), 2);
        let step_param = cosmos_params
            .iter()
            .find(|p| p["name"] == "@customer.id")
            .unwrap();
        assert_eq!(step_param["value"], "cust-42");
    }

    #[test]
    fn test_build_step_params_empty_result_error() {
        let contents = r#"---
description: test
steps:
  - name: customer
    container: customers
  - name: orders
    container: orders
---
-- step: customer
SELECT TOP 1 * FROM c WHERE c.name = @name

-- step: orders
SELECT * FROM c WHERE c.customerId = @customer.id
"#;
        let query = StoredQuery::parse("test", contents).unwrap();
        let params = BTreeMap::new();
        let mut step_results = BTreeMap::new();
        step_results.insert("customer".to_string(), vec![]);

        let result = build_step_params(
            &query.step_queries["orders"],
            &query,
            &params,
            &step_results,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no results"));
    }
}
