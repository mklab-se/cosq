//! Azure authentication commands

use anyhow::Result;
use colored::Colorize;
use cosq_client::auth::AzCliAuth;

use crate::cli::AuthCommands;

pub async fn run(cmd: AuthCommands) -> Result<()> {
    match cmd {
        AuthCommands::Status => status().await,
        AuthCommands::Login => login().await,
        AuthCommands::Logout => logout().await,
    }
}

async fn status() -> Result<()> {
    let status = AzCliAuth::check_status().await?;

    if status.logged_in {
        println!("{}", "Azure CLI: logged in".green().bold());
        if let Some(user) = &status.user {
            println!("  {} {}", "User:".bold(), user);
        }
        if let Some(sub) = &status.subscription_name {
            println!("  {} {}", "Subscription:".bold(), sub);
        }
        if let Some(id) = &status.subscription_id {
            println!("  {} {}", "Subscription ID:".bold(), id.dimmed());
        }
        if let Some(tenant) = &status.tenant_id {
            println!("  {} {}", "Tenant:".bold(), tenant.dimmed());
        }

        // Test Cosmos DB token acquisition
        print!("\n  {} ", "Cosmos DB token:".bold());
        match AzCliAuth::get_token(cosq_client::auth::COSMOS_RESOURCE).await {
            Ok(_) => println!("{}", "OK".green()),
            Err(e) => println!("{} ({})", "FAILED".red(), e),
        }
    } else {
        println!("{}", "Azure CLI: not logged in".red().bold());
        println!(
            "\n  Run {} to authenticate.",
            "cosq auth login".cyan().bold()
        );
    }

    Ok(())
}

async fn login() -> Result<()> {
    println!("Opening browser for Azure login...\n");
    AzCliAuth::login().await?;

    let status = AzCliAuth::check_status().await?;
    if status.logged_in {
        println!("\n{}", "Successfully logged in!".green().bold());
        if let Some(user) = &status.user {
            println!("  {} {}", "User:".bold(), user);
        }
    }

    Ok(())
}

async fn logout() -> Result<()> {
    AzCliAuth::logout().await?;
    println!("{}", "Logged out of Azure CLI.".green());
    Ok(())
}
