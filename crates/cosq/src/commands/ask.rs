//! `cosq ask` — natural-language question → Cosmos SQL → executed answer.

use anyhow::{Context, Result, bail};
use colored::Colorize;
use cosq_client::cosmos::{CosmosClient, QueryOptions};
use cosq_core::config::Config;
use cosq_core::schema_card::SchemaCard;
use serde::Deserialize;

use crate::output::{OutputFormat, write_results};

pub struct AskArgs {
    pub question: String,
    pub db: Option<String>,
    pub container: Option<String>,
    pub output: Option<OutputFormat>,
    pub save: Option<String>,
    pub sql_only: bool,
    pub yes: bool,
    pub quiet: bool,
}

/// Conversation memory for shell follow-ups: (question, sql, result note).
#[derive(Default, Clone)]
pub struct AskMemory {
    pub exchanges: Vec<(String, String, String)>,
}

impl AskMemory {
    const KEEP: usize = 5;
    pub fn push(&mut self, question: &str, sql: &str, note: &str) {
        self.exchanges
            .push((question.to_string(), sql.to_string(), note.to_string()));
        if self.exchanges.len() > Self::KEEP {
            self.exchanges.remove(0);
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct GeneratedQuery {
    pub sql: String,
    #[serde(default)]
    pub parameters: Vec<NamedParam>,
    pub explanation: String,
    pub confidence: f32,
}

#[derive(Debug, Deserialize)]
pub struct NamedParam {
    pub name: String,
    pub value: serde_json::Value,
}

fn generation_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "sql": {"type": "string", "description": "A single Cosmos DB NoSQL SELECT statement"},
            "parameters": {"type": "array", "items": {"type": "object", "properties": {
                "name": {"type": "string", "description": "@-prefixed parameter name"},
                "value": {}
            }, "required": ["name", "value"], "additionalProperties": false}},
            "explanation": {"type": "string", "description": "One sentence: what the query returns"},
            "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0,
                           "description": "How well the SQL matches the question given the schema"}
        },
        "required": ["sql", "parameters", "explanation", "confidence"],
        "additionalProperties": false
    })
}

pub fn system_prompt(card: &SchemaCard) -> String {
    format!(
        "You translate questions into Azure Cosmos DB NoSQL SELECT queries for the container \
         described below. Rules:\n\
         - Cosmos SQL dialect: use TOP not LIMIT; DateTimeAdd/GetCurrentDateTime for time math; \
           c is the document alias; string functions are case-sensitive unless LOWER is used.\n\
         - READ-ONLY: only SELECT statements.\n\
         - Prefer parameters (@name) for literal values derived from the question.\n\
         - When the question pins the partition key ({pk}), filter on it — that makes the query cheap.\n\
         - Only use fields that exist in the schema card.\n\n\
         Schema card:\n{card}",
        pk = card.partition_key.join(", "),
        card = card.to_prompt_yaml()
    )
}

/// Generate SQL for a question (optionally with conversation memory).
pub async fn generate(
    card: &SchemaCard,
    question: &str,
    memory: &AskMemory,
) -> Result<GeneratedQuery> {
    let mut messages = Vec::new();
    for (q, sql, note) in &memory.exchanges {
        messages.push(ailloy::Message::user(q.clone()));
        messages.push(ailloy::Message::assistant(format!(
            "{{\"sql\": {sql:?}, \"result\": {note:?}}}"
        )));
    }
    messages.push(ailloy::Message::user(question.to_string()));

    let value = cosq_client::ai::generate_json_with(
        &system_prompt(card),
        &messages,
        "cosmos_query",
        generation_schema(),
    )
    .await?;
    let generated: GeneratedQuery =
        serde_json::from_value(value).context("AI returned an unexpected query object")?;
    if !generated
        .sql
        .trim_start()
        .to_uppercase()
        .starts_with("SELECT")
    {
        bail!(
            "generated statement is not a SELECT (cosq is read-only): {}",
            generated.sql
        );
    }
    Ok(generated)
}

pub async fn run(args: AskArgs) -> Result<()> {
    if !cosq_client::ai::is_configured() {
        bail!("AI is not configured — run `cosq ai config` (or `cosq ai enable`) first");
    }
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

    let card = super::schema::ensure_card(
        &client,
        &profile_name,
        &profile,
        &database,
        &container,
        false,
    )
    .await?;

    let generated = generate(&card, &args.question, &AskMemory::default()).await?;
    execute_generated(
        &client,
        &card,
        &database,
        &container,
        &args.question,
        &generated,
        args.output.unwrap_or(OutputFormat::Table),
        args.sql_only,
        args.yes,
        args.quiet,
        args.save.as_deref(),
        None,
    )
    .await
}

