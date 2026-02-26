//! AI provider configuration command
//!
//! `cosq ai init` — interactive setup for AI-powered query generation.
//! Detects available providers and guides the user through configuration.

use anyhow::{Context, Result, bail};
use colored::Colorize;
use cosq_core::config::{AiConfig, AiProvider, Config};

use crate::cli::AiCommands;

pub async fn run(cmd: AiCommands) -> Result<()> {
    match cmd {
        AiCommands::Init => init().await,
    }
}

async fn init() -> Result<()> {
    // Load existing config (cosq init must have been run first)
    let mut config = Config::load().map_err(|_| {
        anyhow::anyhow!(
            "No cosq config found. Run `cosq init` first to set up your Cosmos DB account."
        )
    })?;

    // Show current config if present
    if let Some(ref ai) = config.ai {
        eprintln!(
            "{} AI is currently configured with {}{}",
            "Note:".bold(),
            ai.provider.display_name().cyan(),
            ai.effective_model()
                .map(|m| format!(" (model: {m})"))
                .unwrap_or_default()
        );
        eprintln!();
    }

    // Detect available providers
    let available = cosq_client::local_agent::detect_available_providers();

    if available.is_empty() {
        bail!(
            "No AI providers detected. Install one of the following:\n\
             \x20 - claude  (Anthropic Claude CLI)\n\
             \x20 - codex   (OpenAI Codex CLI)\n\
             \x20 - copilot (GitHub Copilot CLI)\n\
             \x20 - ollama  (Ollama for local LLMs)\n\
             \x20 - az      (Azure CLI for Azure OpenAI API)"
        );
    }

    // Build selection items with descriptions and availability status
    let items: Vec<String> = available
        .iter()
        .map(|p| format!("{:<14} — {}", p.display_name(), p.description()))
        .collect();

    eprintln!("{}", "Select an AI provider for query generation:".bold());

    let selection = dialoguer::FuzzySelect::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .items(&items)
        .default(0)
        .interact()
        .context("selection cancelled")?;

    let provider = &available[selection];

    // Provider-specific setup
    let ai_config = match provider {
        AiProvider::Claude | AiProvider::Codex | AiProvider::Copilot => {
            setup_local_agent(provider).await?
        }
        AiProvider::Ollama => setup_ollama().await?,
        AiProvider::AzureOpenai => setup_azure_openai()?,
    };

    // Save config
    config.ai = Some(ai_config.clone());
    let path = config.save()?;

    eprintln!();
    eprintln!(
        "{} AI configured with {}{}",
        "OK".green().bold(),
        ai_config.provider.display_name().cyan(),
        ai_config
            .effective_model()
            .map(|m| format!(" (model: {m})"))
            .unwrap_or_default()
    );
    eprintln!("  Config saved to {}", path.display().to_string().dimmed());
    eprintln!(
        "\n  Generate queries with: {}",
        "cosq queries generate \"describe your query\"".cyan()
    );

    Ok(())
}

/// Set up a local CLI agent (claude, codex, copilot)
async fn setup_local_agent(provider: &AiProvider) -> Result<AiConfig> {
    let default_model = provider.default_model().unwrap_or("default");

    eprintln!();
    eprintln!(
        "  {} uses {} as the recommended model.",
        provider.display_name().bold(),
        default_model.cyan()
    );

    let model: String = dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Model")
        .default(default_model.to_string())
        .interact_text()
        .context("input cancelled")?;

    Ok(AiConfig {
        provider: provider.clone(),
        model: Some(model),
        account: None,
        deployment: None,
        endpoint: None,
        subscription: None,
        resource_group: None,
        api_version: "2024-12-01-preview".to_string(),
        ollama_url: None,
    })
}

/// Set up Ollama with model selection from installed models
async fn setup_ollama() -> Result<AiConfig> {
    eprintln!();
    eprintln!("  {} Connecting to Ollama...", ">>".dimmed());

    let client = cosq_client::ollama::OllamaClient::new(None);
    let models = client
        .list_models()
        .await
        .context("Failed to connect to Ollama. Is it running?")?;

    if models.is_empty() {
        bail!(
            "No models installed in Ollama. Install one first:\n\
             \x20 ollama pull gemma3:4b\n\
             \x20 ollama pull llama3:8b"
        );
    }

    let items: Vec<String> = models
        .iter()
        .map(|m| {
            format!(
                "{:<24} ({})",
                m.name,
                cosq_client::ollama::format_model_size(m.size)
            )
        })
        .collect();

    eprintln!("  Select a model:");

    let selection = dialoguer::FuzzySelect::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .items(&items)
        .default(0)
        .interact()
        .context("selection cancelled")?;

    let model_name = &models[selection].name;

    // Ask for custom Ollama URL
    let url: String = dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Ollama URL")
        .default("http://localhost:11434".to_string())
        .interact_text()
        .context("input cancelled")?;

    let ollama_url = if url == "http://localhost:11434" {
        None // don't store the default
    } else {
        Some(url)
    };

    Ok(AiConfig {
        provider: AiProvider::Ollama,
        model: Some(model_name.clone()),
        account: None,
        deployment: None,
        endpoint: None,
        subscription: None,
        resource_group: None,
        api_version: "2024-12-01-preview".to_string(),
        ollama_url,
    })
}

/// Set up Azure OpenAI with manual account/deployment input
fn setup_azure_openai() -> Result<AiConfig> {
    eprintln!();
    eprintln!(
        "  Enter your Azure OpenAI account details. Find these in the Azure Portal under\n\
         \x20 your Azure AI Services or Azure OpenAI resource."
    );

    let account: String = dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Azure OpenAI account name")
        .interact_text()
        .context("input cancelled")?;

    let deployment: String =
        dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Model deployment name (e.g., gpt-4o-mini)")
            .interact_text()
            .context("input cancelled")?;

    Ok(AiConfig {
        provider: AiProvider::AzureOpenai,
        model: None,
        account: Some(account),
        deployment: Some(deployment),
        endpoint: None,
        subscription: None,
        resource_group: None,
        api_version: "2024-12-01-preview".to_string(),
        ollama_url: None,
    })
}
