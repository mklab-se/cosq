//! Interactive initialization command
//!
//! Discovers Azure subscriptions and Cosmos DB accounts, then saves
//! the selection to a local `cosq.yaml` config file. Also ensures
//! the user has Cosmos DB data plane access (RBAC).

use anyhow::{Context, Result, bail};
use colored::Colorize;
use cosq_client::arm::ArmClient;
use cosq_client::auth::AzCliAuth;
use cosq_core::config::{AccountConfig, Config};
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, FuzzySelect};

pub struct InitArgs {
    pub account: Option<String>,
    pub subscription: Option<String>,
    pub yes: bool,
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

    // Step 4: Ensure data plane access
    ensure_data_plane_access(&arm, &account, args.yes).await?;

    // Step 5: Save config
    let config = Config {
        account: AccountConfig {
            name: account.name.clone(),
            subscription: subscription_id,
            resource_group: account.resource_group.clone(),
            endpoint: account.endpoint.clone(),
        },
        database: None,
        container: None,
    };

    let config_path = config.save()?;

    println!(
        "\n{} Saved configuration to {}",
        "Done!".green().bold(),
        config_path.display().to_string().cyan()
    );
    println!("  {} {}", "Account:".bold(), account.name);
    println!("  {} {}", "Endpoint:".bold(), account.endpoint.dimmed());

    Ok(())
}

/// Check if the user has Cosmos DB data plane access and offer to set it up.
async fn ensure_data_plane_access(
    arm: &ArmClient,
    account: &cosq_client::arm::CosmosAccount,
    auto_confirm: bool,
) -> Result<()> {
    println!("\n{}", "Checking data plane access...".dimmed());

    let principal_id = AzCliAuth::get_principal_id().await?;

    match arm.has_cosmos_data_role(&account.id, &principal_id).await {
        Ok(true) => {
            println!("  {} Data plane access is configured.", "OK".green().bold());
            return Ok(());
        }
        Ok(false) => {
            // No role assigned — offer to set it up
        }
        Err(e) => {
            // Can't check (e.g. insufficient permissions) — warn and continue
            println!(
                "  {} Could not verify data plane access: {}",
                "Warning:".yellow().bold(),
                e
            );
            println!("  If queries fail, you may need to assign a Cosmos DB data plane role.");
            return Ok(());
        }
    }

    println!(
        "\n{} Your account does not have Cosmos DB {} access.",
        "!".yellow().bold(),
        "data plane".bold()
    );
    println!(
        "  This is required to run queries. cosq can assign the {} role for you.",
        "Data Contributor".cyan()
    );

    let confirm = if auto_confirm {
        true
    } else {
        Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Grant data plane access now?")
            .default(true)
            .interact()
            .context("confirmation cancelled")?
    };

    if !confirm {
        println!(
            "\n  {} You can assign the role manually later:",
            "Skipped.".yellow()
        );
        println!("  az cosmosdb sql role assignment create \\",);
        println!("    --account-name {} \\", account.name);
        println!("    --resource-group {} \\", account.resource_group);
        println!("    --role-definition-id 00000000-0000-0000-0000-000000000002 \\");
        println!("    --principal-id {principal_id} --scope /");
        return Ok(());
    }

    arm.assign_cosmos_data_contributor(&account.id, &principal_id)
        .await
        .context("failed to assign data plane role")?;

    println!("  {} Data plane access granted.", "OK".green().bold());
    println!(
        "  {} RBAC changes may take a few seconds to propagate.",
        "Note:".dimmed()
    );

    Ok(())
}
