//! Field helpers forms need: the `Value` flavour of the field text helpers
//! (`mask`, `richtext` and `field_values` hold the logic on plain text), per-column field
//! settings and the form defaults they imply.

use super::field_values::{self as text_values, AttachmentRef};
use super::richtext;
use crate::design::{Control, ControlKind, Form, SelectOption};
use crate::recordstore::{EntitySettings, FieldSettings};
use serde_json::Value;

/// JavaScript's `String(value)` for a JSON value ("" for null).
pub fn value_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => number_text(n),
        other => other.to_string(),
    }
}

fn number_text(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    match n.as_f64() {
        Some(f) if f.fract() == 0.0 && f.abs() < 1e21 => format!("{}", f as i128),
        Some(f) => format!("{f}"),
        None => n.to_string(),
    }
}

/// The field settings of `table.column`, if any.
pub fn field_settings_for<'a>(
    entities: &'a [EntitySettings],
    table: Option<&str>,
    column: Option<&str>,
) -> Option<&'a FieldSettings> {
    let (table, column) = (table?, column?);
    entities
        .iter()
        .find(|e| e.table == table)?
        .fields
        .iter()
        .find(|f| f.column == column)
}

/// The files of an attachment value; anything but a JSON array of files is none.
pub fn parse_attachments(value: &Value) -> Vec<AttachmentRef> {
    text_values::parse_attachments(&value_text(value))
}

/// The stored value for a list of files: a JSON array, or null when empty.
pub fn attachments_value(files: &[AttachmentRef]) -> Value {
    text_values::attachments_value(files).map_or(Value::Null, Value::String)
}

/// The choices of a multi-select value; a plain string is one choice.
pub fn parse_choices(value: &Value) -> Vec<String> {
    text_values::parse_choices(&value_text(value))
}

/// The stored value for chosen items: a JSON array, or null when none.
pub fn choices_value(items: &[String]) -> Value {
    text_values::choices_value(items).map_or(Value::Null, Value::String)
}

/// Plain text of a rich-text (sanitized HTML) value, for lists, datasheets and validation.
pub fn rich_text_plain(html: &Value) -> String {
    richtext::rich_text_plain(Some(&value_text(html)))
}

/// Text of a formatted field's value in a list cell, datasheet or report.
pub fn field_text(value: &Value, format: Option<&str>) -> String {
    text_values::field_text(Some(&value_text(value)), format)
}

/// The form with field settings filled into its controls: a plain text control bound to a
/// rich-text, attachment or multi-select field becomes that kind, a text control without
/// its own input mask uses its column's, and a multi-select without choices uses the
/// field's. Returns None when nothing changes.
pub fn with_field_defaults(form: &Form, entities: &[EntitySettings]) -> Option<Form> {
    let table = crate::forms::schema::form_table(form)?;
    let entity = entities
        .iter()
        .find(|e| e.table == table && !e.fields.is_empty())?;
    let mut changed = false;
    let controls: Vec<Control> = form
        .controls
        .iter()
        .map(|control| {
            let bound = control.binding.as_ref();
            let table_name = bound
                .and_then(|b| b.table.as_deref())
                .filter(|t| !t.is_empty())
                .unwrap_or(table);
            let field = if entity.table == table_name {
                bound.and_then(|b| entity.fields.iter().find(|f| f.column == b.column))
            } else {
                field_settings_for(entities, Some(table_name), bound.map(|b| b.column.as_str()))
            };
            let Some(field) = field else {
                return control.clone();
            };
            let format = field.format.as_deref().unwrap_or("");
            let options = |values: &[String]| -> Vec<SelectOption> {
                values
                    .iter()
                    .map(|v| SelectOption {
                        value: v.clone(),
                        label: v.clone(),
                    })
                    .collect()
            };
            let formatted = match format {
                "richText" => Some(ControlKind::RichText),
                "attachment" => Some(ControlKind::Attachment),
                "multiSelect" => Some(ControlKind::MultiSelect),
                _ => None,
            };
            if let (Some(kind), true) = (
                formatted,
                matches!(control.kind, ControlKind::Text | ControlKind::Multiline),
            ) {
                changed = true;
                let mut next = control.clone();
                next.kind = kind;
                if kind == ControlKind::MultiSelect {
                    next.options = options(&field.options);
                }
                return next;
            }
            if control.kind == ControlKind::Text
                && control.input_mask.as_deref().unwrap_or("").is_empty()
                && field.input_mask.as_deref().is_some_and(|m| !m.is_empty())
            {
                changed = true;
                let mut next = control.clone();
                next.input_mask = field.input_mask.clone();
                return next;
            }
            if control.kind == ControlKind::MultiSelect
                && control.options.is_empty()
                && !field.options.is_empty()
            {
                changed = true;
                let mut next = control.clone();
                next.options = options(&field.options);
                return next;
            }
            control.clone()
        })
        .collect();
    changed.then(|| Form {
        controls,
        ..form.clone()
    })
}

#[cfg(test)]
#[path = "fields_tests.rs"]
mod tests;
