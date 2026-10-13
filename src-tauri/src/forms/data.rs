//! Runtime record data access (port of `src/runtime/data.ts`). Reads go through the backend's
//! DuckDB readers: table pages, saved query pages, and filtered scans. Every function blocks,
//! so views call them from `ui::task::spawn_call`. Caches are per window label and are
//! cleared by a record hook after every write; `invalidate` clears them on a schema change.

use super::conditions::{RowPredicate, SCAN_CHUNK, paged_scan, row_filter, scan_filtered};
use super::fields::value_text;
use super::pushdown::pushdown;
use super::values::{RecordValues, from_data_value, row_object, to_data_value};
use crate::data::{Column, DataValue, Filter, FilterOperator, NamedValue, Sort, TableSchema};
use crate::design::{Form, Relationship, SourceKind};
use crate::expr::{self, EvaluateOptions, ExprError, Scope};
use crate::manager::AppError;
use crate::records::{self, RecordHook, Registration, WriteResult};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// One page of records for a list. `identities` is None for read-only (query) sources.
/// `truncated` is true when a row filter stopped at `SCAN_LIMIT` rows: matches past that
/// point are missing and `total` counts only the rows scanned.
#[derive(Clone, Debug, Default)]
pub struct RecordPage {
    pub columns: Vec<String>,
    pub rows: Vec<RecordValues>,
    pub identities: Option<Vec<Vec<DataValue>>>,
    pub total: u64,
    pub truncated: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PageRequest {
    pub offset: u64,
    pub limit: u64,
    pub sorts: Vec<Sort>,
    pub filters: Vec<Filter>,
}

/// A query source's bound parameter values.
pub type Bound = Map<String, Value>;

#[derive(Clone)]
struct Matched {
    record: RecordValues,
    identity: Option<Vec<DataValue>>,
}

struct Scanned {
    matches: Vec<Matched>,
    truncated: bool,
    columns: Vec<String>,
    keyed: bool,
}

/// A loaded record with the identity values its row is written by.
#[derive(Clone, Debug)]
pub struct LoadedRecord {
    pub record: RecordValues,
    pub identity: Vec<DataValue>,
}

#[derive(Default)]
struct Caches {
    schemas: HashMap<String, Arc<TableSchema>>,
    pages: HashMap<String, RecordPage>,
    scans: HashMap<String, Arc<Scanned>>,
    snapshots: HashMap<String, LoadedRecord>,
    // Lookup display labels keyed by relationship and key (see `lookups`).
    labels: HashMap<String, Option<String>>,
    shown: HashMap<String, String>,
}

struct LabelData {
    caches: Mutex<Caches>,
    _hook: Registration,
}

struct Invalidate(String);

impl RecordHook for Invalidate {
    fn after(&self, _: &records::RecordWrite, _: &WriteResult, _: Option<&str>) -> Result<(), AppError> {
        invalidate(&self.0, false);
        Ok(())
    }
}

fn registry() -> std::sync::MutexGuard<'static, HashMap<String, Arc<LabelData>>> {
    static REGISTRY: std::sync::OnceLock<Mutex<HashMap<String, Arc<LabelData>>>> =
        std::sync::OnceLock::new();
    REGISTRY
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn data(label: &str) -> Arc<LabelData> {
    registry()
        .entry(label.to_string())
        .or_insert_with(|| {
            Arc::new(LabelData {
                caches: Mutex::default(),
                _hook: records::register_hook(label, Arc::new(Invalidate(label.to_string()))),
            })
        })
        .clone()
}

/// A window's cache handle; `lock` gives the caches.
struct Handle(Arc<LabelData>);

impl Handle {
    fn lock(&self) -> std::sync::MutexGuard<'_, Caches> {
        self.0.caches.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn caches(label: &str) -> Handle {
    Handle(data(label))
}

/// Drops cached pages, scans and lookup labels; `schema_too` also drops table schemas (after
/// DDL). Runs after every record write of the window.
pub fn invalidate(label: &str, schema_too: bool) {
    let handle = caches(label);
    let mut c = handle.lock();
    c.pages.clear();
    c.scans.clear();
    c.labels.clear();
    if schema_too {
        c.schemas.clear();
    }
}

/// Forgets a window's caches and hook (its document closed).
pub fn forget(label: &str) {
    registry().remove(label);
}

pub(super) fn label_cached(label: &str, key: &str) -> Option<Option<String>> {
    caches(label).lock().labels.get(key).cloned()
}

pub(super) fn label_store(label: &str, key: String, text: Option<String>) {
    caches(label).lock().labels.insert(key, text);
}

pub fn table_schema(label: &str, table: &str) -> Result<Arc<TableSchema>, AppError> {
    if let Some(hit) = caches(label).lock().schemas.get(table) {
        return Ok(hit.clone());
    }
    let schema = Arc::new(crate::inspect_table(label.to_string(), table.to_string(), None)?);
    caches(label).lock().schemas.insert(table.to_string(), schema.clone());
    Ok(schema)
}

/// A table with what generated forms need around it: the tables its foreign keys point at
/// (lookup display columns) and the tables pointing at it (one-level related lists).
pub struct TableContext {
    pub schema: Arc<TableSchema>,
    pub targets: HashMap<String, Arc<TableSchema>>,
    pub children: Vec<Arc<TableSchema>>,
}

pub fn table_context(label: &str, table: &str) -> Result<TableContext, AppError> {
    let schema = table_schema(label, table)?;
    let known: Vec<Arc<TableSchema>> = crate::list_database_objects(label.to_string())?
        .into_iter()
        .filter(|object| object.object_type == "table")
        .filter_map(|object| table_schema(label, &object.name).ok())
        .collect();
    Ok(TableContext {
        schema,
        targets: known.iter().map(|t| (t.name.clone(), t.clone())).collect(),
        children: known
            .iter()
            .filter(|other| other.foreign_keys.iter().any(|fk| fk.target_table == table))
            .cloned()
            .collect(),
    })
}

fn page_key(form: &Form, request: &PageRequest, scope: &Scope, bound: &Bound) -> String {
    let filter = form
        .filter
        .as_deref()
        .filter(|f| !f.trim().is_empty())
        .map(|f| serde_json::to_string(&(f, scope)).unwrap_or_default());
    format!("{:?}|{:?}|{:?}|{:?}", form.source, bound, filter, request)
}

/// A cached page for instant re-navigation (None when not loaded yet).
pub fn cached_page(
    label: &str,
    form: &Form,
    request: &PageRequest,
    scope: &Scope,
    bound: &Bound,
) -> Option<RecordPage> {
    caches(label)
        .lock()
        .pages
        .get(&page_key(form, request, scope, bound))
        .cloned()
}

/// Where a form's pages come from, plus filters pushed down from its row filter.
#[derive(Clone)]
struct Reader {
    query_id: Option<String>,
    table: Option<String>,
    bound: Bound,
    extra: Vec<Filter>,
}

impl Reader {
    fn read(&self, label: &str, request: &PageRequest) -> Result<RecordPage, AppError> {
        let mut request = request.clone();
        request.filters.extend(self.extra.clone());
        match (&self.query_id, &self.table) {
            (Some(query), _) => query_page(label, query, &request, &self.bound),
            (None, Some(table)) => table_page(label, table, &request),
            (None, None) => Ok(RecordPage::default()),
        }
    }
}

fn named(bound: &Bound) -> Vec<NamedValue> {
    bound
        .iter()
        .map(|(column, value)| NamedValue {
            column: column.clone(),
            value: to_data_value(value),
        })
        .collect()
}

/// Query sources page in DuckDB (the backend wraps the saved SQL), so totals are exact.
fn query_page(
    label: &str,
    query_id: &str,
    request: &PageRequest,
    bound: &Bound,
) -> Result<RecordPage, AppError> {
    let result = tauri::async_runtime::block_on(crate::queries::run_saved_query_page(
        label.to_string(),
        query_id.to_string(),
        named(bound),
        request.offset,
        request.limit,
        request.sorts.clone(),
        request.filters.clone(),
        None,
    ))?;
    Ok(RecordPage {
        rows: result.rows.iter().map(|row| row_object(&result.columns, row)).collect(),
        columns: result.columns,
        identities: None,
        total: result.total,
        truncated: false,
    })
}

/// One page of a table as record values; the page's own column list is returned.
pub fn table_page(label: &str, table: &str, request: &PageRequest) -> Result<RecordPage, AppError> {
    let result = crate::read_table_page(
        label.to_string(),
        table.to_string(),
        request.offset,
        request.limit,
        request.sorts.clone(),
        request.filters.clone(),
        None,
    )?;
    let columns: Vec<String> = result.columns.iter().map(|c| c.name.clone()).collect();
    Ok(RecordPage {
        rows: result.rows.iter().map(|row| row_object(&columns, row)).collect(),
        columns,
        identities: Some(result.identities),
        total: result.total,
        truncated: false,
    })
}

/// A page with a row filter. The source is scanned once (up to `SCAN_LIMIT` rows) per bound
/// params, sort, search, filter and filter inputs; every page is then a slice of the cached
/// matches, so paging never rescans and `total` is the real match count.
fn filtered_page(
    label: &str,
    reader: &Reader,
    request: &PageRequest,
    keep: &RowPredicate,
    key: String,
) -> Result<RecordPage, AppError> {
    let hit = caches(label).lock().scans.get(&key).cloned();
    let found = match hit {
        Some(found) => found,
        None => {
            let mut columns: Vec<String> = vec![];
            let mut keyed = false;
            let scan = paged_scan(
                |offset, limit| {
                    let chunk = reader.read(
                        label,
                        &PageRequest {
                            offset: offset as u64,
                            limit: limit as u64,
                            ..request.clone()
                        },
                    )?;
                    keyed = chunk.identities.is_some();
                    let identities = chunk.identities;
                    let rows: Vec<Matched> = chunk
                        .rows
                        .into_iter()
                        .enumerate()
                        .map(|(i, record)| Matched {
                            record,
                            identity: identities.as_ref().and_then(|ids| ids.get(i).cloned()),
                        })
                        .collect();
                    columns = chunk.columns;
                    Ok::<_, AppError>(rows)
                },
                |row: &Matched| keep(&row.record),
            )?;
            let found = Arc::new(Scanned {
                matches: scan.matches,
                truncated: scan.truncated,
                columns,
                keyed,
            });
            caches(label).lock().scans.insert(key, found.clone());
            found
        }
    };
    let shown: Vec<&Matched> = found
        .matches
        .iter()
        .skip(request.offset as usize)
        .take(request.limit as usize)
        .collect();
    Ok(RecordPage {
        columns: found.columns.clone(),
        rows: shown.iter().map(|m| m.record.clone()).collect(),
        identities: found
            .keyed
            .then(|| shown.iter().map(|m| m.identity.clone().unwrap_or_default()).collect()),
        total: found.matches.len() as u64,
        truncated: found.truncated,
    })
}

/// The scope a form's row filter sees; each row is added as `record` (see `row_filter`).
pub fn filter_scope(scope: &Scope) -> Scope {
    let mut out = Scope::new();
    out.insert("form".into(), Value::Object(Map::new()));
    out.insert("params".into(), Value::Object(Map::new()));
    out.insert("parent".into(), Value::Null);
    out.extend(scope.clone());
    out
}

fn filter_error(error: ExprError) -> AppError {
    AppError::new("INVALID_FILTER", error.message)
}

/// The pushable part of a table source's row filter applied in DuckDB, and the predicate for
/// the rest (None when DuckDB applies all of it). None when nothing can be pushed.
type Pushed = (Vec<Filter>, Option<RowPredicate>);

fn pushed_reader(
    label: &str,
    form: &Form,
    scope: &Scope,
) -> Result<Option<Pushed>, AppError> {
    let table = match form.source.as_ref() {
        Some(source) if source.kind == SourceKind::Table => source.table.as_deref(),
        _ => None,
    };
    let (Some(table), Some(filter)) = (table, form.filter.as_deref().filter(|f| !f.trim().is_empty()))
    else {
        return Ok(None);
    };
    let ast = expr::parse_cached(filter).map_err(filter_error)?;
    let schema = table_schema(label, table)?;
    let split = pushdown(&ast, scope, &schema.columns);
    if split.filters.is_empty() {
        return Ok(None);
    }
    let keep: Option<RowPredicate> = split.rest.map(|rest| {
        let base = scope.clone();
        Arc::new(move |record: &Map<String, Value>| {
            let mut scope = base.clone();
            scope.insert("record".into(), Value::Object(record.clone()));
            expr::evaluate_ast_boolean(&rest, &scope, EvaluateOptions::default()).unwrap_or(false)
        }) as RowPredicate
    });
    Ok(Some((split.filters, keep)))
}

/// Reads pages of a form's source (table page or saved query page), or None without one.
fn source_reader(form: &Form, bound: &Bound) -> Option<Reader> {
    let source = form.source.as_ref()?;
    let (query_id, table) = match source.kind {
        SourceKind::Query => (source.query_id.clone().filter(|q| !q.is_empty()), None),
        SourceKind::Table => (None, source.table.clone().filter(|t| !t.is_empty())),
    };
    (query_id.is_some() || table.is_some()).then(|| Reader {
        query_id,
        table,
        bound: bound.clone(),
        extra: vec![],
    })
}

/// Reads one page of a form's source through DuckDB (table page or saved query). `bound`
/// are the query source's bound parameter values (see `source_params`). A form `filter`
/// expression is applied to the fetched rows with `scope` (`app`, `params`).
pub fn load_page(
    label: &str,
    form: &Form,
    request: &PageRequest,
    scope: &Scope,
    bound: &Bound,
) -> Result<RecordPage, AppError> {
    let fscope = filter_scope(scope);
    let mut keep = row_filter(form.filter.as_deref(), &fscope).map_err(filter_error)?;
    let mut reader = source_reader(form, bound);
    if reader.is_some() && keep.is_some() {
        if let Some((filters, rest)) = pushed_reader(label, form, &fscope)? {
            if let Some(reader) = reader.as_mut() {
                reader.extra = filters;
            }
            keep = rest;
        }
    }
    let page = match (&reader, &keep) {
        (None, _) => RecordPage::default(),
        (Some(reader), Some(keep)) => {
            let scan_key = page_key(
                form,
                &PageRequest {
                    offset: 0,
                    limit: 0,
                    ..request.clone()
                },
                scope,
                bound,
            );
            filtered_page(label, reader, request, keep, scan_key)?
        }
        (Some(reader), None) => reader.read(label, request)?,
    };
    caches(label)
        .lock()
        .pages
        .insert(page_key(form, request, scope, bound), page.clone());
    Ok(page)
}

/// Whether `record` passes the form's row filter, evaluated with `scope` as in `load_page`
/// (true without a filter). A filter that does not parse is an error.
pub fn passes_filter(form: &Form, record: &RecordValues, scope: &Scope) -> Result<bool, AppError> {
    let keep = row_filter(form.filter.as_deref(), &filter_scope(scope)).map_err(filter_error)?;
    Ok(keep.is_none_or(|keep| keep(record)))
}

/// Evaluates a query source's parameter bindings (`source.params`: name to expression over
/// `app` and `params`). The error names the parameter that failed.
pub fn source_params(form: &Form, scope: &Scope) -> Result<Bound, String> {
    let mut out = Bound::new();
    let Some(source) = form.source.as_ref().filter(|s| s.kind == SourceKind::Query) else {
        return Ok(out);
    };
    for (name, src) in &source.params {
        if src.trim().is_empty() {
            continue;
        }
        let value = expr::evaluate(src, scope, EvaluateOptions::default())
            .map_err(|e| format!("Parameter ${name}: {}", e.message))?;
        out.insert(name.clone(), value);
    }
    Ok(out)
}

/// The record id of the first row of a form's source that passes its row filter (the row
/// itself for query sources), or None when there is none. The scan stops at the first match.
/// Used to open detail/edit views that have no record of their own (design preview,
/// dashboard-embedded forms).
pub fn first_record_id(
    label: &str,
    form: &Form,
    bound: &Bound,
    scope: &Scope,
) -> Result<Option<Value>, AppError> {
    let Some(mut reader) = source_reader(form, bound) else {
        return Ok(None);
    };
    let fscope = filter_scope(scope);
    let mut keep = row_filter(form.filter.as_deref(), &fscope).map_err(filter_error)?;
    if keep.is_some() {
        if let Some((filters, rest)) = pushed_reader(label, form, &fscope)? {
            reader.extra = filters;
            keep = rest;
        }
    }
    let chunk = if keep.is_some() { SCAN_CHUNK } else { 1 };
    let scan = scan_filtered(
        |offset, limit| {
            let page = reader.read(
                label,
                &PageRequest {
                    offset: offset as u64,
                    limit: limit as u64,
                    ..Default::default()
                },
            )?;
            let identities = page.identities;
            Ok::<_, AppError>(
                page.rows
                    .into_iter()
                    .enumerate()
                    .map(|(i, record)| Matched {
                        record,
                        identity: identities.as_ref().and_then(|ids| ids.get(i).cloned()),
                    })
                    .collect::<Vec<_>>(),
            )
        },
        |row: &Matched| keep.as_ref().is_none_or(|keep| keep(&row.record)),
        1,
        chunk,
    )?;
    let Some(first) = scan.matches.into_iter().next() else {
        return Ok(None);
    };
    let table = form
        .source
        .as_ref()
        .filter(|s| s.kind == SourceKind::Table)
        .and_then(|s| s.table.clone())
        .filter(|t| !t.is_empty());
    match (table, first.identity) {
        (Some(table), Some(identity)) => {
            let schema = table_schema(label, &table)?;
            Ok(Some(record_id_for(label, &schema, &first.record, &identity)))
        }
        _ => Ok(Some(Value::Object(first.record))),
    }
}

/// Primary key columns in key order.
pub fn key_columns(schema: &TableSchema) -> Vec<String> {
    let mut keys: Vec<&Column> = schema
        .columns
        .iter()
        .filter(|c| c.primary_key_position > 0)
        .collect();
    keys.sort_by_key(|c| c.primary_key_position);
    keys.into_iter().map(|c| c.name.clone()).collect()
}

/// The id passed to the form renderer for a row: the key value, a key object, or a rowid
/// snapshot key.
pub fn record_id_for(
    label: &str,
    schema: &TableSchema,
    record: &RecordValues,
    identity: &[DataValue],
) -> Value {
    let keys = key_columns(schema);
    let get = |k: &String| record.get(k).cloned().unwrap_or(Value::Null);
    match keys.len() {
        1 => get(&keys[0]),
        0 => {
            let first = identity.first().map(from_data_value).unwrap_or(Value::Null);
            let id = format!("rowid:{}:{}", schema.name, value_text(&first));
            caches(label).lock().snapshots.insert(
                id.clone(),
                LoadedRecord {
                    record: record.clone(),
                    identity: identity.to_vec(),
                },
            );
            Value::String(id)
        }
        _ => Value::Object(keys.iter().map(|k| (k.clone(), get(k))).collect()),
    }
}

fn numeric_declared(declared: &str) -> bool {
    let upper = declared.to_uppercase();
    ["INT", "REAL", "NUM", "DEC", "FLOA", "DOUB"].iter().any(|t| upper.contains(t))
}

fn key_filter(schema: &TableSchema, column: &str, raw: &Value) -> Filter {
    let declared = schema
        .columns
        .iter()
        .find(|c| c.name == column)
        .map_or("", |c| c.declared_type.as_str());
    let number = match raw {
        Value::Number(n) => n.as_f64(),
        Value::String(s) if !s.is_empty() => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
        _ => None,
    };
    let value = match (raw, number) {
        (Value::Null, _) => DataValue::Null,
        (_, Some(n)) if numeric_declared(declared) => {
            if n.fract() == 0.0 && n.abs() < 9.0e15 {
                DataValue::Integer(n as i64)
            } else {
                DataValue::Real(n)
            }
        }
        _ => DataValue::Text(value_text(raw)),
    };
    Filter {
        column: column.into(),
        operator: FilterOperator::Eq,
        value: Some(value),
        values: None,
    }
}

/// Loads one record by id through DuckDB.
pub fn load_record(label: &str, table: &str, record_id: &Value) -> Result<Option<LoadedRecord>, AppError> {
    if let Value::String(id) = record_id {
        if let Some(hit) = caches(label).lock().snapshots.get(id) {
            return Ok(Some(hit.clone()));
        }
    }
    let schema = table_schema(label, table)?;
    let keys = key_columns(&schema);
    let key_values: RecordValues = match record_id {
        Value::Object(map) => map.clone(),
        other => match keys.first() {
            Some(key) => Map::from_iter([(key.clone(), other.clone())]),
            None => return Ok(None),
        },
    };
    let filters: Vec<Filter> = key_values
        .iter()
        .filter(|(c, _)| schema.columns.iter().any(|x| &x.name == *c))
        .map(|(column, raw)| key_filter(&schema, column, raw))
        .collect();
    if filters.is_empty() {
        return Ok(None);
    }
    let page = table_page(
        label,
        table,
        &PageRequest {
            limit: 1,
            filters,
            ..Default::default()
        },
    )?;
    let (Some(record), Some(identity)) = (
        page.rows.into_iter().next(),
        page.identities.and_then(|ids| ids.into_iter().next()),
    ) else {
        return Ok(None);
    };
    Ok(Some(LoadedRecord { record, identity }))
}

/// A choice; relationship choices also carry the target record (multi-column keys).
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub value: Value,
    pub label: String,
    pub record: Option<RecordValues>,
}

/// A relationship key: the stored value of a single-column key, or target column to value
/// for a multi-column key (a JSON object). Empty when any part is missing.
pub fn empty_key(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Object(parts) => parts
            .values()
            .any(|part| part.is_null() || part.as_str() == Some("")),
        _ => false,
    }
}

