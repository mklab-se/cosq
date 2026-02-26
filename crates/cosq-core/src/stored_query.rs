//! Stored query file format (.cosq)
//!
//! Stored queries use YAML front matter (between `---` delimiters) for metadata,
//! followed by the SQL query body. They are stored in `~/.cosq/queries/` (user-level)
//! or `.cosq/queries/` (project-level).
//!
//! Example:
//! ```text
//! ---
//! description: Find users who signed up recently
//! database: mydb
//! container: users
//! params:
//!   - name: days
//!     type: number
//!     description: Number of days to look back
//!     default: 30
//! ---
//! SELECT c.id, c.email, c.displayName, c.createdAt
//! FROM c
//! WHERE c.createdAt >= DateTimeAdd("dd", -@days, GetCurrentDateTime())
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoredQueryError {
    #[error("invalid query file: missing front matter delimiters (---)")]
    MissingFrontMatter,

    #[error("failed to parse query metadata: {0}")]
    InvalidMetadata(#[from] serde_yaml::Error),

    #[error("query file has no SQL body")]
    EmptyQuery,

    #[error("failed to read query file: {0}")]
    Read(#[from] std::io::Error),

    #[error("parameter '{name}' is required")]
    MissingParam { name: String },

    #[error("parameter '{name}': expected {expected}, got '{value}'")]
    InvalidParamType {
        name: String,
        expected: String,
        value: String,
    },

    #[error("parameter '{name}': value {value} is below minimum {min}")]
    BelowMin { name: String, value: f64, min: f64 },

    #[error("parameter '{name}': value {value} exceeds maximum {max}")]
    AboveMax { name: String, value: f64, max: f64 },

    #[error("parameter '{name}': '{value}' is not one of the allowed values: {choices}")]
    InvalidChoice {
        name: String,
        value: String,
        choices: String,
    },

    #[error("parameter '{name}': value '{value}' does not match pattern '{pattern}'")]
    PatternMismatch {
        name: String,
        value: String,
        pattern: String,
    },

    #[error("no queries directory found")]
    NoQueriesDir,
}

/// Parameter type for stored query parameters
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ParamType {
    String,
    Number,
    Bool,
}

impl std::fmt::Display for ParamType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParamType::String => write!(f, "string"),
            ParamType::Number => write!(f, "number"),
            ParamType::Bool => write!(f, "bool"),
        }
    }
}

/// A parameter definition within a stored query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamDef {
    /// Parameter name (used as @name in SQL)
    pub name: String,

    /// Parameter type
    #[serde(rename = "type")]
    pub param_type: ParamType,

    /// Human-readable description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Default value
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,

    /// Allowed values (shown as fuzzy-select in interactive mode)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<serde_json::Value>>,

    /// Whether the parameter is required (defaults to true if no default/choices)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,

    /// Minimum value (for number type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,

    /// Maximum value (for number type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,

    /// Regex pattern (for string type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
}

impl ParamDef {
    /// Whether this parameter is required (true if no default and no choices)
    pub fn is_required(&self) -> bool {
        self.required
            .unwrap_or_else(|| self.default.is_none() && self.choices.is_none())
    }

