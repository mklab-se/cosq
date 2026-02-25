//! Interactive initialization command
//!
//! Discovers Azure subscriptions and Cosmos DB accounts, then saves
//! the selection to a local `cosq.yaml` config file.

use anyhow::{Context, Result, bail};
use colored::Colorize;
use cosq_client::arm::ArmClient;
use cosq_client::auth::AzCliAuth;
use cosq_core::config::{AccountConfig, Config};
use dialoguer::FuzzySelect;
use dialoguer::theme::ColorfulTheme;

pub struct InitArgs {
    pub account: Option<String>,
    pub subscription: Option<String>,
}

pub async fn run(args: InitArgs) -> Result<()> {
    // Step 1: Check Azure auth
    let status = AzCliAuth::check_status().await?;
    if !status.logged_in {
        println!(
            "{} You are not logged in to Azure CLI.",
            "!".yellow().bold()
        );
        println!("  Run {} first.\n", "cosq auth login".cyan().bold());
        bail!("Azure authentication required. Run `cosq auth login` first.");
    }

    println!(
        "{} {}",
        "Logged in as:".bold(),
        status.user.as_deref().unwrap_or("unknown")
    );

    let arm = ArmClient::new().await?;

    // Step 2: Select subscription
    let subscription_id = if let Some(sub_id) = args.subscription {
        println!("{} {}", "Using subscription:".bold(), sub_id);
        sub_id
    } else {
        let subs = arm.list_subscriptions().await?;
        if subs.is_empty() {
            bail!("No enabled Azure subscriptions found for this account.");
        }

        if subs.len() == 1 {
            let sub = &subs[0];
            println!(
                "{} {} ({})",
                "Using subscription:".bold(),
                sub.display_name.green(),
                sub.subscription_id.dimmed()
            );
            sub.subscription_id.clone()
        } else {
            let labels: Vec<String> = subs
                .iter()
                .map(|s| format!("{} ({})", s.display_name, s.subscription_id))
                .collect();

            let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
                .with_prompt("Select a subscription")
                .items(&labels)
                .default(0)
                .interact()
                .context("subscription selection cancelled")?;

            let sub = &subs[selection];
            println!("  {} {}", "Selected:".dimmed(), sub.display_name.green());
            sub.subscription_id.clone()
        }
    };

    // Step 3: Select Cosmos DB account
    let accounts = arm.list_cosmos_accounts(&subscription_id).await?;
    if accounts.is_empty() {
        bail!(
            "No Cosmos DB accounts found in subscription {}.",
            subscription_id
        );
    }

    let account = if let Some(account_name) = args.account {
        accounts
            .into_iter()
            .find(|a| a.name == account_name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Cosmos DB account '{}' not found in subscription",
                    account_name
                )
            })?
    } else if accounts.len() == 1 {
        let acct = &accounts[0];
        println!(
            "{} {} ({})",
            "Using Cosmos DB account:".bold(),
            acct.name.green(),
            acct.location.dimmed()
        );
        accounts.into_iter().next().unwrap()
    } else {
        let labels: Vec<String> = accounts
            .iter()
            .map(|a| {
                format!(
                    "{} [{}] ({})",
                    a.name,
                    a.kind.as_deref().unwrap_or("unknown"),
                    a.location
                )
            })
            .collect();

        let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
            .with_prompt("Select a Cosmos DB account")
            .items(&labels)
            .default(0)
            .interact()
            .context("account selection cancelled")?;

        let acct = &accounts[selection];
        println!("  {} {}", "Selected:".dimmed(), acct.name.green());
        accounts.into_iter().nth(selection).unwrap()
    };

    // Step 4: Save config
    let config = Config {
        account: AccountConfig {
            name: account.name.clone(),
            subscription: subscription_id,
            resource_group: account.resource_group.clone(),
            endpoint: account.endpoint.clone(),
        },
    };

    let cwd = std::env::current_dir().context("failed to get current directory")?;
    let config_path = config.save(&cwd)?;

    println!(
        "\n{} Saved configuration to {}",
        "Done!".green().bold(),
        config_path.display().to_string().cyan()
    );
    println!("  {} {}", "Account:".bold(), account.name);
    println!("  {} {}", "Endpoint:".bold(), account.endpoint.dimmed());

    Ok(())
}
