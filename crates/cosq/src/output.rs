//! Output formatting for query results
//!
//! Supports JSON (default), CSV, table, and MiniJinja template output modes.

use std::collections::BTreeSet;
use std::io::Write;

use anyhow::Result;
use comfy_table::Table;
use comfy_table::presets::UTF8_FULL_CONDENSED;
use serde_json::Value;

/// Output format for query results
#[derive(Debug, Clone, Default, clap::ValueEnum)]
pub enum OutputFormat {
    /// Pretty-printed JSON array (default)
    #[default]
    Json,
    /// Compact JSON (one line per document)
    JsonCompact,
    /// Columnar table
    Table,
    /// Comma-separated values
    Csv,
    /// Use template from stored query or --template file
    Template,
}

/// Format and write query results to the given writer.
pub fn write_results(
    writer: &mut dyn Write,
    documents: &[Value],
    format: &OutputFormat,
) -> Result<()> {
    match format {
        OutputFormat::Json => write_json(writer, documents),
        OutputFormat::JsonCompact => write_json_compact(writer, documents),
        OutputFormat::Table => write_table(writer, documents),
        OutputFormat::Csv => write_csv(writer, documents),
        OutputFormat::Template => {
            // Template output is handled separately by the caller
            write_json(writer, documents)
        }
    }
}

/// Render a MiniJinja template against query results and parameters
pub fn render_template(
    template_str: &str,
    documents: &[Value],
    params: &std::collections::BTreeMap<String, Value>,
) -> Result<String> {
    let mut env = minijinja::Environment::new();
    env.add_template("output", template_str)?;
    let tmpl = env.get_template("output")?;

    let mut context = std::collections::BTreeMap::new();
    context.insert("documents".to_string(), Value::Array(documents.to_vec()));

    // Add parameters as top-level template variables
    for (key, value) in params {
        context.insert(key.clone(), value.clone());
    }

    let rendered = tmpl.render(context)?;
    Ok(rendered)
}

fn write_json(writer: &mut dyn Write, documents: &[Value]) -> Result<()> {
    let json = serde_json::to_string_pretty(documents)?;
    writeln!(writer, "{json}")?;
    Ok(())
}

fn write_json_compact(writer: &mut dyn Write, documents: &[Value]) -> Result<()> {
    for doc in documents {
        let json = serde_json::to_string(doc)?;
        writeln!(writer, "{json}")?;
    }
    Ok(())
}

fn write_table(writer: &mut dyn Write, documents: &[Value]) -> Result<()> {
    if documents.is_empty() {
        writeln!(writer, "(no results)")?;
        return Ok(());
    }

    let columns = collect_columns(documents);

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_header(columns.iter().collect::<Vec<_>>());

    for doc in documents {
        let row: Vec<String> = columns
            .iter()
            .map(|col| format_cell(doc.get(col.as_str())))
            .collect();
        table.add_row(row);
    }

    writeln!(writer, "{table}")?;
    Ok(())
}

fn write_csv(writer: &mut dyn Write, documents: &[Value]) -> Result<()> {
    if documents.is_empty() {
        return Ok(());
    }

    let columns = collect_columns(documents);

    // Header
    writeln!(
        writer,
        "{}",
        columns
            .iter()
            .map(|c| csv_escape(c))
            .collect::<Vec<_>>()
            .join(",")
    )?;

    // Rows
    for doc in documents {
        let row: Vec<String> = columns
            .iter()
            .map(|col| csv_escape(&format_cell(doc.get(col.as_str()))))
            .collect();
        writeln!(writer, "{}", row.join(","))?;
    }

    Ok(())
}

/// Collect column names from all documents, preserving order from the first document.
fn collect_columns(documents: &[Value]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut columns = Vec::new();

    for doc in documents {
        if let Value::Object(map) = doc {
            for key in map.keys() {
                if seen.insert(key.clone()) {
                    columns.push(key.clone());
                }
            }
        }
    }

    columns
}

