//! How a datasheet cell reads and writes text for a field (port of
//! `src/fields/datasheet.ts`).

use crate::forms::{
    field_values::field_text,
    mask::{apply_mask, masked_text, parse_mask},
};
use crate::data::{Column, DataValue};
use crate::recordstore::FieldSettings;
use crate::ui::schema::logical::{logical_of, value_from_text};

/// A datasheet cell's typed value: the input mask applied first, then the logical type.
pub fn datasheet_value(
    text: &str,
    column: &Column,
    field: Option<&FieldSettings>,
) -> Result<DataValue, String> {
    let mask = field
        .and_then(|f| f.input_mask.as_deref())
        .filter(|m| !m.is_empty());
    let masked = match mask {
        Some(mask) if text != "NULL" => masked_text(mask, text, &column.name)?,
        _ => text.to_string(),
    };
    value_from_text(&masked, &column.name, &logical_of(column))
}

fn plain(value: &DataValue) -> Option<String> {
    match value {
        DataValue::Null => None,
        DataValue::Integer(v) => Some(v.to_string()),
        DataValue::Real(v) => Some(v.to_string()),
        DataValue::Boolean(v) => Some(v.to_string()),
        DataValue::Blob(v)
        | DataValue::Text(v)
        | DataValue::Date(v)
        | DataValue::Timestamp(v)
        | DataValue::Decimal(v)
        | DataValue::Time(v) => Some(v.clone()),
    }
}

/// A cell's text for a formatted field (read-only; edited in a form) or a masked
/// one, else None.
pub fn datasheet_text(value: &DataValue, field: Option<&FieldSettings>) -> Option<String> {
    let field = field?;
    if let Some(format) = field.format.as_deref().filter(|f| !f.is_empty()) {
        return Some(field_text(plain(value).as_deref(), Some(format)));
    }
    let mask = field.input_mask.as_deref().filter(|m| !m.is_empty())?;
    if matches!(value, DataValue::Null) {
        return None;
    }
    Some(apply_mask(&parse_mask(mask), &plain(value).unwrap_or_default()).display)
}
