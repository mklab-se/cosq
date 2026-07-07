//! `cosq search` — semantic / full-text / hybrid search over a container,
//! powered by Cosmos DB's own search engine (no local vector store).
//!
//! Mode selection from the container's policies (via the schema card):
//! - vector policy → embed the query text (ailloy) + `VectorDistance`
//! - full-text policy → `FullTextScore` with `ORDER BY RANK`
//! - both → RRF hybrid (pk-scoped/single-partition; cross-partition warns)
//! - neither → `CONTAINS` keyword fallback
//!
//! Cross-partition behavior (verified live, see cosmos.rs API_VERSION notes):
//! these queries execute per partition-key-range; VectorDistance projects a
//! mergeable score (exact merge), FullTextScore does not (per-range rank
//! interleave — approximate).

use anyhow::{Context, Result, bail};
use colored::Colorize;
use cosq_client::cosmos::{CosmosClient, QueryOptions};
use cosq_core::config::Config;
use cosq_core::schema_card::SchemaCard;

use crate::output::{OutputFormat, write_results};

pub struct SearchArgs {
    pub text: String,
    pub db: Option<String>,
    pub container: Option<String>,
    pub mode: Option<String>,
    pub top: usize,
    pub show_sql: bool,
    pub pk: Option<String>,
    pub output: Option<OutputFormat>,
    pub quiet: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Vector,
    Text,
    Hybrid,
    Keyword,
}

/// Pick the best mode the container supports (or validate an explicit one).
pub fn resolve_mode(card: &SchemaCard, requested: Option<&str>) -> Result<SearchMode> {
    let has_vector = card.vector.is_some();
    let has_fts = !card.full_text_paths.is_empty();
    match requested {
        Some("vector") => {
            if !has_vector {
                bail!(
                    "container {} has no vector policy — available modes: {}",
                    card.container,
                    available(has_vector, has_fts)
                );
            }
            Ok(SearchMode::Vector)
        }
        Some("text") => {
            if !has_fts {
                bail!(
                    "container {} has no full-text policy — available modes: {}",
                    card.container,
                    available(has_vector, has_fts)
                );
            }
            Ok(SearchMode::Text)
        }
        Some("hybrid") => {
            if !(has_vector && has_fts) {
                bail!(
                    "hybrid needs both vector and full-text policies — available: {}",
                    available(has_vector, has_fts)
                );
            }
            Ok(SearchMode::Hybrid)
        }
        Some(other) => bail!("unknown mode '{other}' (vector|text|hybrid)"),
        None => Ok(match (has_vector, has_fts) {
            (true, true) => SearchMode::Hybrid,
            (true, false) => SearchMode::Vector,
            (false, true) => SearchMode::Text,
            (false, false) => SearchMode::Keyword,
        }),
    }
}

fn available(vector: bool, fts: bool) -> String {
    let mut modes = Vec::new();
    if vector {
        modes.push("vector");
    }
    if fts {
        modes.push("text");
    }
    if vector && fts {
        modes.push("hybrid");
    }
    if modes.is_empty() {
        modes.push("keyword (CONTAINS fallback)");
    }
    modes.join(", ")
}

