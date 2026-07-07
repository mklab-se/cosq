//! `cosq explain` — the query doctor: what did this query cost and why,
//! which indexes were used, and what would make it cheaper.

use anyhow::Result;
use colored::Colorize;
use cosq_client::cosmos::{CosmosClient, QueryMetrics};
use cosq_core::config::Config;

pub struct ExplainArgs {
    pub sql: String,
    pub db: Option<String>,
    pub container: Option<String>,
}

/// Parse Cosmos's `key1=value1;key2=value2` query-metrics text.
pub fn parse_metrics_pairs(raw: &str) -> Vec<(String, String)> {
    raw.split(';')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

/// Human labels for the interesting metrics keys.
fn label(key: &str) -> Option<&'static str> {
    match key {
        "totalExecutionTimeInMs" => Some("total execution time (ms)"),
        "queryCompileTimeInMs" => Some("query compile (ms)"),
        "indexLookupTimeInMs" => Some("index lookup (ms)"),
        "documentLoadTimeInMs" => Some("document load (ms)"),
        "retrievedDocumentCount" => Some("documents retrieved"),
        "retrievedDocumentSize" => Some("retrieved bytes"),
        "outputDocumentCount" => Some("documents returned"),
        "writeOutputTimeInMs" => Some("output write (ms)"),
        "indexUtilizationRatio" => Some("index utilization ratio"),
        _ => None,
    }
}

pub async fn run(args: ExplainArgs) -> Result<()> {
    let mut config = Config::load()?;
    let (profile_name, profile) = config.active_mut(None)?;
    let profile_name = profile_name.to_string();
    let mut profile = profile.clone();
    let client = CosmosClient::new(&profile.account.endpoint).await?;
    let (database, _) =
        super::common::resolve_database(&client, &mut profile, args.db, None).await?;
    let (container, _) =
        super::common::resolve_container(&client, &mut profile, &database, args.container, None)
            .await?;

    let metrics = client
        .query_with_metrics(&database, &container, &args.sql, Vec::new())
        .await?;

    print_metrics(&args.sql, &metrics);

    if cosq_client::ai::is_configured() {
        let card = super::schema::ensure_card(
            &client,
            &profile_name,
            &profile,
            &database,
            &container,
            false,
        )
        .await
        .ok();
        match diagnose(&args.sql, &metrics, card.as_ref()).await {
            Ok(diagnosis) => {
                eprintln!();
                eprintln!("{}", "diagnosis".bold());
                eprintln!("{diagnosis}");
            }
            Err(e) => eprintln!("{}", format!("(AI diagnosis unavailable: {e:#})").dimmed()),
        }
    }
    Ok(())
}

pub fn print_metrics(sql: &str, metrics: &QueryMetrics) {
    eprintln!("{} {}", "query:".dimmed(), sql);
    eprintln!(
        "{} {:.2} RUs · {} docs across {} partition range(s)",
        "cost:".bold(),
        metrics.request_charge,
        metrics.document_count,
        metrics.query_metrics.len().max(1)
    );
    for (range, raw) in &metrics.query_metrics {
        eprintln!();
        eprintln!("{}", format!("range {range}").dimmed());
        for (key, value) in parse_metrics_pairs(raw) {
            if let Some(nice) = label(&key) {
                eprintln!("  {nice:<28} {value}");
            }
        }
    }
    for (range, im) in &metrics.index_metrics {
        let utilized = im
            .pointer("/UtilizedSingleIndexes")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|e| e.pointer("/IndexSpec").and_then(|s| s.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let potential = im
            .pointer("/PotentialSingleIndexes")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|e| e.pointer("/IndexSpecs").and_then(|s| s.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let composite = im
            .pointer("/PotentialCompositeIndexes")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        eprintln!();
        eprintln!("{}", format!("index utilization (range {range})").dimmed());
        if !utilized.is_empty() {
            eprintln!("  {} {utilized}", "used:".green());
        }
        if !potential.is_empty() {
            eprintln!("  {} {potential}", "recommended:".yellow());
        }
        if composite > 0 {
            eprintln!(
                "  {} {composite} composite index candidate(s) — see JSON below",
                "recommended:".yellow()
            );
            eprintln!(
                "{}",
                serde_json::to_string_pretty(im.pointer("/PotentialCompositeIndexes").unwrap())
                    .unwrap_or_default()
            );
        }
    }
}

async fn diagnose(
    sql: &str,
    metrics: &QueryMetrics,
    card: Option<&cosq_core::schema_card::SchemaCard>,
) -> Result<String> {
    let system = "You are a Cosmos DB query-performance doctor for a read-only CLI. Given a \
                  query, its metrics, and index utilization, explain in a few short lines: what \
                  dominated the cost, whether the indexes served it well, the concrete \
                  indexingPolicy JSON snippet to add if indexes are missing (includedPaths / \
                  compositeIndexes), and whether partition scoping or TOP/--first would help. \
                  Plain terminal text, no markdown headers.";
    let mut user = format!(
        "SQL: {sql}\nTotal RU: {:.2}\nDocuments: {}\n",
        metrics.request_charge, metrics.document_count
    );
    for (range, raw) in &metrics.query_metrics {
        user.push_str(&format!("Metrics [{range}]: {raw}\n"));
    }
    for (range, im) in &metrics.index_metrics {
        user.push_str(&format!("Index utilization [{range}]: {im}\n"));
    }
    if let Some(card) = card {
        user.push_str(&format!(
            "Partition key: {}\n",
            card.partition_key.join(", ")
        ));
    }
    cosq_client::ai::generate_text(system, &user).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_pairs_parse() {
        let raw = "totalExecutionTimeInMs=1.24;retrievedDocumentCount=36;outputDocumentCount=7;indexLookupTimeInMs=0.21";
        let pairs = parse_metrics_pairs(raw);
        assert_eq!(pairs.len(), 4);
        assert_eq!(pairs[0].0, "totalExecutionTimeInMs");
        assert_eq!(
            pairs[1],
            ("retrievedDocumentCount".to_string(), "36".to_string())
        );
    }
}
