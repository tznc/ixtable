//! Row values as the datasheet reads and writes them (port of `src/data/sheet/rows.ts`).

use crate::data::{Column, DataValue, NamedValue};
use crate::recordstore::FieldSettings;
use crate::ui::fields::datasheet::datasheet_value;
use crate::ui::schema::logical::logical_of;
use std::collections::BTreeMap;

/// Field settings per column index of a page (masks and formats).
pub type SheetFields = Vec<Option<FieldSettings>>;

/// The row as read, sent back as `expected` so optimistic entities detect concurrent
/// edits. Generated and blob columns are left out.
pub fn original_values(columns: &[Column], row: &[DataValue]) -> Vec<NamedValue> {
    columns
        .iter()
        .zip(row)
        .filter(|(c, _)| !c.generated && logical_of(c) != "blob")
        .map(|(c, v)| NamedValue {
            column: c.name.clone(),
            value: v.clone(),
        })
        .collect()
}

/// Whether the datasheet lets the user type into `column`. Formatted fields (rich
/// text, attachments, multi-select) are edited in a form.
pub fn is_editable(column: &Column, field: Option<&FieldSettings>) -> bool {
    !column.generated
        && logical_of(column) != "blob"
        && field.is_none_or(|f| f.format.as_deref().is_none_or(str::is_empty))
}

/// Typed values for `cells` (column index to text). Empty text becomes null when
/// `empty_as_null` (pasting over a cell), and is left out otherwise (a new record
/// keeps the column default). A column's input mask applies before its type.
pub fn typed_values(
    columns: &[Column],
    cells: &BTreeMap<usize, String>,
    empty_as_null: bool,
    fields: &SheetFields,
) -> Result<Vec<NamedValue>, String> {
    let mut out = Vec::new();
    for (&j, text) in cells {
        let column = &columns[j];
        if text.is_empty() {
            if empty_as_null {
                out.push(NamedValue {
                    column: column.name.clone(),
                    value: DataValue::Null,
                });
            }
            continue;
        }
        let field = fields.get(j).and_then(Option::as_ref);
        out.push(NamedValue {
            column: column.name.clone(),
            value: datasheet_value(text, column, field)?,
        });
    }
    Ok(out)
}
