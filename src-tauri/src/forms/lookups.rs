//! Relationship lookups for list columns (port of the non-hook half of
//! `src/runtime/lookups.ts`): which columns show a related record's display value, and the
//! display labels for the keys of a set of rows.

use super::data::{PageRequest, label_cached, label_store, table_page, table_schema};
use super::fields::value_text;
use super::generate::kind_for_column;
use super::values::{RecordValues, from_data_value, to_data_value};
use crate::data::{DataValue, Filter, FilterOperator, NamedValue, TableSchema};
use crate::design::{ControlKind, KeyPair, Relationship};
use crate::manager::AppError;
use serde_json::Value;
use std::collections::HashMap;

/// Columns with the relationship whose display value is shown instead of the raw key, in
/// column order.
pub type Lookups = Vec<(String, Relationship)>;
/// Column to (key text, from `lookup_key`, to display text).
pub type LookupLabels = HashMap<String, HashMap<String, String>>;

const DISPLAY_NAMES: [&str; 3] = ["name", "title", "label"];

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn cache_key(lookup: &Relationship, key: &str) -> String {
    serde_json::to_string(&(
        &lookup.table,
        &lookup.value_column,
        &lookup.display_column,
        lookup.keys.iter().map(|k| (&k.column, &k.target)).collect::<Vec<_>>(),
        key,
    ))
    .unwrap_or_default()
}

/// Key pairs of a multi-column relationship, else None.
fn composite_keys(lookup: &Relationship) -> Option<&[KeyPair]> {
    (lookup.keys.len() > 1).then_some(lookup.keys.as_slice())
}

/// The lookup key of `row` for `column`: the cell text, or for a multi-column relationship
/// every key column's text as JSON. None when any key value is empty.
pub fn lookup_key(lookup: &Relationship, column: &str, row: &RecordValues) -> Option<String> {
    let columns: Vec<&str> = match composite_keys(lookup) {
        Some(pairs) => pairs.iter().map(|p| p.column.as_str()).collect(),
        None => vec![column],
    };
    let values: Vec<&Value> = columns.iter().map(|c| row.get(*c).unwrap_or(&Value::Null)).collect();
    if values
        .iter()
        .any(|v| v.is_null() || v.as_str() == Some(""))
    {
        return None;
    }
    let texts: Vec<String> = values.iter().map(|v| value_text(v)).collect();
    Some(if texts.len() > 1 {
        serde_json::to_string(&texts).unwrap_or_default()
    } else {
        texts[0].clone()
    })
}

/// Conventional display column: the first text column named name, title, or label.
pub fn guess_display_column(target: &TableSchema) -> Option<String> {
    target
        .columns
        .iter()
        .find(|c| {
            DISPLAY_NAMES.contains(&c.name.to_lowercase().as_str())
                && kind_for_column(c) == Some(ControlKind::Text)
        })
        .map(|c| c.name.clone())
}

/// Lookups for list columns: a relationship control's display column wins; otherwise a
/// single-column foreign key of `table` with a conventional display column on its target.
/// `relationship_of(column)` is the relationship of the column's relationship control.
pub fn column_lookups(
    label: &str,
    table: Option<&str>,
    columns: &[String],
    relationship_of: impl Fn(&str) -> Option<Relationship>,
) -> Lookups {
    let schema = table.and_then(|t| table_schema(label, t).ok());
    columns
        .iter()
        .filter_map(|column| {
            if let Some(rel) = relationship_of(column) {
                if !rel.table.is_empty() && !rel.value_column.is_empty() && !rel.display_column.is_empty() {
                    return (rel.display_column != rel.value_column).then(|| (column.clone(), rel));
                }
            }
            let fk = schema.as_ref()?.foreign_keys.iter().find(|key| {
                key.from_columns.len() == 1 && key.from_columns[0] == *column
            })?;
            let target = table_schema(label, &fk.target_table).ok()?;
            let display = guess_display_column(&target)?;
            let value_column = fk.target_columns.first().cloned().or_else(|| {
                target
                    .columns
                    .iter()
                    .find(|c| c.primary_key_position == 1)
                    .map(|c| c.name.clone())
            })?;
            Some((
                column.clone(),
                Relationship {
                    table: fk.target_table.clone(),
                    value_column,
                    display_column: display,
                    ..Default::default()
                },
            ))
        })
        .collect()
}

fn cell<'a>(row: &'a RecordValues, column: &str) -> &'a Value {
    row.get(column).unwrap_or(&Value::Null)
}

/// The rows `label_query` returns (key columns, then display), read with one `in` filtered
/// table page for a runtime role (the backend refuses it ad hoc SQL but authorizes lookup
/// table reads). Composite keys filter on their first column, then match the rest here.
fn labels_by_key(
    label: &str,
    lookup: &Relationship,
    column: &str,
    rows: &[RecordValues],
) -> Result<Vec<Vec<DataValue>>, AppError> {
    let pairs: Vec<KeyPair> = match composite_keys(lookup) {
        Some(pairs) => pairs.to_vec(),
        None => vec![KeyPair {
            column: column.into(),
            target: lookup.value_column.clone(),
        }],
    };
    let page = table_page(
        label,
        &lookup.table,
        &PageRequest {
            limit: if pairs.len() > 1 { 1000 } else { rows.len() as u64 },
            filters: vec![Filter {
                column: pairs[0].target.clone(),
                operator: FilterOperator::In,
                value: None,
                values: Some(rows.iter().map(|r| to_data_value(cell(r, &pairs[0].column))).collect()),
            }],
            ..Default::default()
        },
    )?;
    let text_of = |row: &RecordValues, name: &str| value_text(cell(row, name));
    let wanted: Vec<Vec<String>> = rows
        .iter()
        .map(|row| pairs.iter().map(|p| value_text(cell(row, &p.column))).collect())
        .collect();
    Ok(page
        .rows
        .into_iter()
        .filter(|row| {
            let key: Vec<String> = pairs.iter().map(|p| text_of(row, &p.target)).collect();
            wanted.contains(&key)
        })
        .map(|row| {
            let mut out: Vec<DataValue> = pairs
                .iter()
                .map(|p| to_data_value(cell(&row, &p.target)))
                .collect();
            out.push(to_data_value(cell(&row, &lookup.display_column)));
            out
        })
        .collect())
}

