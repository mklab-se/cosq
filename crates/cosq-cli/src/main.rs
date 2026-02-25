//! cosq - A CLI to query your Azure Cosmos DB instances

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

mod banner;
mod cli;

use cli::Cli;

#[tokio::main]
async fn main() -> Result<()> {
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

    cli.run().await
}
