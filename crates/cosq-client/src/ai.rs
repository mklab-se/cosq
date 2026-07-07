//! Unified AI text generation via ailloy
//!
//! Uses the globally configured ailloy provider for AI requests.

use ailloy::{ChatOptions, Client, Message};

/// Generate text using the globally configured ailloy provider.
///
/// Uses the default chat node from `~/.config/ailloy/config.yaml`.
/// Run `ailloy config` to set up a provider.
pub async fn generate_text(system_prompt: &str, user_prompt: &str) -> anyhow::Result<String> {
    generate_text_with_limit(system_prompt, user_prompt, 2000).await
}

/// Generate text with a custom max_tokens limit.
pub async fn generate_text_with_limit(
    system_prompt: &str,
    user_prompt: &str,
    max_tokens: u32,
) -> anyhow::Result<String> {
    let client = Client::from_config()?;
    let opts = ChatOptions::builder().max_tokens(max_tokens).build();
    let response = client
        .chat_with(
            &[Message::system(system_prompt), Message::user(user_prompt)],
            &opts,
        )
        .await?;
    Ok(response.content)
}

/// Generate a JSON document matching `schema` (strict where the provider
/// supports it), parsed and returned as a value.
pub async fn generate_json(
    system_prompt: &str,
    user_prompt: &str,
    schema_name: &str,
    schema: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    generate_json_with(
        system_prompt,
        &[Message::user(user_prompt)],
        schema_name,
        schema,
    )
    .await
}

/// `generate_json` over a full message history (for conversational asks).
pub async fn generate_json_with(
    system_prompt: &str,
    messages: &[Message],
    schema_name: &str,
    schema: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let client = Client::from_config()?;
    let opts = ChatOptions::builder()
        .max_tokens(4000)
        .json_schema(schema_name, schema)
        .build();
    let mut all = vec![Message::system(system_prompt)];
    all.extend_from_slice(messages);
    let response = client.chat_with(&all, &opts).await?;
    let text = response.content.trim();
    let text = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
        .unwrap_or(text)
        .trim_end_matches("```")
        .trim();
    serde_json::from_str(text)
        .map_err(|e| anyhow::anyhow!("AI returned invalid JSON ({e}): {text}"))
}

/// Check if ailloy is configured with a default chat node.
pub fn is_configured() -> bool {
    ailloy::config::Config::load()
        .map(|c| c.default_chat_node().is_ok())
        .unwrap_or(false)
}

/// Get a display name for the currently configured provider.
pub fn provider_display_name() -> Option<String> {
    let config = ailloy::config::Config::load().ok()?;
    let (id, _node) = config.default_chat_node().ok()?;
    Some(id.to_string())
}
