//! Field settings lookups and the stored shapes of formatted fields (port of
//! `src/fields/values.ts` and `types.ts`).

use super::richtext::rich_text_plain;
use crate::archive::DocumentConfig;
use crate::recordstore::FieldSettings;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Field formats layered on a column's logical type (`docs/decisions/field-formats.md`):
/// (value, label, logical types it applies to).
pub const FIELD_FORMATS: &[(&str, &str, &[&str])] = &[
    ("richText", "Rich text", &["text"]),
    ("attachment", "Attachments", &["text", "json"]),
    ("multiSelect", "Multiple choices", &["text", "json"]),
];

/// Largest file one attachment can hold (decimal megabytes), as the backend enforces it.
pub const MAX_ATTACHMENT_BYTES: u64 = 20_000_000;

/// One file of an attachment field, as stored in the field's JSON array.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AttachmentRef {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub sha256: String,
}

fn json_array(value: &str) -> Vec<Value> {
    if !value.trim().starts_with('[') {
        return vec![];
    }
    match serde_json::from_str::<Value>(value) {
        Ok(Value::Array(items)) => items,
        _ => vec![],
    }
}

/// The files of an attachment value; anything but a JSON array of files is none.
pub fn parse_attachments(value: &str) -> Vec<AttachmentRef> {
    json_array(value)
        .into_iter()
        .filter(|f| f.get("id").is_some_and(Value::is_string))
        .map(|f| AttachmentRef {
            id: f["id"].as_str().unwrap_or_default().to_string(),
            name: f["name"].as_str().unwrap_or_default().to_string(),
            mime: f["mime"].as_str().unwrap_or_default().to_string(),
            size: f["size"].as_u64().unwrap_or(0),
            sha256: f["sha256"].as_str().unwrap_or_default().to_string(),
        })
        .collect()
}

/// The stored value for a list of files: a JSON array, or None when empty.
pub fn attachments_value(files: &[AttachmentRef]) -> Option<String> {
    (!files.is_empty()).then(|| serde_json::to_string(files).unwrap_or_default())
}

/// The choices of a multi-select value; a plain string is one choice.
pub fn parse_choices(value: &str) -> Vec<String> {
    if value.is_empty() {
        return vec![];
    }
    let items = json_array(value);
    if !items.is_empty() || value.trim().starts_with('[') {
        return items
            .into_iter()
            .map(|v| match v {
                Value::String(s) => s,
                other => other.to_string(),
            })
            .collect();
    }
    vec![value.to_string()]
}

/// The stored value for chosen items: a JSON array, or None when none.
pub fn choices_value(items: &[String]) -> Option<String> {
    (!items.is_empty()).then(|| serde_json::to_string(items).unwrap_or_default())
}

/// Readable size, e.g. `12 KB` (decimal units, as the size limits are written).
pub fn file_size(bytes: u64) -> String {
    if bytes < 1000 {
        format!("{bytes} B")
    } else if bytes < 1_000_000 {
        format!("{} KB", (bytes as f64 / 1000.0).round() as u64)
    } else {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    }
}

/// Text of a formatted field's value in a list cell, datasheet, or report.
pub fn field_text(value: Option<&str>, format: Option<&str>) -> String {
    match format {
        Some("richText") => rich_text_plain(value).replace('\n', " "),
        Some("attachment") => {
            let files = parse_attachments(value.unwrap_or(""));
            match files.len() {
                0 => String::new(),
                1 => files[0].name.clone(),
                n => format!("{n} files"),
            }
        }
        Some("multiSelect") => parse_choices(value.unwrap_or("")).join(", "),
        _ => value.unwrap_or("").to_string(),
    }
}

/// Rows of a table with its formatted fields as text (rich text as plain text, files
/// by name, choices joined), the way a report shows them. Rows without formatted
/// fields come back unchanged.
pub fn field_text_rows(
    rows: Vec<serde_json::Map<String, Value>>,
    config: &DocumentConfig,
    table: Option<&str>,
) -> Vec<serde_json::Map<String, Value>> {
    let formatted: Vec<&FieldSettings> = config
        .entities
        .iter()
        .find(|e| Some(e.table.as_str()) == table)
        .map(|e| e.fields.iter().filter(|f| f.format.is_some()).collect())
        .unwrap_or_default();
    if formatted.is_empty() {
        return rows;
    }
    rows.into_iter()
        .map(|mut row| {
            for field in &formatted {
                if let Some(value) = row.get(&field.column) {
                    let text = match value {
                        Value::Null => None,
                        Value::String(s) => Some(s.clone()),
                        other => Some(other.to_string()),
                    };
                    let shown = field_text(text.as_deref(), field.format.as_deref());
                    row.insert(field.column.clone(), Value::String(shown));
                }
            }
            row
        })
        .collect()
}
