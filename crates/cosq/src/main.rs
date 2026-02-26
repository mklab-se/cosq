//! cosq - A CLI to query your Azure Cosmos DB instances

use anyhow::Result;
use clap::{CommandFactory, Parser};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

mod banner;
mod cli;
mod commands;
mod output;
mod update;

use cli::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    // Handle dynamic shell completions (when invoked via COMPLETE=<shell> cosq)
    clap_complete::CompleteEnv::with_factory(Cli::command).complete();

    let cli = Cli::parse();

    // Initialize logging
    let filter = if cli.verbose > 0 {
        match cli.verbose {
            1 => "cosq=debug",
            _ => "cosq=trace",
        }
    } else if cli.quiet {
        "error"
    } else {
        "cosq=info"
    };

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(false).without_time())
        .with(EnvFilter::new(filter))
        .init();

    if cli.no_color {
        colored::control::set_override(false);
    }

    // Spawn background update check (skip in quiet mode or if disabled via env)
    let update_handle = if !cli.quiet && std::env::var("COSQ_NO_UPDATE_CHECK").is_err() {
        Some(tokio::spawn(update::check_for_updates()))
    } else {
        None
    };

    let result = cli.run().await;

    // Wait for update check to complete before exiting
    if let Some(handle) = update_handle {
        let _ = handle.await;
    }

    result
}
