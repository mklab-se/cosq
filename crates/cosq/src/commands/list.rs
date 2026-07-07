//! `cosq databases` / `cosq containers` — quick listings.

use anyhow::Result;
use colored::Colorize;
use cosq_client::cosmos::CosmosClient;
use cosq_core::config::Config;

pub async fn databases(json: bool) -> Result<()> {
    let config = Config::load()?;
    let (profile_name, profile) = config.active(None)?;
    let client = CosmosClient::new(&profile.account.endpoint).await?;
    let names = client.list_databases().await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&names)?);
    } else {
        eprintln!(
            "{} {} ({})",
            "Databases in".dimmed(),
            profile.account.name.bold(),
            profile_name.dimmed()
        );
        for name in &names {
            println!("{name}");
        }
    }
    Ok(())
}

pub async fn containers(db: Option<String>, json: bool) -> Result<()> {
    let mut config = Config::load()?;
    let (_, profile) = config.active_mut(None)?;
    let mut profile = profile.clone();
    let client = CosmosClient::new(&profile.account.endpoint).await?;
    let (database, _) = super::common::resolve_database(&client, &mut profile, db, None).await?;

    let names = client.list_containers(&database).await?;
    if json {
        let mut out = Vec::new();
        for name in &names {
            let meta = client.get_container(&database, name).await.ok();
            out.push(serde_json::json!({
                "name": name,
                "partitionKey": meta.as_ref().map(|m| m.pk_paths.clone()),
                "vectorSearch": meta.as_ref().map(|m| !m.vector_paths.is_empty()),
                "fullTextSearch": meta.as_ref().map(|m| !m.full_text_paths.is_empty()),
            }));
        }
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        eprintln!("{} {}", "Containers in".dimmed(), database.bold());
        for name in &names {
            let meta = client.get_container(&database, name).await.ok();
            let mut extras = Vec::new();
            if let Some(m) = &meta {
                extras.push(format!("pk: {}", m.pk_paths.join(", ")));
                if !m.vector_paths.is_empty() {
                    extras.push("vector".to_string());
                }
                if !m.full_text_paths.is_empty() {
                    extras.push("full-text".to_string());
                }
            }
            if extras.is_empty() {
                println!("{name}");
            } else {
                println!("{name}  {}", format!("({})", extras.join(", ")).dimmed());
            }
        }
    }
    Ok(())
}