/// Split search text into FullTextScore terms (quoted-string SQL literals).
fn fts_terms(text: &str) -> String {
    text.split_whitespace()
        .map(|term| format!("'{}'", term.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Build the SQL for a mode. The query vector is passed as parameter @qv.
pub fn build_sql(card: &SchemaCard, mode: SearchMode, text: &str, top: usize) -> String {
    let vector_path = card
        .vector
        .as_ref()
        .map(|(p, _, _)| format!("c{}", p.replace('/', ".")))
        .unwrap_or_default();
    let fts_path = card
        .full_text_paths
        .first()
        .map(|p| format!("c{}", p.replace('/', ".")))
        .unwrap_or_default();
    match mode {
        // `SELECT *, expr` is invalid Cosmos SQL — project the doc + score
        // explicitly and flatten client-side.
        SearchMode::Vector => format!(
            "SELECT TOP {top} c AS doc, VectorDistance({vector_path}, @qv) AS _score \
             FROM c ORDER BY VectorDistance({vector_path}, @qv)"
        ),
        SearchMode::Text => format!(
            "SELECT TOP {top} * FROM c \
             ORDER BY RANK FullTextScore({fts_path}, {})",
            fts_terms(text)
        ),
        SearchMode::Hybrid => format!(
            "SELECT TOP {top} * FROM c \
             ORDER BY RANK RRF(VectorDistance({vector_path}, @qv), FullTextScore({fts_path}, {}))",
            fts_terms(text)
        ),
        SearchMode::Keyword => {
            format!("SELECT TOP {top} * FROM c WHERE CONTAINS(LOWER(ToString(c)), LOWER(@text))")
        }
    }
}

/// Find the ailloy embed node for this container: confirmed mapping on the
/// card/profile, else a dimensions-match against configured embed nodes.
async fn resolve_embed_node(card: &SchemaCard, dims: u32) -> Result<String> {
    if let Some(node) = &card.embed_node {
        return Ok(node.clone());
    }
    let ailloy_config = ailloy::config::Config::load()?;
    let embed_nodes: Vec<String> = ailloy_config
        .nodes
        .iter()
        .filter(|(_, node)| {
            node.capabilities
                .iter()
                .any(|c| matches!(c, ailloy::config::Capability::Embedding))
        })
        .map(|(id, _)| id.clone())
        .collect();
    match embed_nodes.len() {
        0 => bail!(
            "no embed-capable ailloy node configured — add one (e.g. text-embedding-3-large) \
             with `ailloy ai config`, capability `embed`"
        ),
        1 => Ok(embed_nodes[0].clone()),
        _ => {
            // Ambiguous: ask interactively, otherwise demand explicit config.
            if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                let choice = inquire::Select::new(
                    &format!("which embed node produced this container's {dims}-dim vectors?"),
                    embed_nodes,
                )
                .prompt()
                .context("selection cancelled")?;
                Ok(choice)
            } else {
                bail!(
                    "multiple embed nodes configured ({}) — set embed_models.{} in the cosq \
                     profile or embed_node in the schema card",
                    embed_nodes.join(", "),
                    card.container
                )
            }
        }
    }
}

pub async fn run(args: SearchArgs) -> Result<()> {
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

    let mut card = super::schema::ensure_card(
        &client,
        &profile_name,
        &profile,
        &database,
        &container,
        false,
    )
    .await?;

    let result = execute(
        &client,
        &mut card,
        &profile_name,
        &database,
        &container,
        &args.text,
        args.mode.as_deref(),
        args.top,
        args.show_sql,
        args.pk.as_deref(),
        args.quiet,
    )
    .await?;

    let format = args.output.unwrap_or(OutputFormat::Table);
    let mut stdout = std::io::stdout();
    write_results(&mut stdout, &result.0, &format)?;
    if !args.quiet {
        eprintln!(
            "{}",
            format!("{} docs · {:.2} RUs", result.0.len(), result.1).dimmed()
        );
    }
    Ok(())
}

/// Core search (shared with the shell). Returns (documents, RU).
#[allow(clippy::too_many_arguments)]
pub async fn execute(
    client: &CosmosClient,
    card: &mut SchemaCard,
    profile_name: &str,
    database: &str,
    container: &str,
    text: &str,
    mode: Option<&str>,
    top: usize,
    show_sql: bool,
    pk: Option<&str>,
    quiet: bool,
) -> Result<(Vec<serde_json::Value>, f64)> {
    let mode = resolve_mode(card, mode)?;
    if mode == SearchMode::Keyword && !quiet {
        eprintln!(
            "{}",
            "container has no vector or full-text policy — falling back to keyword CONTAINS"
                .yellow()
        );
    }
    if matches!(mode, SearchMode::Hybrid | SearchMode::Text) && pk.is_none() && !quiet {
        eprintln!(
            "{}",
            "note: cross-partition text/hybrid ranking is approximate (scores are not \
             comparable across partitions); pass --pk for exact ranking"
                .dimmed()
        );
    }

    let sql = build_sql(card, mode, text, top);
    if show_sql || !quiet {
        eprintln!("{} {}", "sql:".dimmed(), sql);
    }
    if show_sql {
        return Ok((Vec::new(), 0.0));
    }

    // Build parameters: query vector for vector/hybrid, raw text for keyword.
    let mut params: Vec<serde_json::Value> = Vec::new();
    if matches!(mode, SearchMode::Vector | SearchMode::Hybrid) {
        let (_, dims, _) = card.vector.clone().context("vector policy vanished")?;
        let node = resolve_embed_node(card, dims).await?;
        if !quiet {
            eprintln!("{}", format!("embedding query via {node}…").dimmed());
        }
        let ailloy_client = ailloy::Client::with_node(&node)?;
        let vector = ailloy_client.embed_one(text).await?;
        if vector.len() as u32 != dims {
            bail!(
                "embed node {node} produced {} dims but the container stores {dims} — \
                 configure the matching model (embed_models.{container} in the profile)",
                vector.len()
            );
        }
        // Remember the confirmed mapping on the card.
        if card.embed_node.as_deref() != Some(node.as_str()) {
            card.embed_node = Some(node.clone());
            let _ = card.save(profile_name);
        }
        params.push(serde_json::json!({"name": "@qv", "value": vector}));
    }
    if mode == SearchMode::Keyword {
        params.push(serde_json::json!({"name": "@text", "value": text}));
    }

    let opts = QueryOptions::default();
    let result = match pk {
        Some(pk_value) => {
            client
                .query_scoped(
                    database,
                    container,
                    &sql,
                    params,
                    &serde_json::Value::String(pk_value.to_string()),
                    &opts,
                )
                .await?
        }
        None => {
            client
                .query_with_params(database, container, &sql, params, &opts)
                .await?
        }
    };

    // Cross-partition merge: vector scores merge exactly; keep Cosmos's
    // per-range order otherwise (approximate, noted above).
    let mut documents = result.documents;
    if mode == SearchMode::Vector {
        // flatten {doc: {...}, _score} → {..., _score}, dropping the raw
        // embedding array (nobody wants 3072 floats in their results)
        let vector_field = card.vector.as_ref().map(|(p, _, _)| {
            p.trim_start_matches('/')
                .split('/')
                .next()
                .unwrap_or("")
                .to_string()
        });
        documents = documents
            .into_iter()
            .map(|entry| {
                let score = entry.get("_score").cloned();
                let mut doc = entry.get("doc").cloned().unwrap_or(entry);
                if let Some(obj) = doc.as_object_mut() {
                    if let Some(vf) = &vector_field {
                        obj.remove(vf);
                    }
                    // drop Cosmos system fields; keep the relevance score
                    obj.retain(|k, _| !k.starts_with('_'));
                    if let Some(score) = score {
                        obj.insert("_score".to_string(), score);
                    }
                }
                doc
            })
            .collect();
    }
    if mode == SearchMode::Vector && result.per_range.len() > 1 {
        // Determine direction from the distance function: cosine/dotproduct
        // rank higher-is-closer in Cosmos's ORDER BY; euclidean lower-is-closer.
        let ascending = card
            .vector
            .as_ref()
            .map(|(_, _, d)| d.eq_ignore_ascii_case("euclidean"))
            .unwrap_or(false);
        documents.sort_by(|a, b| {
            let sa = a
                .get("_score")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0);
            let sb = b
                .get("_score")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0);
            if ascending {
                sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
            } else {
                sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
            }
        });
    }
    documents.truncate(top);
    Ok((documents, result.request_charge))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(vector: bool, fts: bool) -> SchemaCard {
        SchemaCard {
            database: "db".into(),
            container: "c1".into(),
            built_at: chrono::Utc::now().to_rfc3339(),
            vector: vector.then(|| ("/embedding".into(), 1536, "cosine".into())),
            full_text_paths: if fts { vec!["/text".into()] } else { vec![] },
            ..Default::default()
        }
    }

    #[test]
    fn mode_resolution() {
        assert_eq!(
            resolve_mode(&card(true, true), None).unwrap(),
            SearchMode::Hybrid
        );
        assert_eq!(
            resolve_mode(&card(true, false), None).unwrap(),
            SearchMode::Vector
        );
        assert_eq!(
            resolve_mode(&card(false, true), None).unwrap(),
            SearchMode::Text
        );
        assert_eq!(
            resolve_mode(&card(false, false), None).unwrap(),
            SearchMode::Keyword
        );
        assert_eq!(
            resolve_mode(&card(true, true), Some("vector")).unwrap(),
            SearchMode::Vector
        );
        assert!(resolve_mode(&card(false, true), Some("vector")).is_err());
        assert!(resolve_mode(&card(true, false), Some("hybrid")).is_err());
        assert!(resolve_mode(&card(true, true), Some("bogus")).is_err());
    }

    #[test]
    fn sql_construction() {
        let sql = build_sql(&card(true, false), SearchMode::Vector, "refunds", 5);
        assert!(sql.contains("TOP 5"));
        assert!(sql.contains("c AS doc, VectorDistance(c.embedding, @qv) AS _score"));
        assert!(sql.contains("ORDER BY VectorDistance"));

        let sql = build_sql(&card(false, true), SearchMode::Text, "late delivery", 10);
        assert!(sql.contains("ORDER BY RANK FullTextScore(c.text, 'late', 'delivery')"));

        let sql = build_sql(&card(true, true), SearchMode::Hybrid, "it's here", 3);
        assert!(sql.contains(
            "RRF(VectorDistance(c.embedding, @qv), FullTextScore(c.text, 'it''s', 'here')"
        ));

        let sql = build_sql(&card(false, false), SearchMode::Keyword, "x", 10);
        assert!(sql.contains("CONTAINS"));
        assert!(sql.contains("@text"));
    }
}
