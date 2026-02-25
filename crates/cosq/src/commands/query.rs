//! Query command — execute SQL queries against Cosmos DB
//!
//! Resolves database and container from CLI flags, config, or interactive
//! prompts, then executes the query and prints results as JSON to stdout.

use anyhow::{Context, Result, bail};
use colored::Colorize;
use cosq_client::cosmos::CosmosClient;
use cosq_core::config::Config;
use dialoguer::FuzzySelect;
use dialoguer::theme::ColorfulTheme;

pub struct QueryArgs {
    pub sql: String,
    pub db: Option<String>,
    pub container: Option<String>,
    pub quiet: bool,
}

pub async fn run(args: QueryArgs) -> Result<()> {
    // Load config
    let mut config = Config::load()?;

    // Create Cosmos client
    let client = CosmosClient::new(&config.account.endpoint).await?;

    let mut config_changed = false;

    // Resolve database
    let database = if let Some(db) = args.db {
        db
    } else if let Some(ref db) = config.database {
        db.clone()
    } else {
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
            eprintln!(
                "  {} {}",
                "Selected:".dimmed(),
                databases[selection].green()
            );
            databases[selection].clone()
        };

        config.database = Some(db.clone());
        config_changed = true;
        db
    };

    // Resolve container
    let container = if let Some(ctr) = args.container {
        ctr
    } else if let Some(ref ctr) = config.container {
        ctr.clone()
    } else {
        let containers = client.list_containers(&database).await?;
        if containers.is_empty() {
            bail!("No containers found in database '{}'.", database);
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
            eprintln!(
                "  {} {}",
                "Selected:".dimmed(),
                containers[selection].green()
            );
            containers[selection].clone()
        };

        config.container = Some(ctr.clone());
        config_changed = true;
        ctr
    };

    // Save updated config if we prompted interactively
    if config_changed {
        config.save()?;
    }

    // Execute query
    let result = client.query(&database, &container, &args.sql).await?;

    // Print JSON to stdout (pretty-printed)
    let json = serde_json::to_string_pretty(&result.documents)?;
    println!("{json}");

    // Print RU cost to stderr (unless quiet)
    if !args.quiet {
        eprintln!(
            "\n{} {:.2} RUs",
            "Request charge:".dimmed(),
            result.request_charge
        );
    }

    Ok(())
}
