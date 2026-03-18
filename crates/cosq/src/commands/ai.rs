//! AI feature management
//!
//! `cosq ai`         — show status
//! `cosq ai test`    — test AI connection
//! `cosq ai enable`  — enable AI for cosq
//! `cosq ai disable` — disable AI for cosq
//! `cosq ai config`  — interactive AI node configuration

use anyhow::Result;

use ailloy::config::Config;
use ailloy::config_tui;

use crate::cli::AiCommands;

pub async fn run(cmd: Option<AiCommands>) -> Result<()> {
    match cmd {
        None => config_tui::print_ai_status("cosq", &["chat"]),
        Some(AiCommands::Test { message }) => config_tui::run_test_chat("cosq", message).await,
        Some(AiCommands::Enable) => config_tui::enable_ai("cosq"),
        Some(AiCommands::Disable) => config_tui::disable_ai("cosq"),
        Some(AiCommands::Config) => {
            let mut config = Config::load_global()?;
            config_tui::run_interactive_config(&mut config, &["chat"]).await?;
            Ok(())
        }
    }
}

/// Check if AI features are active (configured via ailloy + enabled for this tool).
pub fn is_ai_active() -> bool {
    config_tui::is_ai_active("cosq")
}
