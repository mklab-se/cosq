//! Shell completion generation

use std::io;

use clap::CommandFactory;
use clap_complete::generate;

use crate::cli::{Cli, Shell};

/// Generate shell completions and write them to stdout.
pub fn generate_completions(shell: Shell) {
    let shell = match shell {
        Shell::Bash => clap_complete::Shell::Bash,
        Shell::Zsh => clap_complete::Shell::Zsh,
        Shell::Fish => clap_complete::Shell::Fish,
        Shell::Powershell => clap_complete::Shell::PowerShell,
    };

    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "cosq", &mut io::stdout());
}
