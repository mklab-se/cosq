//! Schema cards: cached, reviewable knowledge about a container's shape.
//!
//! Built once from sampled documents (optionally distilled by AI), then
//! reused by every AI feature (`ask`, `queries generate`, `search`) instead
//! of re-sampling per invocation. Cards are plain YAML files — reviewable,
//! editable, and committable (`./.cosq/schema/` overrides the user cache).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One field observed in the container's documents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldInfo {
    /// Dotted path from the document root, e.g. `address.zip`.
    pub path: String,
    /// Observed JSON types (`string`, `number`, `bool`, `array`, `object`, `null`).
    #[serde(default)]
    pub types: Vec<String>,
    /// A representative example value (truncated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    /// Known value set for low-cardinality fields (e.g. statuses).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    /// AI-written one-liner about the field's meaning (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A suspected reference to another container.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Relationship {
    /// Field in this container, e.g. `customerId`.
    pub field: String,
    /// Referenced container and field, e.g. `customers.id`.
    pub references: String,
    /// low | medium | high
    #[serde(default)]
    pub confidence: String,
}

/// The card itself.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SchemaCard {
    pub database: String,
    pub container: String,
    /// RFC3339 build timestamp.
    pub built_at: String,
    #[serde(default)]
    pub partition_key: Vec<String>,
    #[serde(default)]
    pub fields: Vec<FieldInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relationships: Vec<Relationship>,
    /// Vector policy: (path, dimensions, distance function).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector: Option<(String, u32, String)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub full_text_paths: Vec<String>,
    /// The ailloy embed node confirmed for `cosq search` on this container.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embed_node: Option<String>,
}

impl SchemaCard {
    /// User-cache path for a card.
    pub fn cache_path(profile: &str, database: &str, container: &str) -> PathBuf {
        let base = std::env::var("COSQ_SCHEMA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_default()
                    .join(".cosq")
                    .join("schema")
            });
        base.join(profile)
            .join(database)
            .join(format!("{container}.yaml"))
    }

    /// Project-local override path (`./.cosq/schema/<db>/<container>.yaml`),
    /// searched upward from the current directory.
    pub fn project_path(database: &str, container: &str) -> Option<PathBuf> {
        let mut dir = std::env::current_dir().ok()?;
        loop {
            let candidate = dir
                .join(".cosq")
                .join("schema")
                .join(database)
                .join(format!("{container}.yaml"));
            if candidate.exists() {
                return Some(candidate);
            }
            if !dir.pop() {
                return None;
            }
        }
    }

    /// Load a card: project-local file wins, then the user cache.
    pub fn load(profile: &str, database: &str, container: &str) -> Option<(Self, PathBuf)> {
        let candidates = [
            Self::project_path(database, container),
            Some(Self::cache_path(profile, database, container)),
        ];
        for path in candidates.into_iter().flatten() {
            if let Ok(text) = std::fs::read_to_string(&path)
                && let Ok(card) = serde_yaml::from_str::<SchemaCard>(&text)
            {
                return Some((card, path));
            }
        }
        None
    }

    /// Save into the user cache (never touches project-local files).
    pub fn save(&self, profile: &str) -> anyhow::Result<PathBuf> {
        let path = Self::cache_path(profile, &self.database, &self.container);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_yaml::to_string(self)?)?;
        Ok(path)
    }

    /// Older than the TTL (`COSQ_SCHEMA_TTL_DAYS`, default 7)?
    pub fn is_stale(&self) -> bool {
        let ttl_days: i64 = std::env::var("COSQ_SCHEMA_TTL_DAYS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(7);
        match chrono::DateTime::parse_from_rfc3339(&self.built_at) {
            Ok(built) => {
                chrono::Utc::now().signed_duration_since(built.with_timezone(&chrono::Utc))
                    > chrono::Duration::days(ttl_days)
            }
            Err(_) => true,
        }
    }

    /// Compact YAML for inclusion in AI prompts.
    pub fn to_prompt_yaml(&self) -> String {
        serde_yaml::to_string(self).unwrap_or_default()
    }
}

/// Mechanically derive field infos from sampled documents (no AI):
/// paths, observed types, an example value, and low-cardinality value sets.
/// (observed types, example, candidate value set) per field path.
type FieldAcc = (Vec<String>, Option<String>, Vec<String>);

