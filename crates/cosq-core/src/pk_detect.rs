//! Partition-key equality detection: when a query's WHERE clause pins the
//! container's partition key to a single value, cosq can scope the request to
//! one partition instead of fanning out to every partition key range.
//!
//! Deliberately conservative string analysis (no SQL parser dependency):
//! returns `None` whenever unsure, which simply keeps the fan-out behavior.

use serde_json::Value;

/// If `sql`'s WHERE clause pins the partition key at `pk_path` (e.g.
/// `/customerId` or `/address/zip`) to a single value, return that value.
///
/// Recognized forms (case-insensitive keywords):
/// - `WHERE c.customerId = "x"` / `= 42` / `= true`
/// - `WHERE c["customerId"] = ...`
/// - nested paths: `WHERE c.address.zip = ...` for pk `/address/zip`
/// - `= @param` resolved through `params`
/// - AND-composed conditions (`WHERE c.pk = 'x' AND c.other > 1`)
///
/// Bails to `None` on: no WHERE, a top-level `OR`, inequality operators on
/// the pk, `IN (...)`, functions wrapping the pk, or unresolvable params.
pub fn detect_pk_equality(sql: &str, pk_path: &str, params: &[(String, Value)]) -> Option<Value> {
    let where_clause = extract_where(sql)?;
    // A top-level OR could make the pk condition non-exclusive.
    if contains_top_level_or(&where_clause) {
        return None;
    }

    // Build the property accessors we accept for this pk path.
    let segments: Vec<&str> = pk_path.trim_start_matches('/').split('/').collect();
    if segments.is_empty() || segments.iter().any(|s| s.is_empty()) {
        return None;
    }
    let dotted = format!("c.{}", segments.join("."));
    let bracketed = format!(
        "c{}",
        segments
            .iter()
            .map(|s| format!("[\"{s}\"]"))
            .collect::<Vec<_>>()
            .join("")
    );

    for condition in where_clause.split_and() {
        if let Some(value) = match_equality(&condition, &dotted, &bracketed, params) {
            return Some(value);
        }
    }
    None
}

/// The text between WHERE and the next clause keyword (ORDER/GROUP/OFFSET) or
/// end of statement.
fn extract_where(sql: &str) -> Option<WhereClause> {
    let lower = sql.to_lowercase();
    let start = find_keyword(&lower, "where")? + "where".len();
    let rest = &sql[start..];
    let rest_lower = &lower[start..];
    let end = ["order by", "group by", "offset"]
        .iter()
        .filter_map(|kw| find_keyword(rest_lower, kw))
        .min()
        .unwrap_or(rest.len());
    Some(WhereClause(rest[..end].trim().to_string()))
}

/// Find a keyword surrounded by non-identifier characters (avoids matching
/// inside strings poorly — good enough for conservative detection: false
/// positives here only cause a `None` result or a failed strict match below).
fn find_keyword(haystack_lower: &str, keyword: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(pos) = haystack_lower[from..].find(keyword) {
        let abs = from + pos;
        let before_ok = abs == 0
            || !haystack_lower
                .as_bytes()
                .get(abs - 1)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'.');
        let after = abs + keyword.len();
        let after_ok = after >= haystack_lower.len()
            || !haystack_lower
                .as_bytes()
                .get(after)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_');
        if before_ok && after_ok {
            return Some(abs);
        }
        from = abs + keyword.len();
    }
    None
}

struct WhereClause(String);

impl WhereClause {
    /// Split on top-level ANDs (parenthesized groups kept intact).
    fn split_and(&self) -> Vec<String> {
        let mut parts = Vec::new();
        let mut depth = 0usize;
        let mut in_string: Option<char> = None;
        let mut current = String::new();
        let chars: Vec<char> = self.0.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let ch = chars[i];
            match in_string {
                Some(quote) => {
                    current.push(ch);
                    if ch == quote {
                        in_string = None;
                    }
                }
                None => match ch {
                    '\'' | '"' => {
                        in_string = Some(ch);
                        current.push(ch);
                    }
                    '(' => {
                        depth += 1;
                        current.push(ch);
                    }
                    ')' => {
                        depth = depth.saturating_sub(1);
                        current.push(ch);
                    }
                    'a' | 'A'
                        if depth == 0
                            && self.keyword_at(&chars, i, "and")
                            && !current.is_empty() =>
                    {
                        parts.push(current.trim().to_string());
                        current = String::new();
                        i += 2; // skip "nd"
                    }
                    _ => current.push(ch),
                },
            }
            i += 1;
        }
        if !current.trim().is_empty() {
            parts.push(current.trim().to_string());
        }
        parts
    }

    fn keyword_at(&self, chars: &[char], i: usize, kw: &str) -> bool {
        let end = i + kw.len();
        if end > chars.len() {
            return false;
        }
        let slice: String = chars[i..end].iter().collect();
        if !slice.eq_ignore_ascii_case(kw) {
            return false;
        }
        let before_ok = i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
        let after_ok = end == chars.len() || !(chars[end].is_alphanumeric() || chars[end] == '_');
        before_ok && after_ok
    }
}

fn contains_top_level_or(clause: &WhereClause) -> bool {
    let mut depth = 0usize;
    let mut in_string: Option<char> = None;
    let chars: Vec<char> = clause.0.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        match in_string {
            Some(quote) => {
                if ch == quote {
                    in_string = None;
                }
            }
            None => match ch {
                '\'' | '"' => in_string = Some(ch),
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                'o' | 'O' if depth == 0 && clause.keyword_at(&chars, i, "or") => {
                    return true;
                }
                _ => {}
            },
        }
        i += 1;
    }
    false
}

