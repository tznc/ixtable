//! A record's place in the ordered records of a collection view (port of
//! `src/runtime/cursor.ts`): the form that lists them, its sort and search, its page
//! parameters, and the index. The record navigation bar moves through these positions.

use super::data::{PageRequest, RecordPage, load_page, record_id_for, source_params, table_schema};
use crate::data::{Filter, Sort};
use crate::design::{Form, SourceKind};
use crate::expr::Scope;
use crate::manager::AppError;
use serde_json::{Map, Value};

#[derive(Clone, Debug, Default)]
pub struct RecordCursor {
    /// Form whose source, filter and sort order define the sequence.
    pub form_id: String,
    pub index: usize,
    pub sorts: Vec<Sort>,
    pub filters: Vec<Filter>,
    pub params: Map<String, Value>,
}

/// The record at a cursor position and the size of the whole sequence.
#[derive(Clone, Debug, PartialEq)]
pub struct Positioned {
    /// Null past the end of the sequence.
    pub record_id: Value,
    pub total: u64,
}

/// The id of the record at `index` of the page's row set.
pub fn id_for_row(label: &str, form: &Form, page: &RecordPage, index: usize) -> Result<Value, AppError> {
    let Some(record) = page.rows.get(index) else {
        return Ok(Value::Null);
    };
    let table = form
        .source
        .as_ref()
        .filter(|s| s.kind == SourceKind::Table)
        .and_then(|s| s.table.clone())
        .filter(|t| !t.is_empty());
    let identity = page.identities.as_ref().and_then(|ids| ids.get(index));
    // Query sources (and keyless reads) open by the row itself, as list mode does.
    let (Some(table), Some(identity)) = (table, identity) else {
        return Ok(Value::Object(record.clone()));
    };
    Ok(record_id_for(label, &*table_schema(label, &table)?, record, identity))
}

/// Reads the record id at `index` of the cursor's sequence through the same DuckDB page
/// reader as list mode, so the order matches what the list showed.
pub fn record_at(
    label: &str,
    form: &Form,
    cursor: &RecordCursor,
    index: usize,
    app: &Map<String, Value>,
) -> Result<Positioned, AppError> {
    let mut scope = Scope::new();
    scope.insert("app".into(), Value::Object(app.clone()));
    scope.insert("params".into(), Value::Object(cursor.params.clone()));
    let bound = source_params(form, &scope).map_err(|m| AppError::new("PARAMETER", m))?;
    let page = load_page(
        label,
        form,
        &PageRequest {
            offset: index as u64,
            limit: 1,
            sorts: cursor.sorts.clone(),
            filters: cursor.filters.clone(),
        },
        &scope,
        &bound,
    )?;
    if page.rows.is_empty() {
        return Ok(Positioned { record_id: Value::Null, total: page.total });
    }
    Ok(Positioned { record_id: id_for_row(label, form, &page, 0)?, total: page.total })
}

#[cfg(test)]
#[path = "cursor_tests.rs"]
mod tests;