/// Bound query reading display text for the keys of `rows` (one row per distinct key).
pub fn label_query(
    lookup: &Relationship,
    column: &str,
    rows: &[RecordValues],
) -> (String, Vec<NamedValue>) {
    let display = format!("{} AS lookup_display", quote(&lookup.display_column));
    let param = |name: String, value: &Value| NamedValue {
        column: name,
        value: to_data_value(value),
    };
    let Some(pairs) = composite_keys(lookup) else {
        let params: Vec<NamedValue> = rows
            .iter()
            .enumerate()
            .map(|(i, row)| param(format!("k{i}"), cell(row, column)))
            .collect();
        let placeholders: Vec<String> = params.iter().map(|p| format!("${}", p.column)).collect();
        return (
            format!(
                "SELECT {} AS lookup_key, {display} FROM {} WHERE {} IN ({})",
                quote(&lookup.value_column),
                quote(&lookup.table),
                quote(&lookup.value_column),
                placeholders.join(", ")
            ),
            params,
        );
    };
    let mut params = vec![];
    let matches: Vec<String> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let parts: Vec<String> = pairs
                .iter()
                .enumerate()
                .map(|(j, pair)| {
                    params.push(param(format!("k{i}_{j}"), cell(row, &pair.column)));
                    format!("{} = $k{i}_{j}", quote(&pair.target))
                })
                .collect();
            format!("({})", parts.join(" AND "))
        })
        .collect();
    let keys: Vec<String> = pairs
        .iter()
        .enumerate()
        .map(|(j, pair)| format!("{} AS lookup_key{j}", quote(&pair.target)))
        .collect();
    (
        format!(
            "SELECT {}, {display} FROM {} WHERE {}",
            keys.join(", "),
            quote(&lookup.table),
            matches.join(" OR ")
        ),
        params,
    )
}

/// Reads the label rows; a role that may not run SQL reads them through a table page. None
/// when both fail (the keys stay unlabelled and uncached).
fn read_labels(
    label: &str,
    lookup: &Relationship,
    column: &str,
    rows: &[RecordValues],
) -> Option<Vec<Vec<DataValue>>> {
    let (sql, params) = label_query(lookup, column, rows);
    match tauri::async_runtime::block_on(crate::queries::execute_parameterized_query(
        label.to_string(),
        sql,
        params,
        None,
        None,
        None,
    )) {
        Ok(run) => Some(run.rows),
        Err(error) if error.code == "FORBIDDEN" => labels_by_key(label, lookup, column, rows).ok(),
        Err(_) => None,
    }
}

/// Display labels for the keys in `rows`: one bound `IN (...)` query per lookup, through DuckDB.
pub fn lookup_labels(label: &str, lookups: &Lookups, rows: &[RecordValues]) -> LookupLabels {
    let mut result = LookupLabels::new();
    for (column, lookup) in lookups {
        let mut labels: HashMap<String, String> = HashMap::new();
        // Distinct missing keys in first-seen order; a later row with the same key replaces it.
        let mut missing: Vec<(String, RecordValues)> = vec![];
        let pairs = composite_keys(lookup);
        for row in rows {
            let Some(key) = lookup_key(lookup, column, row) else {
                continue;
            };
            match label_cached(label, &cache_key(lookup, &key)) {
                Some(Some(hit)) => {
                    labels.insert(key, hit);
                }
                Some(None) => {}
                None => match missing.iter_mut().find(|(k, _)| *k == key) {
                    Some(slot) => slot.1 = row.clone(),
                    None => missing.push((key, row.clone())),
                },
            }
        }
        if !missing.is_empty() {
            let wanted: Vec<RecordValues> = missing.iter().map(|(_, r)| r.clone()).collect();
            let found = read_labels(label, lookup, column, &wanted);
            for row in found.iter().flatten() {
                let width = pairs.map_or(1, <[_]>::len);
                let values: Vec<String> = row
                    .iter()
                    .take(width)
                    .map(|v| value_text(&from_data_value(v)))
                    .collect();
                let key = if pairs.is_some() {
                    serde_json::to_string(&values).unwrap_or_default()
                } else {
                    values.first().cloned().unwrap_or_default()
                };
                let display = row.last().map(from_data_value).unwrap_or(Value::Null);
                let text = (!display.is_null()).then(|| value_text(&display));
                label_store(label, cache_key(lookup, &key), text.clone());
                if let Some(text) = text {
                    labels.insert(key.clone(), text);
                }
                missing.retain(|(k, _)| *k != key);
            }
            if found.is_some() {
                for (key, _) in &missing {
                    label_store(label, cache_key(lookup, key), None);
                }
            }
        }
        result.insert(column.clone(), labels);
    }
    result
}

/// The label for a cell: None when the column has no lookup or the key has no label yet.
pub fn label_for(
    lookups: &Lookups,
    labels: &LookupLabels,
    column: &str,
    row: &RecordValues,
) -> Option<String> {
    let lookup = &lookups.iter().find(|(c, _)| c == column)?.1;
    let key = lookup_key(lookup, column, row)?;
    labels.get(column)?.get(&key).cloned()
}

#[cfg(test)]
#[path = "lookups_tests.rs"]
mod tests;