/// Shared execution path (also used by the shell's `?` mode).
#[allow(clippy::too_many_arguments)]
pub async fn execute_generated(
    client: &CosmosClient,
    card: &SchemaCard,
    database: &str,
    container: &str,
    question: &str,
    generated: &GeneratedQuery,
    format: OutputFormat,
    sql_only: bool,
    yes: bool,
    quiet: bool,
    save: Option<&str>,
    memory: Option<&mut AskMemory>,
) -> Result<()> {
    if !quiet {
        eprintln!("{} {}", "sql:".dimmed(), generated.sql);
        eprintln!("{} {}", "→".dimmed(), generated.explanation.dimmed());
    }
    if sql_only {
        println!("{}", generated.sql);
        return Ok(());
    }

    // Low confidence or unscoped fan-out → confirm (unless --yes / non-tty).
    let params: Vec<(String, serde_json::Value)> = generated
        .parameters
        .iter()
        .map(|p| (p.name.clone(), p.value.clone()))
        .collect();
    let pk_value = card
        .partition_key
        .first()
        .and_then(|pk| cosq_core::pk_detect::detect_pk_equality(&generated.sql, pk, &params));
    let interactive = std::io::IsTerminal::is_terminal(&std::io::stdin());
    if !yes && interactive && generated.confidence < 0.6 {
        let proceed = inquire::Confirm::new(&format!(
            "confidence is {:.2} — run this query anyway?",
            generated.confidence
        ))
        .with_default(true)
        .prompt()
        .unwrap_or(false);
        if !proceed {
            return Ok(());
        }
    }

    let cosmos_params: Vec<serde_json::Value> = generated
        .parameters
        .iter()
        .map(|p| serde_json::json!({"name": p.name, "value": p.value}))
        .collect();
    let opts = QueryOptions::default();
    let result = match &pk_value {
        Some(pk) => {
            if !quiet {
                eprintln!("{}", format!("scoped to partition {pk}").dimmed());
            }
            client
                .query_scoped(
                    database,
                    container,
                    &generated.sql,
                    cosmos_params,
                    pk,
                    &opts,
                )
                .await?
        }
        None => {
            client
                .query_with_params(database, container, &generated.sql, cosmos_params, &opts)
                .await?
        }
    };

    let mut stdout = std::io::stdout();
    write_results(&mut stdout, &result.documents, &format)?;
    if !quiet {
        eprintln!(
            "{}",
            format!(
                "{} docs · {:.2} RUs",
                result.documents.len(),
                result.request_charge
            )
            .dimmed()
        );
    }

    if let Some(memory) = memory {
        memory.push(
            question,
            &generated.sql,
            &format!("{} documents returned", result.documents.len()),
        );
    }

    if let Some(name) = save {
        save_stored_query(name, question, generated)?;
        eprintln!("{} saved as stored query '{}'", "✓".green().bold(), name);
    }
    Ok(())
}

fn save_stored_query(name: &str, question: &str, generated: &GeneratedQuery) -> Result<()> {
    let dir = cosq_core::stored_query::user_queries_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.cosq"));
    if path.exists() {
        bail!("stored query '{name}' already exists");
    }
    let mut front = format!(
        "---\ndescription: {}\ngenerated_by: cosq ask\n",
        serde_yaml::to_string(&question)?.trim()
    );
    if !generated.parameters.is_empty() {
        front.push_str("params:\n");
        for p in &generated.parameters {
            let kind = match &p.value {
                serde_json::Value::Number(_) => "number",
                serde_json::Value::Bool(_) => "bool",
                _ => "string",
            };
            front.push_str(&format!(
                "  - name: {}\n    type: {}\n    default: {}\n",
                p.name.trim_start_matches('@'),
                kind,
                p.value
            ));
        }
    }
    front.push_str("---\n");
    std::fs::write(&path, format!("{front}{}\n", generated.sql))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> SchemaCard {
        SchemaCard {
            database: "db".into(),
            container: "orders".into(),
            built_at: chrono::Utc::now().to_rfc3339(),
            partition_key: vec!["/customerId".into()],
            fields: vec![],
            ..Default::default()
        }
    }

    #[test]
    fn system_prompt_contains_schema_and_rules() {
        let p = system_prompt(&card());
        assert!(p.contains("TOP not LIMIT"));
        assert!(p.contains("READ-ONLY"));
        assert!(p.contains("/customerId"));
        assert!(p.contains("container: orders"));
    }

    #[test]
    fn generated_query_parses_and_guards_non_select() {
        let g: GeneratedQuery = serde_json::from_value(serde_json::json!({
            "sql": "SELECT TOP 5 c.id FROM c",
            "parameters": [{"name": "@days", "value": 7}],
            "explanation": "five ids",
            "confidence": 0.9
        }))
        .unwrap();
        assert_eq!(g.parameters[0].name, "@days");
        assert!((g.confidence - 0.9).abs() < 1e-6);
    }

    #[test]
    fn memory_caps_at_five() {
        let mut m = AskMemory::default();
        for i in 0..8 {
            m.push(&format!("q{i}"), "SELECT 1", "1 documents returned");
        }
        assert_eq!(m.exchanges.len(), 5);
        assert_eq!(m.exchanges[0].0, "q3");
    }
}
