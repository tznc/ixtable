//! Row lookups for record steps: which rows an update or delete step matches, with the
//! identity and original values (`expected`) each write sends (formerly `rows.ts`).

use super::values::{from_data_value, row_to_object, to_data_value};
use crate::data::{Column, DataValue, Filter, FilterOperator, LogicalType, NamedValue};
use crate::trigger_auth::TriggerWrite;
use serde_json::{Map, Value};
use std::collections::HashSet;

/// Rows read per page while collecting a step's matches.
pub const MATCH_PAGE_SIZE: u64 = 1000;
/// Most rows one update or delete step may match. Above it the step fails before
/// writing anything, instead of silently handling only part of the matches.
pub const MAX_MATCHED_ROWS: u64 = 100_000;

#[derive(Debug, Clone)]
pub struct FoundRow {
    pub identity: Vec<DataValue>,
    pub object: Map<String, Value>,
    /// Original values for the optimistic check (stored, non-generated, non-blob columns).
    pub expected: Vec<NamedValue>,
    /// True when `expected` was just read from the database (not a snapshot).
    pub fresh: bool,
}

pub fn key_columns(columns: &[Column]) -> Vec<String> {
    let mut keys: Vec<&Column> = columns.iter().filter(|c| c.primary_key_position > 0).collect();
    keys.sort_by_key(|c| c.primary_key_position);
    keys.into_iter().map(|c| c.name.clone()).collect()
}

/// Columns whose original values can be compared at write time.
fn comparable(columns: &[Column]) -> HashSet<String> {
    columns
        .iter()
        .filter(|c| !c.generated && c.logical_type != LogicalType::Blob)
        .map(|c| c.name.clone())
        .collect()
}

fn inspect(
    label: &str,
    table: &str,
    trigger: Option<&TriggerWrite>,
) -> Result<crate::data::TableSchema, String> {
    crate::inspect_table(label.into(), table.into(), trigger.cloned()).map_err(|e| e.message)
}

/// Every row of `table` matching `criteria` (column → value; null matches NULL), read page
/// by page in identity order. Fails when none match or when more than `MAX_MATCHED_ROWS` do.
/// `trigger` authorizes the reads of an app-mode trigger step.
pub fn match_rows(
    label: &str,
    table: &str,
    criteria: &Map<String, Value>,
    trigger: Option<&TriggerWrite>,
) -> Result<Vec<FoundRow>, String> {
    let schema = inspect(label, table, trigger)?;
    let keep = comparable(&schema.columns);
    let filters = |_: ()| -> Vec<Filter> {
        criteria
            .iter()
            .map(|(column, value)| {
                if value.is_null() {
                    Filter {
                        column: column.clone(),
                        operator: FilterOperator::IsNull,
                        value: None,
                        values: None,
                    }
                } else {
                    Filter {
                        column: column.clone(),
                        operator: FilterOperator::Eq,
                        value: Some(to_data_value(value)),
                        values: None,
                    }
                }
            })
            .collect()
    };
    let mut found = Vec::new();
    let mut offset = 0;
    loop {
        let page = crate::read_table_page(
            label.into(),
            table.into(),
            offset,
            MATCH_PAGE_SIZE,
            vec![],
            filters(()),
            trigger.cloned(),
        )
        .map_err(|e| e.message)?;
        if page.total > MAX_MATCHED_ROWS {
            return Err(format!(
                "{} {table} rows match; one step can change at most {MAX_MATCHED_ROWS}. Narrow the match.",
                page.total
            ));
        }
        let names: Vec<String> = page.columns.iter().map(|c| c.name.clone()).collect();
        for (i, row) in page.rows.iter().enumerate() {
            found.push(FoundRow {
                identity: page.identities.get(i).cloned().unwrap_or_default(),
                object: row_to_object(&names, row),
                expected: names
                    .iter()
                    .zip(row)
                    .filter(|(name, _)| keep.contains(*name))
                    .map(|(name, value)| NamedValue {
                        column: name.clone(),
                        value: value.clone(),
                    })
                    .collect(),
                fresh: true,
            });
        }
        if (page.rows.len() as u64) < MATCH_PAGE_SIZE {
            break;
        }
        offset += MATCH_PAGE_SIZE;
    }
    if found.is_empty() {
        return Err(format!("No {table} rows match"));
    }
    Ok(found)
}

