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

    // Execute query
    let result = client.query(&database, &container, &args.sql).await?;

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
    }

    Ok(())
}