fn key_text(value: &Value) -> String {
    match value {
        Value::Object(_) => value.to_string(),
        other => value_text(other),
    }
}

/// Lookup choices for a relationship selector, searched on the display column. A relationship
/// `filter` keeps only choice rows it accepts (`record` is the choice row; `scope` supplies
/// `parent`, `form` and `app`); rows are scanned until `limit` match.
pub fn relationship_choices(
    label: &str,
    relationship: &Relationship,
    search: &str,
    limit: usize,
    scope: &Scope,
) -> Result<Vec<Choice>, AppError> {
    let filters: Vec<Filter> = if search.trim().is_empty() {
        vec![]
    } else {
        vec![Filter {
            column: relationship.display_column.clone(),
            operator: FilterOperator::Contains,
            value: Some(DataValue::Text(search.trim().to_string())),
            values: None,
        }]
    };
    let sorts = vec![Sort {
        column: relationship.display_column.clone(),
        descending: false,
    }];
    let mut full = Scope::new();
    for name in ["form", "app", "params"] {
        full.insert(name.into(), Value::Object(Map::new()));
    }
    full.extend(scope.clone());
    let keep = row_filter(relationship.filter.as_deref(), &full).map_err(filter_error)?;
    let to_choice = |record: RecordValues| {
        let shown = record
            .get(&relationship.display_column)
            .filter(|v| !v.is_null())
            .or_else(|| record.get(&relationship.value_column))
            .map(value_text)
            .unwrap_or_default();
        Choice {
            value: record.get(&relationship.value_column).cloned().unwrap_or(Value::Null),
            label: shown,
            record: Some(record),
        }
    };
    let read = |offset: u64, size: u64| {
        table_page(
            label,
            &relationship.table,
            &PageRequest {
                offset,
                limit: size,
                sorts: sorts.clone(),
                filters: filters.clone(),
            },
        )
    };
    let Some(keep) = keep else {
        return Ok(read(0, limit as u64)?.rows.into_iter().map(to_choice).collect());
    };
    let scan = scan_filtered(
        |offset, size| Ok::<_, AppError>(read(offset as u64, size as u64)?.rows),
        |record: &RecordValues| keep(record),
        limit,
        SCAN_CHUNK,
    )?;
    Ok(scan.matches.into_iter().take(limit).map(to_choice).collect())
}