    /// Validate a resolved value against this parameter's constraints
    pub fn validate(&self, value: &serde_json::Value) -> Result<(), StoredQueryError> {
        // Type check
        match self.param_type {
            ParamType::String => {
                if !value.is_string() {
                    return Err(StoredQueryError::InvalidParamType {
                        name: self.name.clone(),
                        expected: "string".into(),
                        value: value.to_string(),
                    });
                }
            }
            ParamType::Number => {
                if !value.is_number() {
                    return Err(StoredQueryError::InvalidParamType {
                        name: self.name.clone(),
                        expected: "number".into(),
                        value: value.to_string(),
                    });
                }
            }
            ParamType::Bool => {
                if !value.is_boolean() {
                    return Err(StoredQueryError::InvalidParamType {
                        name: self.name.clone(),
                        expected: "bool".into(),
                        value: value.to_string(),
                    });
                }
            }
        }

        // Range check for numbers
        if let Some(num) = value.as_f64() {
            if let Some(min) = self.min {
                if num < min {
                    return Err(StoredQueryError::BelowMin {
                        name: self.name.clone(),
                        value: num,
                        min,
                    });
                }
            }
            if let Some(max) = self.max {
                if num > max {
                    return Err(StoredQueryError::AboveMax {
                        name: self.name.clone(),
                        value: num,
                        max,
                    });
                }
            }
        }

        // Choice validation
        if let Some(ref choices) = self.choices {
            if !choices.contains(value) {
                let choices_str = choices
                    .iter()
                    .map(|c| match c {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(StoredQueryError::InvalidChoice {
                    name: self.name.clone(),
                    value: match value {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    },
                    choices: choices_str,
                });
            }
        }

        // Pattern check for strings
        if let (Some(pattern), Some(s)) = (&self.pattern, value.as_str()) {
            let re = regex::Regex::new(pattern).map_err(|_| StoredQueryError::PatternMismatch {
                name: self.name.clone(),
                value: s.to_string(),
                pattern: pattern.clone(),
            })?;
            if !re.is_match(s) {
                return Err(StoredQueryError::PatternMismatch {
                    name: self.name.clone(),
                    value: s.to_string(),
                    pattern: pattern.clone(),
                });
            }
        }

        Ok(())
    }
}

/// YAML front matter metadata for a stored query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredQueryMetadata {
    /// Brief description of what the query does
    pub description: String,

    /// Target database (overrides config default)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,

    /// Target container (overrides config default)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,

    /// Parameter definitions
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<ParamDef>,

    /// Inline output template (MiniJinja)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,

    /// Path to external template file
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_file: Option<String>,

    /// Marks this query as AI-generated
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,

    /// The original natural language prompt (for AI-generated queries)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_from: Option<String>,
}

/// A fully parsed stored query
#[derive(Debug, Clone)]
pub struct StoredQuery {
    /// The file name (without .cosq extension)
    pub name: String,

    /// Query metadata from YAML front matter
    pub metadata: StoredQueryMetadata,

    /// The SQL query body
    pub sql: String,
}

impl StoredQuery {
    /// Parse a .cosq file from its contents
    pub fn parse(name: &str, contents: &str) -> Result<Self, StoredQueryError> {
        let (metadata, sql) = parse_front_matter(contents)?;
        let sql = sql.trim().to_string();
        if sql.is_empty() {
            return Err(StoredQueryError::EmptyQuery);
        }
        Ok(Self {
            name: name.to_string(),
            metadata,
            sql,
        })
    }

    /// Load a stored query from a file path
    pub fn load(path: &Path) -> Result<Self, StoredQueryError> {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let contents = std::fs::read_to_string(path)?;
        Self::parse(&name, &contents)
    }

    /// Serialize this stored query back to .cosq file format
    pub fn to_file_contents(&self) -> Result<String, serde_yaml::Error> {
        let yaml = serde_yaml::to_string(&self.metadata)?;
        Ok(format!("---\n{}---\n{}\n", yaml, self.sql))
    }

