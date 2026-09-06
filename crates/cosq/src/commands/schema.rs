//! `cosq schema` — build, cache, and print a container's schema card.

use anyhow::Result;
use colored::Colorize;
use cosq_client::cosmos::{CosmosClient, QueryOptions};
use cosq_core::config::{Config, Profile};
use cosq_core::schema_card::{FieldInfo, Relationship, SchemaCard, fields_from_samples};

pub struct SchemaArgs {
    pub container: Option<String>,
    pub db: Option<String>,
    pub refresh: bool,
    pub json: bool,
}

pub async fn run(args: SchemaArgs) -> Result<()> {
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

    let card = ensure_card(
        &client,
        &profile_name,
        &profile,
        &database,
        &container,
        args.refresh,
    )
    .await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&card)?);
    } else {
        print_card(&card);
    }
    Ok(())
}

/// Load a fresh-enough card or build one (used by schema/ask/search/shell).
pub async fn ensure_card(
    client: &CosmosClient,
    profile_name: &str,
    profile: &Profile,
    database: &str,
    container: &str,
    refresh: bool,
) -> Result<SchemaCard> {
    if !refresh
        && let Some((card, _path)) = SchemaCard::load(profile_name, database, container)
        && !card.is_stale()
    {
        return Ok(card);
    }
    let card = build_card(client, profile, database, container).await?;
    card.save(profile_name)?;
    Ok(card)
}

async fn build_card(
    client: &CosmosClient,
    profile: &Profile,
    database: &str,
    container: &str,
) -> Result<SchemaCard> {
    eprintln!(
        "{}",
        format!("building schema card for {database}/{container}…").dimmed()
    );
    let meta = client.get_container(database, container).await?;
    let samples = client
        .query_with_params(
            database,
            container,
            "SELECT TOP 3 * FROM c",
            Vec::new(),
            &QueryOptions::default(),
        )
        .await?
        .documents;

    let mut fields = fields_from_samples(&samples);
    let mut relationships = Vec::new();

    // AI distillation: descriptions + relationships (best effort).
    if cosq_client::ai::is_configured() {
        match distill(database, container, &samples, &fields, client).await {
            Ok((ai_fields, ai_rels)) => {
                // merge descriptions into mechanical fields (paths are truth)
                for field in &mut fields {
                    if let Some(ai) = ai_fields.iter().find(|f| f.path == field.path) {
                        field.description = ai.description.clone().filter(|d| !d.trim().is_empty());
                        if field.values.is_empty() {
                            field.values = ai.values.clone();
                        }
                    }
                }
                relationships = ai_rels;
            }
            Err(e) => eprintln!("{}", format!("(AI distillation skipped: {e:#})").dimmed()),
        }
    }

    Ok(SchemaCard {
        database: database.to_string(),
        container: container.to_string(),
        built_at: chrono::Utc::now().to_rfc3339(),
        partition_key: meta.pk_paths,
        fields,
        relationships,
        vector: meta.vector_paths.first().cloned(),
        full_text_paths: meta.full_text_paths,
        embed_node: profile.embed_models.get(container).cloned(),
    })
}

async fn distill(
    database: &str,
    container: &str,
    samples: &[serde_json::Value],
    fields: &[FieldInfo],
    client: &CosmosClient,
) -> Result<(Vec<FieldInfo>, Vec<Relationship>)> {
    // Other containers in the database inform relationship inference.
    let siblings = client.list_containers(database).await.unwrap_or_default();

    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "fields": {"type": "array", "items": {"type": "object", "properties": {
                "path": {"type": "string"},
                "description": {"type": "string", "description": "One line; empty string if nothing useful to say"},
                "values": {"type": "array", "items": {"type": "string"},
                           "description": "Low-cardinality value set; empty if not applicable"}
            }, "required": ["path", "description", "values"], "additionalProperties": false}},
            "relationships": {"type": "array", "items": {"type": "object", "properties": {
                "field": {"type": "string"},
                "references": {"type": "string", "description": "container.field, e.g. customers.id"},
                "confidence": {"type": "string", "enum": ["low", "medium", "high"]}
            }, "required": ["field", "references", "confidence"], "additionalProperties": false}}
        },
        "required": ["fields", "relationships"],
        "additionalProperties": false
    });

    let system = "You document Cosmos DB containers for a query tool. Given sampled documents \
                  and the mechanically-extracted field list, write a one-line description per \
                  meaningful field and infer likely references to sibling containers (only when \
                  a field name/value pattern strongly suggests it). Be terse and factual.";
    let samples_text = serde_json::to_string_pretty(samples).unwrap_or_default();
    let samples_text: String = samples_text.chars().take(4000).collect();
    let user = format!(
        "Container: {container} (database {database})\nSibling containers: {}\nFields: {}\n\nSample documents:\n{samples_text}",
        siblings.join(", "),
        fields
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    let value = cosq_client::ai::generate_json(system, &user, "schema_card", schema).await?;
    let ai_fields: Vec<FieldInfo> = value
        .get("fields")
        .and_then(|f| serde_json::from_value(f.clone()).ok())
        .unwrap_or_default();
    let relationships: Vec<Relationship> = value
        .get("relationships")
        .and_then(|r| serde_json::from_value(r.clone()).ok())
        .unwrap_or_default();
    Ok((ai_fields, relationships))
}

pub fn print_card(card: &SchemaCard) {
    println!(
        "{} {}/{}",
        "Schema card".bold(),
        card.database,
        card.container.bold()
    );
    println!(
        "  {} {}   {} {}",
        "partition key:".dimmed(),
        card.partition_key.join(", "),
        "built:".dimmed(),
        card.built_at
    );
    if let Some((path, dims, distance)) = &card.vector {
        println!(
            "  {} {path} ({dims} dims, {distance}){}",
            "vector:".dimmed(),
            card.embed_node
                .as_deref()
                .map(|n| format!(" · embed node: {n}"))
                .unwrap_or_default()
        );
    }
    if !card.full_text_paths.is_empty() {
        println!(
            "  {} {}",
            "full-text:".dimmed(),
            card.full_text_paths.join(", ")
        );
    }
    println!();
    for field in &card.fields {
        let mut line = format!("  {:<28} {}", field.path.bold(), field.types.join("|"));
        if !field.values.is_empty() {
            line.push_str(&format!("  {{{}}}", field.values.join("|")));
        } else if let Some(example) = &field.example {
            line.push_str(&format!("  e.g. {example}"));
        }
        println!("{line}");
        if let Some(desc) = &field.description {
            println!("      {}", desc.dimmed());
        }
    }
    if !card.relationships.is_empty() {
        println!();
        println!("  {}", "relationships:".bold());
        for r in &card.relationships {
            println!(
                "    {} → {} ({})",
                r.field,
                r.references,
                r.confidence.dimmed()
            );
        }
    }
}