fn label_key(relationship: &Relationship, value: &Value) -> String {
    serde_json::to_string(&(
        &relationship.table,
        &relationship.value_column,
        &relationship.display_column,
        key_text(value),
    ))
    .unwrap_or_default()
}

/// Display label for one stored relationship key.
pub fn relationship_label(label: &str, relationship: &Relationship, value: &Value) -> String {
    if empty_key(value) {
        return String::new();
    }
    let keys = match value {
        Value::Object(_) => value.clone(),
        other => Value::Object(Map::from_iter([(relationship.value_column.clone(), other.clone())])),
    };
    let hit = load_record(label, &relationship.table, &keys).ok().flatten();
    let text = match hit.as_ref().and_then(|h| h.record.get(&relationship.display_column)) {
        Some(shown) if !shown.is_null() => value_text(shown),
        _ if value.is_object() => String::new(),
        _ => value_text(value),
    };
    caches(label).lock().shown.insert(label_key(relationship, value), text.clone());
    text
}

/// The label `relationship_label` last returned for this key, if any (stale-while-revalidate).
pub fn cached_relationship_label(label: &str, relationship: &Relationship, value: &Value) -> Option<String> {
    if empty_key(value) {
        return Some(String::new());
    }
    caches(label).lock().shown.get(&label_key(relationship, value)).cloned()
}

