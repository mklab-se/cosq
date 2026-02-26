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

use super::common;
use crate::output::{OutputFormat, render_multi_step_template, render_template, write_results};

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

    // Load config for connection details
    let mut config = Config::load()?;
    let client = CosmosClient::new(&config.account.endpoint).await?;

    let (database, db_changed) = common::resolve_database(
        &client,
        &mut config,
        args.db,
        query.metadata.database.as_deref(),
    )
    .await?;

    if query.is_multi_step() {
        // Multi-step execution: resolve database only (containers are per-step)
        if db_changed {
            config.save()?;
        }

        if !args.quiet {
            eprintln!("{}", "Executing steps:".dimmed());
        }

        let pipeline_result =
            super::pipeline::execute(&client, &database, &query, &resolved, args.quiet).await?;

        // Output multi-step results
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
                let template_str = resolve_template_str(&args.template, &query)?;
                if let Some(tmpl) = template_str {
                    // Flatten all step results for rendering recovery
                    let all_docs: Vec<Value> = pipeline_result
                        .step_results
                        .values()
                        .flat_map(|v| v.clone())
                        .collect();
                    match render_multi_step_template(
                        &tmpl,
                        &pipeline_result.step_results,
                        &resolved,
                    ) {
                        Ok(rendered) => print!("{rendered}"),
                        Err(_) => {
                            let rendered =
                                render_with_ai_recovery(&tmpl, &all_docs, &resolved, &query)
                                    .await?;
                            print!("{rendered}");
                        }
                    }
                } else {
                    // No template — output all step results as JSON
                    let combined: serde_json::Value =
                        serde_json::to_value(&pipeline_result.step_results)?;
                    let json = serde_json::to_string_pretty(&combined)?;
                    println!("{json}");
                }
            }
            _ => {
                // For non-template formats, combine all step results
                let combined: serde_json::Value =
                    serde_json::to_value(&pipeline_result.step_results)?;
                let json = serde_json::to_string_pretty(&combined)?;
                println!("{json}");
            }
        }

        if !args.quiet {
            eprintln!(
                "\n{} {:.2} RUs",
                "Request charge:".dimmed(),
                pipeline_result.total_charge
            );
        }
    } else {
        // Single-step execution (original path)
        let (container, ctr_changed) = common::resolve_container(
            &client,
            &mut config,
            &database,
            args.container,
            query.metadata.container.as_deref(),
        )
        .await?;

        if db_changed || ctr_changed {
            config.save()?;
        }

        let cosmos_params = StoredQuery::build_cosmos_params(&resolved);
        let result = client
            .query_with_params(&database, &container, &query.sql, cosmos_params)
            .await?;

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
                let template_str = resolve_template_str(&args.template, &query)?;
                if let Some(tmpl) = template_str {
                    let rendered =
                        render_with_ai_recovery(&tmpl, &result.documents, &resolved, &query)
                            .await?;
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
    }

    Ok(())
}

/// Attempt to render a template, and if it fails, offer AI-assisted fix.
/// Returns the rendered output or propagates the error if the user declines.
async fn render_with_ai_recovery(
    template_str: &str,
    documents: &[Value],
    params: &std::collections::BTreeMap<String, Value>,
    query: &StoredQuery,
) -> Result<String> {
    match render_template(template_str, documents, params) {
        Ok(rendered) => Ok(rendered),
        Err(e) => {
            let error_msg = format!("{e}");
            eprintln!("\n{} {}", "Template error:".red().bold(), error_msg);

            // Check if AI is configured
            let config = Config::load().ok();
            let ai_config = config.as_ref().and_then(|c| c.ai.clone());

            if let Some(ai) = ai_config {
                let fix = dialoguer::Confirm::with_theme(&ColorfulTheme::default())
                    .with_prompt("Would you like AI to fix this?")
                    .default(true)
                    .interact()
                    .unwrap_or(false);

                if fix {
                    return fix_template_with_ai(
                        &ai,
                        template_str,
                        &error_msg,
                        documents,
                        params,
                        query,
                    )
                    .await;
                }
            }

            Err(e)
        }
    }
}

