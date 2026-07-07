//! Query command — execute SQL queries against Cosmos DB
//!
//! Resolves database and container from CLI flags, config, or interactive
//! prompts, then executes the query and prints results in the requested format.

use anyhow::{Context, Result};
use colored::Colorize;
use cosq_client::cosmos::CosmosClient;
use cosq_core::config::Config;

use super::common;
use crate::output::{OutputFormat, render_template, write_results};

pub struct QueryArgs {
    pub sql: String,
    pub db: Option<String>,
    pub container: Option<String>,
    pub output: Option<OutputFormat>,
    pub template: Option<String>,
    pub pk: Option<String>,
    pub first: Option<usize>,
    pub max_items: Option<u32>,
    pub quiet: bool,
}

pub async fn run(args: QueryArgs) -> Result<()> {
    let mut config = Config::load()?;
    let client = CosmosClient::new(&config.account.endpoint).await?;

    let (database, db_changed) =
        common::resolve_database(&client, &mut config, args.db, None).await?;
    let (container, ctr_changed) =
        common::resolve_container(&client, &mut config, &database, args.container, None).await?;

    if db_changed || ctr_changed {
        config.save()?;
    }

    // Execute query — scoped to one partition when possible.
    let opts = cosq_client::cosmos::QueryOptions {
        max_item_count: args.max_items,
        first: args.first,
    };
    let pk_value: Option<serde_json::Value> = match &args.pk {
        Some(explicit) => Some(serde_json::Value::String(explicit.clone())),
        None => {
            // auto-detect from the WHERE clause using container metadata
            match client.get_container(&database, &container).await {
                Ok(meta) => meta.pk_paths.first().and_then(|pk_path| {
                    cosq_core::pk_detect::detect_pk_equality(&args.sql, pk_path, &[])
                }),
                Err(_) => None, // metadata unavailable — plain fan-out
            }
        }
    };
    let result = match &pk_value {
        Some(pk) => {
            if !args.quiet {
                eprintln!("{}", format!("Scoped to partition {pk}").dimmed());
            }
            client
                .query_scoped(&database, &container, &args.sql, Vec::new(), pk, &opts)
                .await?
        }
        None => {
            client
                .query_with_params(&database, &container, &args.sql, Vec::new(), &opts)
                .await?
        }
    };

    // Determine output format
    let has_template = args.template.is_some();
    let format = args.output.unwrap_or(if has_template {
        OutputFormat::Template
    } else {
        OutputFormat::Json
    });

    match format {
        OutputFormat::Template => {
            if let Some(ref path) = args.template {
                let template_str = std::fs::read_to_string(path)
                    .with_context(|| format!("failed to read template file: {path}"))?;
                let rendered = render_template(
                    &template_str,
                    &result.documents,
                    &std::collections::BTreeMap::new(),
                )?;
                print!("{rendered}");
            } else {
                write_results(
                    &mut std::io::stdout(),
                    &result.documents,
                    &OutputFormat::Json,
                )?;
            }
        }
        _ => {
            write_results(&mut std::io::stdout(), &result.documents, &format)?;
        }
    }

    if !args.quiet {
        eprintln!(
            "\n{} {:.2} RUs",
            "Request charge:".dimmed(),
            result.request_charge
        );
        if tracing::enabled!(tracing::Level::DEBUG) && result.per_range.len() > 1 {
            for (range, charge) in &result.per_range {
                eprintln!("{}", format!("  range {range}: {charge:.2} RUs").dimmed());
            }
        }
    }

    Ok(())
}