/// Format a JSON value for display in a table cell or CSV.
fn format_cell(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Array(arr)) => {
            if arr.len() <= 3 {
                serde_json::to_string(value.unwrap()).unwrap_or_default()
            } else {
                format!("[{} items]", arr.len())
            }
        }
        Some(Value::Object(obj)) => {
            if obj.len() <= 3 {
                serde_json::to_string(value.unwrap()).unwrap_or_default()
            } else {
                format!("{{{} fields}}", obj.len())
            }
        }
    }
}

/// Escape a value for CSV output.
fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_format_cell_types() {
        assert_eq!(format_cell(Some(&json!("hello"))), "hello");
        assert_eq!(format_cell(Some(&json!(42))), "42");
        assert_eq!(format_cell(Some(&json!(3.14))), "3.14");
        assert_eq!(format_cell(Some(&json!(true))), "true");
        assert_eq!(format_cell(Some(&Value::Null)), "");
        assert_eq!(format_cell(None), "");
    }

    #[test]
    fn test_format_cell_complex() {
        let small_arr = json!([1, 2]);
        assert!(format_cell(Some(&small_arr)).starts_with('['));

        let large_arr = json!([1, 2, 3, 4, 5]);
        assert_eq!(format_cell(Some(&large_arr)), "[5 items]");

        let small_obj = json!({"a": 1});
        assert!(format_cell(Some(&small_obj)).starts_with('{'));

        let large_obj = json!({"a": 1, "b": 2, "c": 3, "d": 4});
        assert_eq!(format_cell(Some(&large_obj)), "{4 fields}");
    }

    #[test]
    fn test_csv_escape() {
        assert_eq!(csv_escape("hello"), "hello");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_escape("line\nbreak"), "\"line\nbreak\"");
    }

    #[test]
    fn test_collect_columns() {
        let docs = vec![
            json!({"name": "Alice", "age": 30}),
            json!({"age": 25, "email": "bob@test.com", "name": "Bob"}),
        ];
        let cols = collect_columns(&docs);
        assert_eq!(cols, vec!["name", "age", "email"]);
    }

    #[test]
    fn test_write_json() {
        let docs = vec![json!({"id": "1"})];
        let mut buf = Vec::new();
        write_results(&mut buf, &docs, &OutputFormat::Json).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("\"id\": \"1\""));
    }

    #[test]
    fn test_write_json_compact() {
        let docs = vec![json!({"id": "1"}), json!({"id": "2"})];
        let mut buf = Vec::new();
        write_results(&mut buf, &docs, &OutputFormat::JsonCompact).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.trim().lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"id\":\"1\""));
    }

    #[test]
    fn test_write_csv() {
        let docs = vec![json!({"id": "1", "name": "Alice"})];
        let mut buf = Vec::new();
        write_results(&mut buf, &docs, &OutputFormat::Csv).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.trim().lines().collect();
        assert_eq!(lines[0], "id,name");
        assert_eq!(lines[1], "1,Alice");
    }

    #[test]
    fn test_write_table_empty() {
        let docs: Vec<Value> = vec![];
        let mut buf = Vec::new();
        write_results(&mut buf, &docs, &OutputFormat::Table).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("no results"));
    }

    #[test]
    fn test_write_table_with_data() {
        let docs = vec![json!({"id": "1", "name": "Alice"})];
        let mut buf = Vec::new();
        write_results(&mut buf, &docs, &OutputFormat::Table).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("id"));
        assert!(output.contains("name"));
        assert!(output.contains("Alice"));
    }

    #[test]
    fn test_render_template() {
        let docs = vec![
            json!({"id": "1", "name": "Alice"}),
            json!({"id": "2", "name": "Bob"}),
        ];
        let params = std::collections::BTreeMap::new();
        let template = "{% for doc in documents %}{{ doc.name }}\n{% endfor %}";
        let result = render_template(template, &docs, &params).unwrap();
        assert!(result.contains("Alice"));
        assert!(result.contains("Bob"));
    }

    #[test]
    fn test_render_template_with_params() {
        let docs = vec![json!({"total": 100})];
        let mut params = std::collections::BTreeMap::new();
        params.insert("status".to_string(), json!("shipped"));
        let template = "Status: {{ status }}\nTotal: {{ documents[0].total }}";
        let result = render_template(template, &docs, &params).unwrap();
        assert!(result.contains("Status: shipped"));
        assert!(result.contains("Total: 100"));
    }
}