/// Use AI to fix a broken template and re-render
async fn fix_template_with_ai(
    ai_config: &cosq_core::config::AiConfig,
    broken_template: &str,
    error_msg: &str,
    documents: &[Value],
    params: &std::collections::BTreeMap<String, Value>,
    query: &StoredQuery,
) -> Result<String> {
    eprintln!(
        "{}",
        format!("Fixing via {}...", ai_config.provider.display_name()).dimmed()
    );

    let sample = if documents.is_empty() {
        "(no documents)".to_string()
    } else {
        serde_json::to_string_pretty(&documents[0]).unwrap_or_default()
    };

    let system_prompt = format!(
        "You fix MiniJinja templates for cosq query output. \
         Respond with ONLY the corrected template — no explanation, no markdown fences.\n\n\
         Available filters: truncate(length), pad(width), length, upper, lower, title, trim, replace, default, join, first, last, round.\n\
         Available variables: documents (array of results), and named step arrays for multi-step queries.\n\n\
         Sample document:\n{sample}"
    );

    let user_prompt = format!(
        "This template has an error:\n\n{broken_template}\n\nError: {error_msg}\n\nFix the template."
    );

    let response = cosq_client::ai::generate_text(ai_config, &system_prompt, &user_prompt)
        .await
        .context("AI fix failed")?;

    let fixed = response.trim().to_string();
    let fixed = fixed
        .strip_prefix("```")
        .unwrap_or(&fixed)
        .strip_suffix("```")
        .unwrap_or(&fixed)
        .trim();

    // Try rendering with the fixed template
    match render_template(fixed, documents, params) {
        Ok(rendered) => {
            eprintln!("{} Template fixed successfully.", "OK".green().bold());

            // Offer to save the fix
            if query.metadata.template.is_some() {
                let save = dialoguer::Confirm::with_theme(&ColorfulTheme::default())
                    .with_prompt("Save the fixed template to the query file?")
                    .default(true)
                    .interact()
                    .unwrap_or(false);

                if save {
                    if let Err(e) = save_fixed_template(query, fixed) {
                        eprintln!("{} Could not save fix: {e}", "Warning:".yellow().bold());
                    }
                }
            }

            Ok(rendered)
        }
        Err(e2) => {
            eprintln!("{} AI fix still has errors: {}", "Error:".red().bold(), e2);
            Err(e2)
        }
    }
}

/// Save a fixed template back to the query's .cosq file
fn save_fixed_template(query: &StoredQuery, fixed_template: &str) -> Result<()> {
    let mut updated = query.clone();
    updated.metadata.template = Some(fixed_template.to_string());
    let contents = updated.to_file_contents()?;
    let path = cosq_core::stored_query::query_file_path(&query.name, false)?;
    std::fs::write(&path, &contents)?;
    eprintln!("{} Saved fix to {}", "OK".green().bold(), path.display());
    Ok(())
}

/// Resolve the template string from CLI arg, query metadata, or template file
fn resolve_template_str(
    cli_template: &Option<String>,
    query: &StoredQuery,
) -> Result<Option<String>> {
    if let Some(path) = cli_template {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read template file: {path}"))?;
        Ok(Some(content))
    } else if let Some(ref tmpl) = query.metadata.template {
        Ok(Some(tmpl.clone()))
    } else if let Some(ref tmpl_file) = query.metadata.template_file {
        let content = std::fs::read_to_string(tmpl_file)
            .with_context(|| format!("failed to read template file: {tmpl_file}"))?;
        Ok(Some(content))
    } else {
        Ok(None)
    }
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
            cosq_core::stored_query::parse_param_value_public(&param.name, &param.param_type, raw)?
        } else if let Some(ref choices) = param.choices {
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