/// The current record as a row to write: by its key columns, else by its rowid. Without a
/// key the row is not re-read, so `expected` comes from `snapshot` (the record as loaded,
/// before unsaved edits) when given, else from `record`.
pub fn current_row(
    label: &str,
    table: &str,
    record: Option<&Map<String, Value>>,
    snapshot: Option<&Map<String, Value>>,
    trigger: Option<&TriggerWrite>,
) -> Result<Vec<FoundRow>, String> {
    let record = record.ok_or("There is no current record")?;
    let schema = inspect(label, table, trigger)?;
    let keys = key_columns(&schema.columns);
    if !keys.is_empty() {
        let mut criteria = Map::new();
        for key in keys {
            match record.get(&key) {
                None | Some(Value::Null) => {
                    return Err(format!("The current record has no value for key column {key}"))
                }
                Some(value) => {
                    criteria.insert(key, value.clone());
                }
            }
        }
        return match_rows(label, table, &criteria, trigger);
    }
    let rowid = match record.get("rowid") {
        None | Some(Value::Null) => {
            return Err(format!(
                "Table {table} has no primary key and the record has no rowid"
            ))
        }
        Some(rowid) => rowid,
    };
    let keep = comparable(&schema.columns);
    Ok(vec![FoundRow {
        identity: vec![to_data_value(rowid)],
        object: record.clone(),
        expected: snapshot
            .unwrap_or(record)
            .iter()
            .filter(|(column, _)| keep.contains(*column))
            .map(|(column, value)| NamedValue {
                column: column.clone(),
                value: to_data_value(value),
            })
            .collect(),
        fresh: false,
    }])
}

/// `values` plus the generated key of a created row (rowid when there is no key).
pub fn with_keys(
    label: &str,
    table: &str,
    values: &Map<String, Value>,
    identity: &[DataValue],
    trigger: Option<&TriggerWrite>,
) -> Map<String, Value> {
    let mut out = values.clone();
    let Ok(schema) = inspect(label, table, trigger) else {
        return out;
    };
    let keys = key_columns(&schema.columns);
    let names = if keys.is_empty() {
        vec!["rowid".to_string()]
    } else {
        keys
    };
    for (i, name) in names.into_iter().enumerate() {
        out.insert(name, identity.get(i).map_or(Value::Null, from_data_value));
    }
    out
}

/// Comparable original values of `object` (a row as read or loaded) for `table`.
pub fn expected_of(
    label: &str,
    table: &str,
    object: &Map<String, Value>,
) -> Result<Vec<NamedValue>, String> {
    let keep = comparable(&inspect(label, table, None)?.columns);
    Ok(object
        .iter()
        .filter(|(column, _)| keep.contains(*column))
        .map(|(column, value)| NamedValue {
            column: column.clone(),
            value: to_data_value(value),
        })
        .collect())
}

/// Key of a row within one action run: table plus identity.
pub fn row_key(table: &str, identity: &[DataValue]) -> String {
    format!(
        "{table}\u{0}{}",
        serde_json::to_string(identity).unwrap_or_default()
    )
}

/// `expected` after writing `values` over it: the row's values once the write applies.
pub fn after_write(expected: &[NamedValue], values: &[NamedValue]) -> Vec<NamedValue> {
    expected
        .iter()
        .map(|v| {
            values
                .iter()
                .find(|w| w.column == v.column)
                .map_or_else(|| v.clone(), |w| NamedValue {
                    column: v.column.clone(),
                    value: w.value.clone(),
                })
        })
        .collect()
}

/// One row of `table` by identity (primary key values, or rowid); None when it can't be read.
pub fn read_row(label: &str, table: &str, identity: &[DataValue]) -> Option<Map<String, Value>> {
    let schema = inspect(label, table, None).ok()?;
    let keys = key_columns(&schema.columns);
    if keys.is_empty() || keys.len() != identity.len() {
        return None;
    }
    let filters = keys
        .iter()
        .zip(identity)
        .map(|(column, value)| Filter {
            column: column.clone(),
            operator: FilterOperator::Eq,
            value: Some(value.clone()),
            values: None,
        })
        .collect();
    let page = crate::read_table_page(
        label.into(),
        table.into(),
        0,
        1,
        vec![],
        filters,
        None,
    )
    .ok()?;
    let names: Vec<String> = page.columns.iter().map(|c| c.name.clone()).collect();
    page.rows.first().map(|row| row_to_object(&names, row))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn after_write_overlays_written_columns_only() {
        let nv = |c: &str, v: DataValue| NamedValue {
            column: c.into(),
            value: v,
        };
        let out = after_write(
            &[
                nv("id", DataValue::Integer(1)),
                nv("status", DataValue::Text("open".into())),
            ],
            &[nv("status", DataValue::Text("x".into()))],
        );
        assert_eq!(
            out,
            vec![
                nv("id", DataValue::Integer(1)),
                nv("status", DataValue::Text("x".into()))
            ]
        );
    }
}
