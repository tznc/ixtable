//! SQL helpers for the editor. Port of src/query/sql.ts and the value conversion in
//! src/query/api.ts.

use super::{compile::try_compile, model::normalize_model};
use crate::{
    archive::SavedQuery,
    data::{DataValue, NamedValue},
};
use serde_json::Value;
use std::collections::BTreeMap;

/// `$name` placeholders in SQL, in first-use order, skipping strings, quoted
/// identifiers, comments, and dollar-quoted strings. Mirrors `rewrite_placeholders`
/// in queries (which is authoritative); used for live parameter hints.
pub fn placeholder_names(sql: &str) -> Vec<String> {
    let chars: Vec<char> = sql.chars().collect();
    let at = |i: usize| chars.get(i).copied();
    let mut names: Vec<String> = vec![];
    let mut i = 0;
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    while i < chars.len() {
        let c = chars[i];
        let next = at(i + 1);
        if c == '\'' || c == '"' {
            i += 1;
            while i < chars.len() {
                if chars[i] == c {
                    if at(i + 1) == Some(c) {
                        i += 2;
                    } else {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            i += 1;
        } else if c == '-' && next == Some('-') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let end = (i + 2..chars.len().saturating_sub(1)).find(|&k| chars[k] == '*' && chars[k + 1] == '/');
            i = end.map_or(chars.len(), |e| e + 2);
        } else if c == '$' {
            let mut j = i + 1;
            while j < chars.len() && ident(chars[j]) {
                j += 1;
            }
            let word: String = chars[i + 1..j].iter().collect();
            if at(j) == Some('$') {
                let tag: Vec<char> = chars[i..=j].to_vec();
                let end = (j + 1..chars.len()).find(|&k| chars[k..].starts_with(&tag));
                i = end.map_or(chars.len(), |e| e + tag.len());
            } else {
                let starts = word
                    .chars()
                    .next()
                    .is_some_and(|f| f.is_ascii_alphabetic() || f == '_');
                if starts && !names.contains(&word) {
                    names.push(word);
                }
                i = j.max(i + 1);
            }
        } else {
            i += 1;
        }
    }
    names
}

/// The SQL a query runs, compiled from the builder model when it has one:
/// `(sql, error)`.
pub fn effective_sql(query: &SavedQuery) -> (String, String) {
    match &query.builder {
        Some(builder) if !builder.is_null() => try_compile(&normalize_model(builder)),
        _ => (query.sql.clone(), String::new()),
    }
}

/// Form values -> run params; blank values are omitted so defaults apply.
pub fn run_params(values: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    values
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Converts a plain JSON value (or a tagged `DataValue`) to the value sent to the backend.
pub fn to_data_value(value: &Value) -> DataValue {
    match value {
        Value::Null => DataValue::Null,
        Value::Bool(b) => DataValue::Boolean(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => DataValue::Integer(i),
            None => {
                let f = n.as_f64().unwrap_or(0.0);
                if f.fract() == 0.0 {
                    DataValue::Integer(f as i64)
                } else {
                    DataValue::Real(f)
                }
            }
        },
        Value::Object(o) if o.contains_key("type") => {
            serde_json::from_value(value.clone()).unwrap_or_else(|_| DataValue::Text(value.to_string()))
        }
        Value::String(s) => DataValue::Text(s.clone()),
        other => DataValue::Text(other.to_string()),
    }
}

/// Run parameters (text from the "Run with" fields) as named backend values.
pub fn to_named_values(params: &BTreeMap<String, String>) -> Vec<NamedValue> {
    params
        .iter()
        .map(|(column, value)| NamedValue {
            column: column.clone(),
            value: DataValue::Text(value.clone()),
        })
        .collect()
}