    /// Resolve parameters from a map of CLI-provided values, filling in defaults.
    /// Returns a map of parameter name → resolved JSON value.
    pub fn resolve_params(
        &self,
        provided: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, serde_json::Value>, StoredQueryError> {
        let mut resolved = BTreeMap::new();

        for param in &self.metadata.params {
            let value = if let Some(raw) = provided.get(&param.name) {
                // Parse from string to the expected type
                parse_param_value(&param.name, &param.param_type, raw)?
            } else if let Some(ref default) = param.default {
                default.clone()
            } else if let Some(ref choices) = param.choices {
                if choices.len() == 1 {
                    choices[0].clone()
                } else if param.is_required() {
                    return Err(StoredQueryError::MissingParam {
                        name: param.name.clone(),
                    });
                } else {
                    continue;
                }
            } else if param.is_required() {
                return Err(StoredQueryError::MissingParam {
                    name: param.name.clone(),
                });
            } else {
                continue;
            };

            param.validate(&value)?;
            resolved.insert(param.name.clone(), value);
        }

        Ok(resolved)
    }

    /// Build the Cosmos DB parameters array from resolved parameter values.
    pub fn build_cosmos_params(
        resolved: &BTreeMap<String, serde_json::Value>,
    ) -> Vec<serde_json::Value> {
        resolved
            .iter()
            .map(|(name, value)| {
                serde_json::json!({
                    "name": format!("@{name}"),
                    "value": value
                })
            })
            .collect()
    }
}

/// Parse a string value into the expected parameter type (public API for CLI usage)
pub fn parse_param_value_public(
    name: &str,
    param_type: &ParamType,
    raw: &str,
) -> Result<serde_json::Value, StoredQueryError> {
    parse_param_value(name, param_type, raw)
}

/// Parse a string value into the expected parameter type
fn parse_param_value(
    name: &str,
    param_type: &ParamType,
    raw: &str,
) -> Result<serde_json::Value, StoredQueryError> {
    match param_type {
        ParamType::String => Ok(serde_json::Value::String(raw.to_string())),
        ParamType::Number => {
            if let Ok(i) = raw.parse::<i64>() {
                Ok(serde_json::json!(i))
            } else if let Ok(f) = raw.parse::<f64>() {
                Ok(serde_json::json!(f))
            } else {
                Err(StoredQueryError::InvalidParamType {
                    name: name.to_string(),
                    expected: "number".into(),
                    value: raw.to_string(),
                })
            }
        }
        ParamType::Bool => match raw.to_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(serde_json::Value::Bool(true)),
            "false" | "0" | "no" => Ok(serde_json::Value::Bool(false)),
            _ => Err(StoredQueryError::InvalidParamType {
                name: name.to_string(),
                expected: "bool (true/false)".into(),
                value: raw.to_string(),
            }),
        },
    }
}

/// Parse YAML front matter from file contents
fn parse_front_matter(contents: &str) -> Result<(StoredQueryMetadata, String), StoredQueryError> {
    let trimmed = contents.trim_start();
    if !trimmed.starts_with("---") {
        return Err(StoredQueryError::MissingFrontMatter);
    }

    // Find the closing ---
    let after_first = &trimmed[3..];
    let closing = after_first
        .find("\n---")
        .ok_or(StoredQueryError::MissingFrontMatter)?;

    let yaml_str = &after_first[..closing];
    let rest = &after_first[closing + 4..]; // skip \n---

    let metadata: StoredQueryMetadata = serde_yaml::from_str(yaml_str)?;
    Ok((metadata, rest.to_string()))
}

/// Return the user-level queries directory: `~/.cosq/queries/`
pub fn user_queries_dir() -> Result<PathBuf, StoredQueryError> {
    dirs::home_dir()
        .map(|d| d.join(".cosq").join("queries"))
        .ok_or(StoredQueryError::NoQueriesDir)
}

/// Return the project-level queries directory: `.cosq/queries/` relative to cwd
pub fn project_queries_dir() -> Option<PathBuf> {
    std::env::current_dir()
        .ok()
        .map(|d| d.join(".cosq").join("queries"))
}

/// List all stored queries from both user and project directories.
/// Project-level queries take precedence over user-level queries with the same name.
pub fn list_stored_queries() -> Result<Vec<StoredQuery>, StoredQueryError> {
    let mut queries = BTreeMap::new();

    // Load user-level queries first
    if let Ok(user_dir) = user_queries_dir() {
        if user_dir.is_dir() {
            load_queries_from_dir(&user_dir, &mut queries)?;
        }
    }

    // Load project-level queries (override user-level)
    if let Some(project_dir) = project_queries_dir() {
        if project_dir.is_dir() {
            load_queries_from_dir(&project_dir, &mut queries)?;
        }
    }

    Ok(queries.into_values().collect())
}

/// List stored query names (lightweight — only reads filenames, not file contents).
/// Used for shell tab-completion.
pub fn list_query_names() -> Vec<(String, Option<String>)> {
    // Try full parse first for descriptions; fall back to filenames only
    if let Ok(queries) = list_stored_queries() {
        return queries
            .into_iter()
            .map(|q| (q.name, Some(q.metadata.description)))
            .collect();
    }

    // Fallback: just scan filenames
    let mut names = BTreeMap::new();
    if let Ok(user_dir) = user_queries_dir() {
        if user_dir.is_dir() {
            collect_names_from_dir(&user_dir, &mut names);
        }
    }
    if let Some(project_dir) = project_queries_dir() {
        if project_dir.is_dir() {
            collect_names_from_dir(&project_dir, &mut names);
        }
    }
    names.into_keys().map(|name| (name, None)).collect()
}

fn collect_names_from_dir(dir: &Path, names: &mut BTreeMap<String, ()>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "cosq") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.insert(stem.to_string(), ());
                }
            }
        }
    }
}

/// Find a stored query by name, checking project dir first, then user dir
pub fn find_stored_query(name: &str) -> Result<StoredQuery, StoredQueryError> {
    let filename = if name.ends_with(".cosq") {
        name.to_string()
    } else {
        format!("{name}.cosq")
    };

    // Check project-level first
    if let Some(project_dir) = project_queries_dir() {
        let path = project_dir.join(&filename);
        if path.exists() {
            return StoredQuery::load(&path);
        }
    }

    // Check user-level
    let user_dir = user_queries_dir()?;
    let path = user_dir.join(&filename);
    if path.exists() {
        return StoredQuery::load(&path);
    }

    Err(StoredQueryError::Read(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("stored query '{name}' not found"),
    )))
}

