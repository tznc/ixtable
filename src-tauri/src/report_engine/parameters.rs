//! Report parameters: which a report needs and how prompt values are parsed.

use crate::archive::{QueryParameter, SavedQuery};
use crate::reports::Report;
use serde_json::Value;

/// Parameters a report needs: those its dataset query declares, then those of its tables'
/// queries (first declaration wins), with the report's own defaults ahead of the query
/// defaults.
pub fn report_parameters(report: &Report, queries: &[SavedQuery]) -> Vec<QueryParameter> {
    let mut ids: Vec<String> = vec![];
    if let Some(id) = &report.dataset_query_id {
        ids.push(id.clone());
    }
    for entry in super::model::band_entries(report) {
        for c in &entry.band.components {
            if c.kind != "table" {
                continue;
            }
            if let Some(q) = c.extra.get("queryId").and_then(Value::as_str).filter(|q| !q.is_empty()) {
                ids.push(q.to_string());
            }
        }
    }
    let mut seen: Vec<QueryParameter> = vec![];
    for id in ids {
        let Some(query) = queries.iter().find(|q| q.id == id) else {
            continue;
        };
        for p in &query.parameters {
            if seen.iter().any(|s| s.name == p.name) {
                continue;
            }
            let own = report.params.get(&p.name).filter(|v| !v.is_null()).cloned();
            seen.push(QueryParameter {
                default_value: own.or_else(|| p.default_value.clone()),
                ..p.clone()
            });
        }
    }
    seen
}

/// The kind of input a prompt shows for a logical type.
pub fn input_type(logical_type: &str) -> &'static str {
    match logical_type {
        "integer" | "number" => "number",
        "date" => "date",
        "timestamp" => "datetime-local",
        _ => "text",
    }
}

/// A parameter value as prompt text (null and booleans show empty).
pub fn display(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(_)) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e21 => format!("{}", f as i64),
            Some(f) => format!("{f}"),
            None => n.to_string(),
        },
        Some(other) => other.to_string(),
    }
}

/// One prompt field's raw entry.
#[derive(Debug, Clone, PartialEq)]
pub enum Raw {
    Text(String),
    Flag(bool),
}

/// Typed value of one prompt field, or an error message.
pub fn parse_parameter(p: &QueryParameter, raw: &Raw) -> Result<Value, String> {
    let text = match raw {
        Raw::Flag(b) => return Ok(Value::Bool(*b)),
        Raw::Text(t) => t.trim(),
    };
    if text.is_empty() {
        return if p.required {
            Err(format!("Enter a value for {}.", p.name))
        } else {
            Ok(Value::Null)
        };
    }
    if p.logical_type == "integer" || p.logical_type == "number" {
        let integer = p.logical_type == "integer";
        let n: Option<f64> = text.parse::<f64>().ok().filter(|n| n.is_finite());
        return match n {
            Some(n) if !integer || n.fract() == 0.0 => Ok(super::values::num_value(n)),
            _ => Err(format!(
                "{} must be {} number.",
                p.name,
                if integer { "a whole" } else { "a" }
            )),
        };
    }
    Ok(Value::String(text.to_string()))
}