/// Choices from a saved query: the first column is the value, the second (optional) the label.
pub fn query_choices(label: &str, query_id: &str) -> Result<Vec<Choice>, AppError> {
    let result = tauri::async_runtime::block_on(crate::queries::run_saved_query(
        label.to_string(),
        query_id.to_string(),
        vec![],
        None,
        None,
    ))?;
    Ok(result
        .rows
        .iter()
        .map(|row| {
            let value = row.first().map(from_data_value).unwrap_or(Value::Null);
            let shown = row.get(1).or(row.first()).map(from_data_value).unwrap_or(Value::Null);
            Choice {
                value,
                label: value_text(&shown),
                record: None,
            }
        })
        .collect())
}

/// The DuckDB filters that reproduce a form's row filter, for exports that read the source in
/// the backend: `Some(vec![])` without a filter, None when part of it runs only per row (the
/// export would then include rows the list hides).
pub fn export_filters(label: &str, form: &Form, scope: &Scope) -> Option<Vec<Filter>> {
    let filter = form.filter.as_deref().filter(|f| !f.trim().is_empty());
    let Some(filter) = filter else {
        return Some(vec![]);
    };
    let table = form
        .source
        .as_ref()
        .filter(|s| s.kind == SourceKind::Table)
        .and_then(|s| s.table.as_deref())?;
    let ast = expr::parse_cached(filter).ok()?;
    let schema = table_schema(label, table).ok()?;
    let split = pushdown(&ast, &filter_scope(scope), &schema.columns);
    if split.rest.is_some() || split.filters.is_empty() {
        None
    } else {
        Some(split.filters)
    }
}

#[cfg(test)]
#[path = "data_tests.rs"]
pub(crate) mod tests;
