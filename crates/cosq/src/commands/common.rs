//! Shared helpers for CLI commands
//!
//! Database and container resolution with the standard fallback chain:
//! CLI flag > stored query metadata > config > interactive picker.

use anyhow::{Context, Result, bail};
use colored::Colorize;
use cosq_client::cosmos::CosmosClient;
use cosq_core::config::Config;
use dialoguer::FuzzySelect;
use dialoguer::theme::ColorfulTheme;

/// Resolve which database to target.
///
/// Fallback chain: `cli` > `metadata` > `config.database` > interactive picker.
/// Returns the database name and whether the config was updated (needs save).
pub async fn resolve_database(
    client: &CosmosClient,
    config: &mut Config,
    cli: Option<String>,
    metadata: Option<&str>,
) -> Result<(String, bool)> {
    if let Some(db) = cli {
        return Ok((db, false));
    }
    if let Some(db) = metadata {
        return Ok((db.to_string(), false));
    }
    if let Some(ref db) = config.database {
        return Ok((db.clone(), false));
    }

    let databases = client.list_databases().await?;
    if databases.is_empty() {
        bail!(
            "No databases found in Cosmos DB account '{}'.",
            config.account.name
        );
    }

    let db = if databases.len() == 1 {
        eprintln!("{} {}", "Using database:".bold(), databases[0].green());
        databases[0].clone()
    } else {
        let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
            .with_prompt("Select a database")
            .items(&databases)
            .default(0)
            .interact()
            .context("database selection cancelled")?;
        databases[selection].clone()
    };

    config.database = Some(db.clone());
    Ok((db, true))
}

/// Resolve which container to target within a database.
///
/// Fallback chain: `cli` > `metadata` > `config.container` > interactive picker.
/// Returns the container name and whether the config was updated (needs save).
pub async fn resolve_container(
    client: &CosmosClient,
    config: &mut Config,
    database: &str,
    cli: Option<String>,
    metadata: Option<&str>,
) -> Result<(String, bool)> {
    if let Some(ctr) = cli {
        return Ok((ctr, false));
    }
    if let Some(ctr) = metadata {
        return Ok((ctr.to_string(), false));
    }
    if let Some(ref ctr) = config.container {
        return Ok((ctr.clone(), false));
    }

    let containers = client.list_containers(database).await?;
    if containers.is_empty() {
        bail!("No containers found in database '{database}'.");
    }

    let ctr = if containers.len() == 1 {
        eprintln!("{} {}", "Using container:".bold(), containers[0].green());
        containers[0].clone()
    } else {
        let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
            .with_prompt("Select a container")
            .items(&containers)
            .default(0)
            .interact()
            .context("container selection cancelled")?;
        containers[selection].clone()
    };

    config.container = Some(ctr.clone());
    Ok((ctr, true))
}