/// Get the path where a stored query should be saved (user-level by default)
pub fn query_file_path(name: &str, project_level: bool) -> Result<PathBuf, StoredQueryError> {
    let filename = if name.ends_with(".cosq") {
        name.to_string()
    } else {
        format!("{name}.cosq")
    };

    if project_level {
        project_queries_dir()
            .map(|d| d.join(filename))
            .ok_or(StoredQueryError::NoQueriesDir)
    } else {
        Ok(user_queries_dir()?.join(filename))
    }
}

fn load_queries_from_dir(
    dir: &Path,
    queries: &mut BTreeMap<String, StoredQuery>,
) -> Result<(), StoredQueryError> {
    let entries = std::fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "cosq") {
            match StoredQuery::load(&path) {
                Ok(query) => {
                    queries.insert(query.name.clone(), query);
                }
                Err(e) => {
                    // Log but don't fail on individual parse errors
                    eprintln!("Warning: skipping {}: {}", path.display(), e);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE_QUERY: &str = r#"---
description: Find users who signed up recently
database: mydb
container: users
params:
  - name: days
    type: number
    description: Number of days to look back
    default: 30
---
SELECT c.id, c.email, c.displayName, c.createdAt
FROM c
WHERE c.createdAt >= DateTimeAdd("dd", -@days, GetCurrentDateTime())
ORDER BY c.createdAt DESC
"#;

    const QUERY_WITH_CHOICES: &str = r#"---
description: List orders by status
database: shop-db
container: orders
params:
  - name: status
    type: string
    description: Order status
    choices: ["pending", "shipped", "delivered"]
    default: "pending"
  - name: limit
    type: number
    default: 50
    min: 1
    max: 1000
---
SELECT TOP @limit * FROM c WHERE c.status = @status
"#;

    const QUERY_WITH_TEMPLATE: &str = r#"---
description: Orders summary
params:
  - name: status
    type: string
    default: "pending"
template: |
  Orders ({{ status }}):
  {% for doc in documents %}
  {{ loop.index }}. #{{ doc.id }} — ${{ doc.total }}
  {% endfor %}
---
SELECT c.id, c.total FROM c WHERE c.status = @status
"#;

    #[test]
    fn test_parse_basic_query() {
        let query = StoredQuery::parse("recent-users", EXAMPLE_QUERY).unwrap();
        assert_eq!(query.name, "recent-users");
        assert_eq!(
            query.metadata.description,
            "Find users who signed up recently"
        );
        assert_eq!(query.metadata.database.as_deref(), Some("mydb"));
        assert_eq!(query.metadata.container.as_deref(), Some("users"));
        assert_eq!(query.metadata.params.len(), 1);
        assert_eq!(query.metadata.params[0].name, "days");
        assert_eq!(query.metadata.params[0].param_type, ParamType::Number);
        assert_eq!(
            query.metadata.params[0].default,
            Some(serde_json::json!(30))
        );
        assert!(query.sql.contains("SELECT"));
        assert!(query.sql.contains("@days"));
    }

    #[test]
    fn test_parse_query_with_choices() {
        let query = StoredQuery::parse("orders", QUERY_WITH_CHOICES).unwrap();
        assert_eq!(query.metadata.params.len(), 2);

        let status_param = &query.metadata.params[0];
        assert_eq!(status_param.name, "status");
        assert_eq!(
            status_param.choices.as_ref().unwrap(),
            &vec![
                serde_json::json!("pending"),
                serde_json::json!("shipped"),
                serde_json::json!("delivered"),
            ]
        );

        let limit_param = &query.metadata.params[1];
        assert_eq!(limit_param.min, Some(1.0));
        assert_eq!(limit_param.max, Some(1000.0));
    }

    #[test]
    fn test_parse_query_with_template() {
        let query = StoredQuery::parse("orders-summary", QUERY_WITH_TEMPLATE).unwrap();
        assert!(query.metadata.template.is_some());
        assert!(
            query
                .metadata
                .template
                .as_ref()
                .unwrap()
                .contains("{% for doc in documents %}")
        );
    }

    #[test]
    fn test_resolve_params_with_defaults() {
        let query = StoredQuery::parse("recent-users", EXAMPLE_QUERY).unwrap();
        let provided = BTreeMap::new();
        let resolved = query.resolve_params(&provided).unwrap();
        assert_eq!(resolved.get("days"), Some(&serde_json::json!(30)));
    }

    #[test]
    fn test_resolve_params_with_cli_values() {
        let query = StoredQuery::parse("recent-users", EXAMPLE_QUERY).unwrap();
        let mut provided = BTreeMap::new();
        provided.insert("days".to_string(), "7".to_string());
        let resolved = query.resolve_params(&provided).unwrap();
        assert_eq!(resolved.get("days"), Some(&serde_json::json!(7)));
    }

    #[test]
    fn test_resolve_params_validation_range() {
        let query = StoredQuery::parse("orders", QUERY_WITH_CHOICES).unwrap();
        let mut provided = BTreeMap::new();
        provided.insert("status".to_string(), "pending".to_string());
        provided.insert("limit".to_string(), "5000".to_string());
        let result = query.resolve_params(&provided);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds maximum"));
    }

    #[test]
    fn test_resolve_params_validation_choices() {
        let query = StoredQuery::parse("orders", QUERY_WITH_CHOICES).unwrap();
        let mut provided = BTreeMap::new();
        provided.insert("status".to_string(), "invalid".to_string());
        provided.insert("limit".to_string(), "10".to_string());
        let result = query.resolve_params(&provided);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not one of the allowed")
        );
    }

    #[test]
    fn test_build_cosmos_params() {
        let mut resolved = BTreeMap::new();
        resolved.insert("days".to_string(), serde_json::json!(7));
        resolved.insert("status".to_string(), serde_json::json!("active"));

        let params = StoredQuery::build_cosmos_params(&resolved);
        assert_eq!(params.len(), 2);

        let days_param = params.iter().find(|p| p["name"] == "@days").unwrap();
        assert_eq!(days_param["value"], 7);

        let status_param = params.iter().find(|p| p["name"] == "@status").unwrap();
        assert_eq!(status_param["value"], "active");
    }

    #[test]
    fn test_roundtrip_serialization() {
        let query = StoredQuery::parse("test", EXAMPLE_QUERY).unwrap();
        let contents = query.to_file_contents().unwrap();
        let reparsed = StoredQuery::parse("test", &contents).unwrap();
        assert_eq!(reparsed.metadata.description, query.metadata.description);
        assert_eq!(reparsed.metadata.database, query.metadata.database);
        assert_eq!(reparsed.metadata.params.len(), query.metadata.params.len());
        assert_eq!(reparsed.sql, query.sql);
    }

    #[test]
    fn test_missing_front_matter() {
        let result = StoredQuery::parse("bad", "SELECT * FROM c");
        assert!(matches!(result, Err(StoredQueryError::MissingFrontMatter)));
    }

    #[test]
    fn test_empty_query() {
        let contents = "---\ndescription: empty\n---\n";
        let result = StoredQuery::parse("empty", contents);
        assert!(matches!(result, Err(StoredQueryError::EmptyQuery)));
    }

    #[test]
    fn test_param_required_without_default() {
        let contents = r#"---
description: test
params:
  - name: id
    type: string
---
SELECT * FROM c WHERE c.id = @id
"#;
        let query = StoredQuery::parse("test", contents).unwrap();
        assert!(query.metadata.params[0].is_required());

        let result = query.resolve_params(&BTreeMap::new());
        assert!(matches!(result, Err(StoredQueryError::MissingParam { .. })));
    }

    #[test]
    fn test_parse_bool_param() {
        let value = parse_param_value("active", &ParamType::Bool, "true").unwrap();
        assert_eq!(value, serde_json::Value::Bool(true));

        let value = parse_param_value("active", &ParamType::Bool, "false").unwrap();
        assert_eq!(value, serde_json::Value::Bool(false));

        let value = parse_param_value("active", &ParamType::Bool, "yes").unwrap();
        assert_eq!(value, serde_json::Value::Bool(true));
    }

    #[test]
    fn test_param_with_pattern() {
        let contents = r#"---
description: test
params:
  - name: email
    type: string
    pattern: "^[^@]+@[^@]+$"
---
SELECT * FROM c WHERE c.email = @email
"#;
        let query = StoredQuery::parse("test", contents).unwrap();

        let mut provided = BTreeMap::new();
        provided.insert("email".to_string(), "user@example.com".to_string());
        assert!(query.resolve_params(&provided).is_ok());

        let mut bad = BTreeMap::new();
        bad.insert("email".to_string(), "not-an-email".to_string());
        assert!(query.resolve_params(&bad).is_err());
    }

    #[test]
    fn test_query_no_params() {
        let contents = r#"---
description: Simple count
---
SELECT VALUE COUNT(1) FROM c
"#;
        let query = StoredQuery::parse("count", contents).unwrap();
        assert!(query.metadata.params.is_empty());
        let resolved = query.resolve_params(&BTreeMap::new()).unwrap();
        assert!(resolved.is_empty());
    }
}
