//! CLI argument definitions using clap

use anyhow::Result;
use clap::Parser;
use clap_complete::engine::{ArgValueCandidates, CompletionCandidate};

use crate::output::OutputFormat;

/// Provide tab-completion candidates for stored query names
fn complete_query_names() -> Vec<CompletionCandidate> {
    cosq_core::stored_query::list_query_names()
        .into_iter()
        .map(|(name, desc)| {
            let mut candidate = CompletionCandidate::new(name);
            if let Some(d) = desc {
                candidate = candidate.help(Some(d.into()));
            }
            candidate
        })
        .collect()
}

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

        /// Database name (overrides config)
        #[arg(long)]
        db: Option<String>,

        /// Container name (overrides config)
        #[arg(long)]
        container: Option<String>,

        /// Output format
        #[arg(long, short, value_enum)]
        output: Option<OutputFormat>,

        /// Path to a MiniJinja template file for output formatting
        #[arg(long)]
        template: Option<String>,
    },

    /// Execute a stored query by name (interactive picker if no name given)
    Run {
        /// Name of the stored query (with or without .cosq extension)
        #[arg(add = ArgValueCandidates::new(complete_query_names))]
        name: Option<String>,

        /// Database name (overrides query metadata and config)
        #[arg(long)]
        db: Option<String>,

        /// Container name (overrides query metadata and config)
        #[arg(long)]
        container: Option<String>,

        /// Output format (auto-detects template from query if available)
        #[arg(long, short, value_enum)]
        output: Option<OutputFormat>,

        /// Path to a MiniJinja template file for output formatting
        #[arg(long)]
        template: Option<String>,

        /// Query parameters (passed as trailing args: -- --param1 value1 --param2 value2)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        params: Vec<String>,
    },

    /// Manage stored queries
    Queries {
        #[command(subcommand)]
        command: QueriesCommands,
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

    /// Configure AI provider for query generation
    Ai {
        #[command(subcommand)]
        command: AiCommands,
    },

    /// Show version information
    Version,
}

#[derive(clap::Subcommand)]
pub enum QueriesCommands {
    /// List all stored queries
    List,

    /// Create a new stored query (opens in editor)
    Create {
        /// Name for the query (becomes the .cosq filename)
        name: String,

        /// Create in project directory (.cosq/queries/) instead of user directory
        #[arg(long)]
        project: bool,
    },

    /// Edit a stored query in your default editor
    Edit {
        /// Name of the query to edit
        #[arg(add = ArgValueCandidates::new(complete_query_names))]
        name: String,
    },

    /// Delete a stored query
    Delete {
        /// Name of the query to delete
        #[arg(add = ArgValueCandidates::new(complete_query_names))]
        name: String,

        /// Skip confirmation prompt
        #[arg(long, short)]
        yes: bool,
    },

    /// Show details of a stored query
    Show {
        /// Name of the query to show
        #[arg(add = ArgValueCandidates::new(complete_query_names))]
        name: String,
    },

    /// Generate a stored query from a natural language description (requires AI config)
    Generate {
        /// Natural language description of the query
        description: String,

        /// Save to project directory (.cosq/queries/) instead of user directory
        #[arg(long)]
        project: bool,
    },
}

#[derive(clap::Subcommand)]
pub enum AiCommands {
    /// Set up AI provider for query generation (local CLI agents, Ollama, or Azure OpenAI)
    Init,
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
            Some(Commands::Query {
                sql,
                db,
                container,
                output,
                template,
            }) => {
                crate::commands::query::run(crate::commands::query::QueryArgs {
                    sql,
                    db,
                    container,
                    output,
                    template,
                    quiet: self.quiet,
                })
                .await
            }
            Some(Commands::Run {
                name,
                db,
                container,
                output,
                template,
                params,
            }) => {
                crate::commands::run::run(crate::commands::run::RunArgs {
                    name,
                    params,
                    output,
                    db,
                    container,
                    template,
                    quiet: self.quiet,
                })
                .await
            }
            Some(Commands::Queries { command }) => {
                crate::commands::queries::run(command, self.quiet).await
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
            Some(Commands::Ai { command }) => crate::commands::ai::run(command).await,
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
