//! CLI argument definitions using clap

use anyhow::Result;
use clap::Parser;

/// A CLI to query your Azure Cosmos DB instances
#[derive(Parser)]
#[command(name = "cosq")]
#[command(author, version, about)]
#[command(long_about = "A CLI to query your Azure Cosmos DB instances.\n\n\
    Connect to your Cosmos DB accounts and run queries directly from the command line.")]
#[command(propagate_version = true)]
pub struct Cli {
    /// Increase output verbosity (-v for debug, -vv for trace)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Suppress non-essential output
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Disable colored output
    #[arg(long, global = true)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(clap::Subcommand)]
pub enum Commands {
    /// Show version information
    Version,
}

impl Cli {
    pub async fn run(self) -> Result<()> {
        match self.command {
            Some(Commands::Version) => {
                crate::banner::print_banner_with_version();
                Ok(())
            }
            None => {
                crate::banner::print_banner_with_version();
                Ok(())
            }
        }
    }
}
