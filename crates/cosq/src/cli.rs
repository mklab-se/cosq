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
    /// Execute a SQL query against Cosmos DB
    Query {
        /// SQL query string
        sql: String,

        /// Database name (overrides cosq.yaml)
        #[arg(long)]
        db: Option<String>,

        /// Container name (overrides cosq.yaml)
        #[arg(long)]
        container: Option<String>,
    },

    /// Initialize cosq with a Cosmos DB account
    Init {
        /// Cosmos DB account name (skip interactive selection)
        #[arg(long)]
        account: Option<String>,

        /// Azure subscription ID (skip interactive selection)
        #[arg(long)]
        subscription: Option<String>,

        /// Auto-confirm prompts (e.g. RBAC role assignment)
        #[arg(long, short)]
        yes: bool,
    },

    /// Manage Azure authentication
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },

    /// Generate shell completions
    Completion {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Show version information
    Version,
}

#[derive(clap::Subcommand)]
pub enum AuthCommands {
    /// Show Azure CLI login status
    Status,
    /// Login to Azure (opens browser)
    Login,
    /// Logout from Azure
    Logout,
}

#[derive(Clone, clap::ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Powershell,
}

impl Cli {
    pub async fn run(self) -> Result<()> {
        match self.command {
            Some(Commands::Query { sql, db, container }) => {
                crate::commands::query::run(crate::commands::query::QueryArgs {
                    sql,
                    db,
                    container,
                    quiet: self.quiet,
                })
                .await
            }
            Some(Commands::Init {
                account,
                subscription,
                yes,
            }) => {
                crate::commands::init::run(crate::commands::init::InitArgs {
                    account,
                    subscription,
                    yes,
                })
                .await
            }
            Some(Commands::Auth { command }) => crate::commands::auth::run(command).await,
            Some(Commands::Completion { shell }) => {
                crate::commands::completion::generate_completions(shell);
                Ok(())
            }
            Some(Commands::Version) => {
                crate::banner::print_banner_with_version();
                Ok(())
            }
            None => {
                // Show help when no subcommand is given
                use clap::CommandFactory;
                let mut cmd = Self::command();
                cmd.print_help()?;
                println!();
                Ok(())
            }
        }
    }
}