pub fn fields_from_samples(samples: &[Value]) -> Vec<FieldInfo> {
    use std::collections::BTreeMap;
    let mut acc: BTreeMap<String, FieldAcc> = BTreeMap::new();

    fn type_name(v: &Value) -> &'static str {
        match v {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
    }

    fn walk(prefix: &str, v: &Value, acc: &mut std::collections::BTreeMap<String, FieldAcc>) {
        if let Value::Object(map) = v {
            for (k, val) in map {
                if k.starts_with('_') {
                    continue; // Cosmos system fields (_rid, _ts, _etag, ...)
                }
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                let entry = acc.entry(path.clone()).or_default();
                let t = type_name(val).to_string();
                if !entry.0.contains(&t) {
                    entry.0.push(t);
                }
                match val {
                    Value::Object(_) => walk(&path, val, acc),
                    Value::Array(items) => {
                        if entry.1.is_none() {
                            entry.1 = Some(format!("[{} items]", items.len()));
                        }
                    }
                    other => {
                        let text = match other {
                            Value::String(s) => s.clone(),
                            v => v.to_string(),
                        };
                        let truncated: String = text.chars().take(60).collect();
                        if entry.1.is_none() {
                            entry.1 = Some(truncated.clone());
                        }
                        if matches!(other, Value::String(_))
                            && truncated.len() <= 24
                            && !entry.2.contains(&truncated)
                            && entry.2.len() < 8
                        {
                            entry.2.push(truncated);
                        }
                    }
                }
            }
        }
    }

    for sample in samples {
        walk("", sample, &mut acc);
    }
    acc.into_iter()
        .map(|(path, (types, example, values))| FieldInfo {
            path,
            types,
            example,
            // only keep the value set when it looks low-cardinality vs samples
            values: if values.len() < samples.len().max(2) {
                values
            } else {
                Vec::new()
            },
            description: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_docs() -> Vec<Value> {
        vec![
            json!({"id": "1", "status": "shipped", "amount": 12.5, "_ts": 111,
                   "customer": {"id": "c1", "zip": "12345"}, "tags": ["a", "b"]}),
            json!({"id": "2", "status": "pending", "amount": 3, "_rid": "x",
                   "customer": {"id": "c2", "zip": "54321"}, "tags": []}),
        ]
    }

    #[test]
    fn mechanical_fields_extraction() {
        let fields = fields_from_samples(&sample_docs());
        let paths: Vec<&str> = fields.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"id"));
        assert!(paths.contains(&"status"));
        assert!(paths.contains(&"customer.id"));
        assert!(paths.contains(&"customer.zip"));
        assert!(paths.contains(&"tags"));
        assert!(
            !paths.iter().any(|p| p.starts_with('_')),
            "system fields dropped"
        );
        let status = fields.iter().find(|f| f.path == "status").unwrap();
        assert_eq!(status.types, vec!["string"]);
        let amount = fields.iter().find(|f| f.path == "amount").unwrap();
        assert_eq!(amount.types, vec!["number"]);
    }

    #[test]
    fn yaml_round_trip_and_staleness() {
        let card = SchemaCard {
            database: "db".into(),
            container: "orders".into(),
            built_at: chrono::Utc::now().to_rfc3339(),
            partition_key: vec!["/customerId".into()],
            fields: fields_from_samples(&sample_docs()),
            relationships: vec![Relationship {
                field: "customer.id".into(),
                references: "customers.id".into(),
                confidence: "high".into(),
            }],
            vector: Some(("/embedding".into(), 3072, "cosine".into())),
            full_text_paths: vec!["/text".into()],
            embed_node: None,
        };
        let yaml = serde_yaml::to_string(&card).unwrap();
        let back: SchemaCard = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back.container, "orders");
        assert_eq!(back.vector.as_ref().unwrap().1, 3072);
        assert!(!back.is_stale());

        let old = SchemaCard {
            built_at: "2020-01-01T00:00:00Z".into(),
            ..back
        };
        assert!(old.is_stale());
    }

    #[test]
    fn cache_path_layout() {
        unsafe { std::env::set_var("COSQ_SCHEMA_DIR", "/tmp/cosq-schema-test") };
        let p = SchemaCard::cache_path("work", "appdb", "orders");
        assert!(p.ends_with("work/appdb/orders.yaml"));
        unsafe { std::env::remove_var("COSQ_SCHEMA_DIR") };
    }
}
