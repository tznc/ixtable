//! Record writes for views that edit records in place (port of `src/runtime/rowWrites.ts`).
//! They match the record view's save: computed bound inputs are written, disabled fields keep
//! their stored value, and updates carry the loaded values as `expected` (PRD section 19).
//! Every function blocks; call them from `ui::task::spawn_call`.

use super::schema::is_input_kind;
use super::state::{FormScope, compute, disabled_columns};
use super::values::{RecordValues, from_data_value, named_values, same_value};
use crate::data::{Column, DataValue, NamedValue, TableSchema};
use crate::design::Form;
use crate::manager::AppError;
use crate::records::{self, RecordWriteMeta, WriteError, WriteResult};

/// Values to write: the record plus computed bound inputs, minus disabled fields' edits.
pub fn values_to_write(form: &Form, scope: &FormScope, original: &RecordValues) -> RecordValues {
    let mut values = scope.record().clone();
    for control in &form.controls {
        let column = control.binding.as_ref().map(|b| b.column.as_str()).filter(|c| !c.is_empty());
        if let (Some(column), Some(computed)) = (column, control.computed.as_deref()) {
            if is_input_kind(control.kind) {
                values.insert(column.to_string(), compute(Some(computed), scope.scope()).value);
            }
        }
    }
    for column in disabled_columns(form, scope) {
        match original.get(&column) {
            Some(value) => values.insert(column, value.clone()),
            None => values.remove(&column),
        };
    }
    values
}

/// Original values for the optimistic concurrency check.
pub fn expected_values(schema: &TableSchema, original: &RecordValues) -> Vec<NamedValue> {
    let columns: Vec<&Column> = schema
        .columns
        .iter()
        .filter(|c| !c.generated && original.contains_key(&c.name))
        .collect();
    named_values(original, &columns, None)
}

/// The record after a write; `problem` is set when the write committed but a sync trigger
/// failed afterwards.
#[derive(Clone, Debug, Default)]
pub struct WriteOutcome {
    pub record: RecordValues,
    pub problem: Option<String>,
    /// The inserted row's identity (create only).
    pub identity: Vec<DataValue>,
}

/// A write that committed but whose sync trigger failed still counts as saved.
fn committed(result: Result<WriteResult, WriteError>) -> Result<(WriteResult, Option<String>), AppError> {
    match result {
        Ok(result) => Ok((result, None)),
        Err(error) if error.committed => {
            let problem = format!("Saved. {}", error.error.message);
            let result = error.results.into_iter().next().unwrap_or(WriteResult::Changed(0));
            Ok((result, Some(problem)))
        }
        Err(error) => Err(error.error),
    }
}

/// Inserts `values`; generated keys come back in the returned record.
pub fn create_row(
    label: &str,
    table: &str,
    schema: &TableSchema,
    values: &RecordValues,
) -> Result<WriteOutcome, AppError> {
    let null = |name: &str| values.get(name).is_none_or(serde_json::Value::is_null);
    let columns: Vec<&Column> = schema
        .columns
        .iter()
        .filter(|c| !(c.primary_key_position > 0 && null(&c.name)) && !null(&c.name) && !c.generated)
        .collect();
    let write = records::RecordWrite::insert(table, named_values(values, &columns, None));
    let (result, problem) = committed(records::write(label, write))?;
    let identity = match result {
        WriteResult::Inserted(identity) => identity,
        WriteResult::Changed(_) => Vec::new(),
    };
    let mut record = values.clone();
    let keys = schema.columns.iter().filter(|c| c.primary_key_position > 0);
    for (i, column) in keys.enumerate() {
        if null(&column.name) {
            let value = identity.get(i).map_or(serde_json::Value::Null, from_data_value);
            record.insert(column.name.clone(), value);
        }
    }
    Ok(WriteOutcome { record, problem, identity })
}

/// Updates the changed columns of the row `identity`; a no-op when nothing changed.
pub fn update_row(
    label: &str,
    table: &str,
    schema: &TableSchema,
    identity: &[DataValue],
    values: &RecordValues,
    original: &RecordValues,
) -> Result<WriteOutcome, AppError> {
    let changed: Vec<&Column> = schema
        .columns
        .iter()
        .filter(|c| {
            !c.generated
                && values.contains_key(&c.name)
                && !same_value(&values[&c.name], original.get(&c.name).unwrap_or(&serde_json::Value::Null))
        })
        .collect();
    let mut record = original.clone();
    record.extend(values.clone());
    if changed.is_empty() {
        return Ok(WriteOutcome { record, problem: None, identity: Vec::new() });
    }
    let meta = RecordWriteMeta {
        expected: Some(expected_values(schema, original)),
        old: Some(original.clone()),
        ..Default::default()
    };
    let write = records::RecordWrite::update(table, named_values(values, &changed, None), identity.to_vec())
        .with_meta(meta);
    let (_, problem) = committed(records::write(label, write))?;
    Ok(WriteOutcome { record, problem, identity: Vec::new() })
}

/// Deletes the row `identity`, checking the loaded values (`original`) as `expected`.
pub fn delete_row(
    label: &str,
    table: &str,
    schema: &TableSchema,
    identity: &[DataValue],
    original: &RecordValues,
) -> Result<(), AppError> {
    let meta = RecordWriteMeta {
        expected: Some(expected_values(schema, original)),
        old: Some(original.clone()),
        ..Default::default()
    };
    let write = records::RecordWrite::delete(table, identity.to_vec()).with_meta(meta);
    records::write(label, write).map(|_| ()).map_err(|e| e.error)
}

/// The text shown for a failed write: a concurrent change explains how to recover.
pub fn write_message(error: &AppError) -> String {
    if error.code == "CONFLICT" {
        return "Someone else changed this record since you opened it. Go back and open it again to see the latest values.".into();
    }
    error.message.clone()
}

#[cfg(test)]
#[path = "row_writes_tests.rs"]
mod tests;