/// Match `<accessor> = <value>` (either operand order) and decode the value.
fn match_equality(
    condition: &str,
    dotted: &str,
    bracketed: &str,
    params: &[(String, Value)],
) -> Option<Value> {
    let (lhs, rhs) = condition.split_once('=')?;
    // reject >=, <=, != which also contain '='
    if lhs.ends_with(['>', '<', '!']) {
        return None;
    }
    let lhs = lhs.trim();
    let rhs = rhs.trim();

    let value_text = if lhs.eq_ignore_ascii_case(dotted) || lhs == bracketed {
        rhs
    } else if rhs.eq_ignore_ascii_case(dotted) || rhs == bracketed {
        lhs
    } else {
        return None;
    };

    decode_value(value_text, params)
}

fn decode_value(text: &str, params: &[(String, Value)]) -> Option<Value> {
    let text = text.trim();
    if let Some(param) = text.strip_prefix('@') {
        // whole token must be the param name
        if param.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return params
                .iter()
                .find(|(name, _)| name.trim_start_matches('@') == param)
                .map(|(_, v)| v.clone());
        }
        return None;
    }
    if (text.starts_with('\'') && text.ends_with('\'') && text.len() >= 2)
        || (text.starts_with('"') && text.ends_with('"') && text.len() >= 2)
    {
        return Some(Value::String(text[1..text.len() - 1].to_string()));
    }
    if text.eq_ignore_ascii_case("true") {
        return Some(Value::Bool(true));
    }
    if text.eq_ignore_ascii_case("false") {
        return Some(Value::Bool(false));
    }
    if let Ok(n) = text.parse::<i64>() {
        return Some(Value::Number(n.into()));
    }
    if let Ok(f) = text.parse::<f64>() {
        return serde_json::Number::from_f64(f).map(Value::Number);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn detect(sql: &str) -> Option<Value> {
        detect_pk_equality(sql, "/customerId", &[])
    }

    #[test]
    fn literal_string_and_number() {
        assert_eq!(
            detect("SELECT * FROM c WHERE c.customerId = 'abc'"),
            Some(json!("abc"))
        );
        assert_eq!(
            detect("SELECT * FROM c WHERE c.customerId = \"abc\""),
            Some(json!("abc"))
        );
        assert_eq!(
            detect("SELECT * FROM c WHERE c.customerId = 42"),
            Some(json!(42))
        );
        assert_eq!(
            detect("select * from c where C.CUSTOMERID = 'x' order by c.ts"),
            Some(json!("x"))
        );
    }

    #[test]
    fn reversed_operands_and_bracket_syntax() {
        assert_eq!(
            detect("SELECT * FROM c WHERE 'abc' = c.customerId"),
            Some(json!("abc"))
        );
        assert_eq!(
            detect("SELECT * FROM c WHERE c[\"customerId\"] = 'abc'"),
            Some(json!("abc"))
        );
    }

    #[test]
    fn parameter_resolution() {
        let params = vec![("@cid".to_string(), json!("cust-1"))];
        assert_eq!(
            detect_pk_equality(
                "SELECT * FROM c WHERE c.customerId = @cid",
                "/customerId",
                &params
            ),
            Some(json!("cust-1"))
        );
        // unresolvable param
        assert_eq!(
            detect_pk_equality(
                "SELECT * FROM c WHERE c.customerId = @nope",
                "/customerId",
                &[]
            ),
            None
        );
    }

    #[test]
    fn and_composed_conditions() {
        assert_eq!(
            detect("SELECT * FROM c WHERE c.active = true AND c.customerId = 'x' AND c.n > 3"),
            Some(json!("x"))
        );
    }

    #[test]
    fn bails_on_or_inequality_in_and_functions() {
        assert_eq!(
            detect("SELECT * FROM c WHERE c.customerId = 'x' OR c.other = 1"),
            None
        );
        assert_eq!(detect("SELECT * FROM c WHERE c.customerId != 'x'"), None);
        assert_eq!(detect("SELECT * FROM c WHERE c.customerId >= 'x'"), None);
        assert_eq!(
            detect("SELECT * FROM c WHERE c.customerId IN ('a', 'b')"),
            None
        );
        assert_eq!(
            detect("SELECT * FROM c WHERE UPPER(c.customerId) = 'X'"),
            None
        );
        assert_eq!(detect("SELECT * FROM c"), None);
    }

    #[test]
    fn or_inside_parens_is_fine() {
        assert_eq!(
            detect("SELECT * FROM c WHERE c.customerId = 'x' AND (c.a = 1 OR c.b = 2)"),
            Some(json!("x"))
        );
    }

    #[test]
    fn nested_pk_path() {
        assert_eq!(
            detect_pk_equality(
                "SELECT * FROM c WHERE c.address.zip = '12345'",
                "/address/zip",
                &[]
            ),
            Some(json!("12345"))
        );
    }

    #[test]
    fn different_field_is_not_pk() {
        assert_eq!(detect("SELECT * FROM c WHERE c.otherId = 'x'"), None);
        // suffix must not match (c.customerIdOld)
        assert_eq!(detect("SELECT * FROM c WHERE c.customerIdOld = 'x'"), None);
    }

    #[test]
    fn booleans_and_floats() {
        assert_eq!(
            detect_pk_equality("SELECT * FROM c WHERE c.flag = true", "/flag", &[]),
            Some(json!(true))
        );
        assert_eq!(
            detect_pk_equality("SELECT * FROM c WHERE c.score = 1.5", "/score", &[]),
            Some(json!(1.5))
        );
    }
}
