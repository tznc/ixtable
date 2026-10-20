//! Access to the kind-specific settings of a stored report component (`Component::extra`).

use crate::reports::Component;
use serde_json::{Map, Value};

/// A string setting (empty when absent).
pub fn text(c: &Component, key: &str) -> String {
    c.extra.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

pub fn flag(c: &Component, key: &str) -> bool {
    c.extra.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// Sets a setting; `Null` removes it.
pub fn set(c: &mut Component, key: &str, value: Value) {
    if value.is_null() {
        c.extra.remove(key);
    } else {
        c.extra.insert(key.to_string(), value);
    }
}

/// A setting that is a list of objects.
pub fn objects(c: &Component, key: &str) -> Vec<Map<String, Value>> {
    c.extra
        .get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_object().cloned()).collect())
        .unwrap_or_default()
}

pub fn set_objects(c: &mut Component, key: &str, list: Vec<Map<String, Value>>) {
    c.extra
        .insert(key.to_string(), Value::Array(list.into_iter().map(Value::Object).collect()));
}

/// A setting that is a list of strings.
pub fn strings(c: &Component, key: &str) -> Vec<String> {
    c.extra
        .get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

/// A number of the component's `style`.
pub fn style_num(c: &Component, key: &str) -> Option<f64> {
    c.extra.get("style")?.get(key)?.as_f64()
}

pub fn style_flag(c: &Component, key: &str) -> bool {
    c.extra
        .get("style")
        .and_then(|s| s.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn style_text(c: &Component, key: &str) -> Option<String> {
    c.extra.get("style")?.get(key)?.as_str().map(String::from)
}

/// Sets a `style` entry; `Null` removes it.
pub fn set_style(c: &mut Component, key: &str, value: Value) {
    let mut style = match c.extra.remove("style") {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    if value.is_null() {
        style.remove(key);
    } else {
        style.insert(key.to_string(), value);
    }
    c.extra.insert("style".to_string(), Value::Object(style));
}

pub fn str_of(map: &Map<String, Value>, key: &str) -> String {
    map.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// What a component shows on the canvas, after its kind label.
pub fn summary(c: &Component) -> String {
    match c.kind.as_str() {
        "staticText" => text(c, "text"),
        "field" | "calculated" => {
            let expression = text(c, "expression");
            if expression.is_empty() {
                "[no expression]".to_string()
            } else {
                format!("[{expression}]")
            }
        }
        "table" => format!("Table ({} columns)", objects(c, "columns").len()),
        "image" => {
            if text(c, "assetId").is_empty() {
                "Image (none)".to_string()
            } else {
                "Image".to_string()
            }
        }
        "subreport" => {
            if text(c, "reportId").is_empty() {
                "Subreport (none)".to_string()
            } else {
                "Subreport".to_string()
            }
        }
        "chart" => {
            let y: Vec<String> = strings(c, "yFields").into_iter().filter(|s| !s.is_empty()).collect();
            let kind = text(c, "chartType");
            if y.is_empty() {
                format!("{kind} chart")
            } else {
                format!("{kind} chart: {}", y.join(", "))
            }
        }
        _ => String::new(),
    }
}

/// The accessibility label of a component on the canvas: `Field [record.amount]`.
pub fn canvas_label(c: &Component) -> String {
    format!("{} {}", crate::report_engine::model::kind_label(&c.kind), summary(c))
        .trim()
        .to_string()
}

/// The field a bound expression names (`record.amount`, `record.[Unit price]`).
pub fn bound_field(expression: &str) -> Option<String> {
    let rest = expression.trim().strip_prefix("record.")?;
    let name = match rest.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        Some(inner) => inner,
        None if rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => rest,
        None => return None,
    };
    (!name.is_empty() && !name.contains(['[', ']'])).then(|| name.to_string())
}

/// What a component shows on the canvas: the field name, or "Expression" with a short
/// preview, instead of the raw expression.
pub fn canvas_text(c: &Component) -> String {
    if !matches!(c.kind.as_str(), "field" | "calculated") {
        return summary(c);
    }
    let expression = text(c, "expression");
    if expression.trim().is_empty() {
        return "No value yet".to_string();
    }
    if let Some(field) = bound_field(&expression) {
        return field;
    }
    let flat = expression.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = flat.chars().take(18).collect();
    if flat.chars().count() > 18 {
        format!("Expression: {short}…")
    } else {
        format!("Expression: {short}")
    }
}
