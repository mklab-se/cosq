//! Shell completion generation
//!
//! Supports two modes:
//! - Static (AOT): `cosq completion <shell>` generates a static completion script
//! - Dynamic: `source <(COMPLETE=<shell> cosq)` enables dynamic completions
//!   with stored query name tab-completion (handled in main.rs via CompleteEnv)

use std::io;

use clap::CommandFactory;
use clap_complete::generate;
use colored::Colorize;

use crate::cli::{Cli, Shell};

/// Generate shell completions and write them to stdout.
pub fn generate_completions(shell: Shell) {
    let shell_name = match shell {
        Shell::Bash => "bash",
        Shell::Zsh => "zsh",
        Shell::Fish => "fish",
        Shell::Powershell => "powershell",
    };

    let clap_shell = match shell {
        Shell::Bash => clap_complete::Shell::Bash,
        Shell::Zsh => clap_complete::Shell::Zsh,
        Shell::Fish => clap_complete::Shell::Fish,
        Shell::Powershell => clap_complete::Shell::PowerShell,
    };

    let mut cmd = Cli::command();
    generate(clap_shell, &mut cmd, "cosq", &mut io::stdout());

    // Print a hint about dynamic completions to stderr
    eprintln!();
    eprintln!(
        "{} For dynamic completions (with stored query name tab-completion), use instead:",
        "Tip:".bold()
    );
    eprintln!(
        "  {}",
        format!("source <(COMPLETE={shell_name} cosq)").cyan()
    );
}
