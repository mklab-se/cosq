//! Run command — execute stored queries from .cosq files
//!
//! Resolves parameters from CLI arguments or interactive prompts,
//! validates them, and executes the query against Cosmos DB.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use colored::Colorize;
use cosq_client::cosmos::CosmosClient;
use cosq_core::config::Config;
use cosq_core::stored_query::{StoredQuery, find_stored_query, list_stored_queries};
use dialoguer::theme::ColorfulTheme;
use dialoguer::{FuzzySelect, Input};
use serde_json::Value;

use crate::output::{OutputFormat, render_template, write_results};

pub struct RunArgs {
    pub name: Option<String>,
    pub params: Vec<String>,
    pub output: Option<OutputFormat>,
    pub db: Option<String>,
    pub container: Option<String>,
    pub template: Option<String>,
    pub quiet: bool,
}

pub async fn run(args: RunArgs) -> Result<()> {
    // Resolve query: from name argument or interactive picker
    let query = if let Some(ref name) = args.name {
        find_stored_query(name)
            .map_err(|e| anyhow::anyhow!("Failed to load query '{name}': {e}"))?
    } else {
        pick_query_interactive()?
    };

    if !args.quiet {
        eprintln!("{} {}", "Running:".bold(), query.name.cyan());
        if !query.metadata.description.is_empty() {
            eprintln!("  {}", query.metadata.description.dimmed());
        }
    }

    // Parse CLI params (--key value pairs from the raw args)
    let cli_params = parse_cli_params(&args.params)?;

    // Resolve parameters: CLI > interactive > default
    let resolved = resolve_params_interactive(&query, &cli_params)?;

    // Build Cosmos DB parameters
    let cosmos_params = StoredQuery::build_cosmos_params(&resolved);

    // Load config for connection details
    let mut config = Config::load()?;
    let client = CosmosClient::new(&config.account.endpoint).await?;

    // Resolve database: CLI > query metadata > config > interactive
    let mut config_changed = false;
    let database = if let Some(db) = args.db {
        db
    } else if let Some(ref db) = query.metadata.database {
        db.clone()
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
            databases[selection].clone()
        };
        config.database = Some(db.clone());
        config_changed = true;
        db
    };

    // Resolve container: CLI > query metadata > config > interactive
    let container = if let Some(ctr) = args.container {
        ctr
    } else if let Some(ref ctr) = query.metadata.container {
        ctr.clone()
    } else if let Some(ref ctr) = config.container {
        ctr.clone()
    } else {
        let containers = client.list_containers(&database).await?;
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
        config_changed = true;
        ctr
    };

    if config_changed {
        config.save()?;
    }

    // Execute query
    let result = client
        .query_with_params(&database, &container, &query.sql, cosmos_params)
        .await?;

    // Determine output format
    // If an explicit template file is passed, use it
    // If the query has an embedded template and no explicit output format, auto-use it
    // Otherwise use the specified format (or default JSON)
    let has_template = args.template.is_some()
        || query.metadata.template.is_some()
        || query.metadata.template_file.is_some();

    let effective_output = args.output.unwrap_or(if has_template {
        OutputFormat::Template
    } else {
        OutputFormat::Json
    });

    match effective_output {
        OutputFormat::Template => {
            let template_str = if let Some(ref path) = args.template {
                std::fs::read_to_string(path)
                    .with_context(|| format!("failed to read template file: {path}"))?
            } else if let Some(ref tmpl) = query.metadata.template {
                tmpl.clone()
            } else if let Some(ref tmpl_file) = query.metadata.template_file {
                std::fs::read_to_string(tmpl_file)
                    .with_context(|| format!("failed to read template file: {tmpl_file}"))?
            } else {
                // No template available, fall back to JSON
                write_results(
                    &mut std::io::stdout(),
                    &result.documents,
                    &OutputFormat::Json,
                )?;
                if !args.quiet {
                    eprintln!(
                        "\n{} {:.2} RUs",
                        "Request charge:".dimmed(),
                        result.request_charge
                    );
                }
                return Ok(());
            };

            let rendered = render_template(&template_str, &result.documents, &resolved)?;
            print!("{rendered}");
        }
        _ => {
            write_results(&mut std::io::stdout(), &result.documents, &effective_output)?;
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

/// Interactively pick a stored query from a fuzzy-select list.
fn pick_query_interactive() -> Result<StoredQuery> {
    let queries = list_stored_queries().unwrap_or_default();
    if queries.is_empty() {
        bail!(
            "No stored queries found.\n\n  \
             Create one with: cosq queries create <name>"
        );
    }

    let display_items: Vec<String> = queries
        .iter()
        .map(|q| {
            if q.metadata.description.is_empty() {
                q.name.clone()
            } else {
                format!("{} — {}", q.name, q.metadata.description)
            }
        })
        .collect();

    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Select a stored query")
        .items(&display_items)
        .default(0)
        .interact()
        .context("query selection cancelled")?;

    Ok(queries.into_iter().nth(selection).unwrap())
}

/// Parse --key value pairs from the raw parameter strings.
/// Expects alternating --name value pairs.
fn parse_cli_params(params: &[String]) -> Result<BTreeMap<String, String>> {
    let mut map = BTreeMap::new();
    let mut iter = params.iter();

    while let Some(key) = iter.next() {
        let name = key
            .strip_prefix("--")
            .ok_or_else(|| anyhow::anyhow!("expected parameter in --name format, got: {key}"))?;

        let value = iter
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing value for parameter --{name}"))?;

        map.insert(name.to_string(), value.to_string());
    }

    Ok(map)
}

/// Resolve parameters, prompting interactively for any that aren't provided via CLI.
fn resolve_params_interactive(
    query: &StoredQuery,
    cli_params: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, Value>> {
    let mut resolved = BTreeMap::new();

    for param in &query.metadata.params {
        let value = if let Some(raw) = cli_params.get(&param.name) {
            // Parse from CLI string
            cosq_core::stored_query::parse_param_value_public(&param.name, &param.param_type, raw)?
        } else if let Some(ref choices) = param.choices {
            // Interactive: fuzzy-select from choices
            let choice_strs: Vec<String> = choices
                .iter()
                .map(|c| match c {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect();

            let default_idx = param
                .default
                .as_ref()
                .and_then(|d| choices.iter().position(|c| c == d))
                .unwrap_or(0);

            let prompt = if let Some(ref desc) = param.description {
                format!("{} ({})", param.name, desc)
            } else {
                param.name.clone()
            };

            let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
                .with_prompt(&prompt)
                .items(&choice_strs)
                .default(default_idx)
                .interact()
                .context("parameter selection cancelled")?;

            choices[selection].clone()
        } else if param.is_required() || param.default.is_some() {
            // Interactive: text input
            let prompt = if let Some(ref desc) = param.description {
                format!("{} ({})", param.name, desc)
            } else {
                param.name.clone()
            };

            let default_str = param.default.as_ref().map(|d| match d {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            });

            let theme = ColorfulTheme::default();
            let raw = if let Some(def) = default_str {
                Input::<String>::with_theme(&theme)
                    .with_prompt(&prompt)
                    .default(def)
                    .interact_text()
                    .context("input cancelled")?
            } else {
                Input::<String>::with_theme(&theme)
                    .with_prompt(&prompt)
                    .interact_text()
                    .context("input cancelled")?
            };

            cosq_core::stored_query::parse_param_value_public(&param.name, &param.param_type, &raw)?
        } else {
            // Not required and no default — skip
            continue;
        };

        param.validate(&value).map_err(|e| anyhow::anyhow!("{e}"))?;
        resolved.insert(param.name.clone(), value);
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cli_params() {
        let params = vec![
            "--days".to_string(),
            "7".to_string(),
            "--status".to_string(),
            "active".to_string(),
        ];
        let parsed = parse_cli_params(&params).unwrap();
        assert_eq!(parsed.get("days"), Some(&"7".to_string()));
        assert_eq!(parsed.get("status"), Some(&"active".to_string()));
    }

    #[test]
    fn test_parse_cli_params_empty() {
        let parsed = parse_cli_params(&[]).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn test_parse_cli_params_missing_value() {
        let params = vec!["--days".to_string()];
        let result = parse_cli_params(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_cli_params_bad_format() {
        let params = vec!["days".to_string(), "7".to_string()];
        let result = parse_cli_params(&params);
        assert!(result.is_err());
    }
}
