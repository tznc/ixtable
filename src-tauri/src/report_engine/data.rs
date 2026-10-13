//! Loading what a report run needs: dataset rows, rows of table and chart components'
//! saved queries, subreports and their rows, and image assets. Port of
//! `src/reports/data.ts`.
//!
//! Loading blocks, so callers run it on a worker thread. `LoadControl::cancel` interrupts
//! the running query in DuckDB and makes the load return `CANCELLED`.

use super::model::{band_entries, subreport_ids};
use super::subreport::MAX_SUBREPORT_DEPTH;
use super::{Row, SubreportData};
use crate::archive::{DocumentConfig, SavedQuery};
use crate::data::{DataValue, NamedValue};
use crate::manager::AppError;
use crate::queries::QueryRun;
use crate::reports::Report;
use serde_json::{Map, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Rows a report reads per query (Rust caps a run at 100,000).
pub const REPORT_ROW_LIMIT: u64 = 100_000;

/// Converts a backend cell to the plain value expressions see.
pub fn cell_value(cell: &DataValue) -> Value {
    match cell {
        DataValue::Null => Value::Null,
        DataValue::Integer(n) => Value::from(*n),
        DataValue::Real(f) => serde_json::Number::from_f64(*f).map_or(Value::Null, Value::Number),
        DataValue::Boolean(b) => Value::Bool(*b),
        DataValue::Text(s)
        | DataValue::Blob(s)
        | DataValue::Date(s)
        | DataValue::Timestamp(s)
        | DataValue::Decimal(s)
        | DataValue::Time(s) => Value::String(s.clone()),
    }
}

pub fn result_rows(columns: &[String], rows: &[Vec<DataValue>]) -> Vec<Row> {
    rows.iter()
        .map(|row| {
            columns
                .iter()
                .enumerate()
                .map(|(i, column)| (column.clone(), row.get(i).map_or(Value::Null, cell_value)))
                .collect()
        })
        .collect()
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Only the parameters a saved query declares (all of them when it declares none).
fn query_params(query: &SavedQuery, params: &Map<String, Value>) -> Vec<NamedValue> {
    let to_named = |name: &str, value: &Value| NamedValue {
        column: name.to_string(),
        value: crate::queries::json_value(value),
    };
    if query.parameters.is_empty() {
        return params.iter().map(|(k, v)| to_named(k, v)).collect();
    }
    query
        .parameters
        .iter()
        .filter_map(|p| params.get(&p.name).map(|v| to_named(&p.name, v)))
        .collect()
}

/// The reads a dataset load needs; a test supplies canned ones.
pub trait DatasetSource {
    fn run_saved(&self, query: &SavedQuery, params: Vec<NamedValue>, limit: u64, run_id: &str) -> Result<QueryRun, AppError>;
    fn run_sql(&self, sql: &str, limit: u64, run_id: &str) -> Result<QueryRun, AppError>;
    fn read_page(&self, table: &str, offset: u64, limit: u64) -> Result<crate::data::Page, AppError>;
}

/// The open document of window `label`.
pub struct Session<'a>(pub &'a str);

impl DatasetSource for Session<'_> {
    fn run_saved(&self, query: &SavedQuery, params: Vec<NamedValue>, limit: u64, run_id: &str) -> Result<QueryRun, AppError> {
        tauri::async_runtime::block_on(crate::queries::run_saved_query(
            self.0.to_string(),
            query.id.clone(),
            params,
            Some(limit),
            Some(run_id.to_string()),
        ))
    }
    fn run_sql(&self, sql: &str, limit: u64, run_id: &str) -> Result<QueryRun, AppError> {
        tauri::async_runtime::block_on(crate::queries::execute_parameterized_query(
            self.0.to_string(),
            sql.to_string(),
            vec![],
            None,
            Some(limit),
            Some(run_id.to_string()),
        ))
    }
    fn read_page(&self, table: &str, offset: u64, limit: u64) -> Result<crate::data::Page, AppError> {
        crate::read_table_page(self.0.to_string(), table.to_string(), offset, limit, vec![], vec![], None)
    }
}

/// Rows of the report's dataset: its saved query (parameters bound by Rust, never
/// interpolated), else its table, else none.
pub fn load_dataset(
    source: &dyn DatasetSource,
    report: &Report,
    config: &DocumentConfig,
    params: &Map<String, Value>,
    run_id: &str,
    limit: u64,
) -> Result<Option<QueryRun>, AppError> {
    if let Some(id) = &report.dataset_query_id {
        let query = config
            .saved_queries
            .iter()
            .find(|q| &q.id == id)
            .ok_or_else(|| AppError::new("NOT_FOUND", "The report's saved query no longer exists"))?;
        return source
            .run_saved(query, query_params(query, params), limit, run_id)
            .map(Some);
    }
    match report.table.as_deref().filter(|t| !t.is_empty()) {
        Some(table) => table_dataset(source, table, limit, run_id).map(Some),
        None => Ok(None),
    }
}

/// All rows of a table, up to `limit`. A runtime role may not run ad hoc SQL (FORBIDDEN),
/// so it reads the table page by page, which Rust authorizes.
fn table_dataset(source: &dyn DatasetSource, table: &str, limit: u64, run_id: &str) -> Result<QueryRun, AppError> {
    match source.run_sql(&format!("SELECT * FROM {}", quote_ident(table)), limit, run_id) {
        Ok(run) => return Ok(run),
        Err(e) if e.code != "FORBIDDEN" => return Err(e),
        Err(_) => {}
    }
    let started = std::time::Instant::now();
    let first = source.read_page(table, 0, limit.min(1000))?;
    let total = first.total;
    let columns: Vec<String> = first.columns.iter().map(|c| c.name.clone()).collect();
    let mut rows = first.rows;
    while (rows.len() as u64) < total.min(limit) {
        let page = source.read_page(table, rows.len() as u64, 1000.min(limit - rows.len() as u64))?;
        if page.rows.is_empty() {
            break;
        }
        rows.extend(page.rows);
    }
    Ok(QueryRun {
        columns,
        truncated: total > rows.len() as u64,
        rows,
        row_limit: limit,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

/// An image asset's bytes for preview and PDF.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetData {
    pub media_type: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct ReportData {
    pub rows: Vec<Row>,
    pub tables: HashMap<String, Vec<Row>>,
    pub assets: HashMap<String, AssetData>,
    /// Subreport definitions and rows, by report id.
    pub subreports: HashMap<String, SubreportData>,
    /// True when a query returned only the first `REPORT_ROW_LIMIT` rows.
    pub truncated: bool,
}

/// Cancels a running load: sets a flag checked between steps and interrupts the running
/// query in DuckDB.
#[derive(Clone)]
pub struct LoadControl {
    label: String,
    run_id: String,
    cancelled: Arc<AtomicBool>,
}

impl LoadControl {
    pub fn new(label: &str) -> Self {
        Self {
            label: label.to_string(),
            run_id: uuid::Uuid::new_v4().to_string(),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let _ = crate::queries::cancel_query(self.label.clone(), Some(self.run_id.clone()));
    }

    fn check(&self) -> Result<(), AppError> {
        if self.is_cancelled() {
            Err(AppError::new("CANCELLED", "Report loading cancelled"))
        } else {
            Ok(())
        }
    }
}

/// Loads everything a report needs. Cancelling `control` interrupts the running query and
/// returns `CANCELLED`.
pub fn load_report_data(
    label: &str,
    report: &Report,
    config: &DocumentConfig,
    params: &Map<String, Value>,
    control: &LoadControl,
) -> Result<ReportData, AppError> {
    load_report_data_from(&Session(label), label, report, config, params, control)
}

pub fn load_report_data_from(
    source: &dyn DatasetSource,
    label: &str,
    report: &Report,
    config: &DocumentConfig,
    params: &Map<String, Value>,
    control: &LoadControl,
) -> Result<ReportData, AppError> {
    let run_id = control.run_id();
    let mut truncated = false;
    let mut rows_of = |run: &QueryRun| {
        truncated |= run.truncated;
        result_rows(&run.columns, &run.rows)
    };
    control.check()?;
    let dataset = load_dataset(source, report, config, params, run_id, REPORT_ROW_LIMIT)?;
    control.check()?;
    // Each subreport's rows load once, in full; layout filters them per parent row.
    let mut subreports: HashMap<String, SubreportData> = HashMap::new();
    let mut printed: Vec<&Report> = vec![report];
    let mut queue: VecDeque<(String, usize)> = subreport_ids(report).into_iter().map(|id| (id, 1)).collect();
    while let Some((id, level)) = queue.pop_front() {
        let Some(sub) = config.reports.iter().find(|r| r.id == id) else {
            continue;
        };
        if subreports.contains_key(&id) || id == report.id || level > MAX_SUBREPORT_DEPTH {
            continue;
        }
        let mut values: Map<String, Value> = sub.params.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (k, v) in params {
            values.insert(k.clone(), v.clone());
        }
        let result = load_dataset(source, sub, config, &values, run_id, REPORT_ROW_LIMIT)?;
        control.check()?;
        subreports.insert(
            id,
            SubreportData {
                report: sub.clone(),
                rows: result.as_ref().map(&mut rows_of).unwrap_or_default(),
            },
        );
        printed.push(sub);
        queue.extend(subreport_ids(sub).into_iter().map(|child| (child, level + 1)));
    }
    let components: Vec<&crate::reports::Component> = printed
        .iter()
        .flat_map(|r| band_entries(r).into_iter().flat_map(|e| e.band.components.iter()))
        .collect();
    let mut tables: HashMap<String, Vec<Row>> = HashMap::new();
    for c in &components {
        if c.kind != "table" && c.kind != "chart" {
            continue;
        }
        let Some(q) = c.extra.get("queryId").and_then(Value::as_str).filter(|q| !q.is_empty()) else {
            continue;
        };
        if tables.contains_key(q) {
            continue;
        }
        let Some(query) = config.saved_queries.iter().find(|s| s.id == q) else {
            continue;
        };
        let run = source.run_saved(query, query_params(query, params), REPORT_ROW_LIMIT, run_id)?;
        control.check()?;
        tables.insert(q.to_string(), rows_of(&run));
    }
    let mut ids: Vec<String> = vec![];
    for c in &components {
        if c.kind != "image" {
            continue;
        }
        if let Some(id) = c.extra.get("assetId").and_then(Value::as_str).filter(|i| !i.is_empty()) {
            if !ids.iter().any(|i| i == id) {
                ids.push(id.to_string());
            }
        }
    }
    let mut assets = HashMap::new();
    if !ids.is_empty() {
        use base64::Engine as _;
        for a in crate::reports::read_report_assets(label.to_string(), ids)? {
            control.check()?;
            let data = base64::engine::general_purpose::STANDARD
                .decode(a.data_base64.as_bytes())
                .unwrap_or_default();
            assets.insert(a.id, AssetData { media_type: a.media_type, data });
        }
    }
    let table = if report.dataset_query_id.is_some() { None } else { report.table.as_deref() };
    let rows = match &dataset {
        Some(run) => field_text_rows(rows_of(run), config, table),
        None => vec![],
    };
    Ok(ReportData {
        rows,
        tables,
        assets,
        subreports,
        truncated,
    })
}

// ---- formatted fields ------------------------------------------------------------

/// Plain text of a rich-text value, for lists, datasheets, reports, and validation.
pub fn rich_text_plain(html: &str) -> String {
    if !html.contains(['<', '&']) {
        return html.to_string();
    }
    const DROPPED: [&str; 7] = ["SCRIPT", "STYLE", "TEMPLATE", "IFRAME", "OBJECT", "EMBED", "NOSCRIPT"];
    const BLOCKS: [&str; 8] = ["P", "DIV", "LI", "H1", "H2", "H3", "BLOCKQUOTE", "BR"];
    let chars: Vec<char> = html.chars().collect();
    let mut parts = String::new();
    let mut i = 0;
    let mut skip: Option<String> = None;
    while i < chars.len() {
        let c = chars[i];
        let tag_start = c == '<'
            && chars
                .get(i + 1)
                .is_some_and(|n| n.is_ascii_alphabetic() || *n == '/' || *n == '!');
        if tag_start {
            let end = chars[i..].iter().position(|ch| *ch == '>').map_or(chars.len(), |p| i + p + 1);
            let inner: String = chars[i + 1..end.saturating_sub(1).max(i + 1)].iter().collect();
            i = end;
            if inner.starts_with('!') {
                continue;
            }
            let closing = inner.starts_with('/');
            let name: String = inner
                .trim_start_matches('/')
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_uppercase();
            match (&skip, closing) {
                (Some(s), true) if *s == name => skip = None,
                (Some(_), _) => {}
                (None, false) => {
                    if DROPPED.contains(&name.as_str()) {
                        skip = Some(name);
                    } else if BLOCKS.contains(&name.as_str()) && !parts.is_empty() {
                        parts.push('\n');
                    }
                }
                (None, true) => {}
            }
            continue;
        }
        if skip.is_some() {
            i += 1;
            continue;
        }
        if c == '&' {
            if let Some(semi) = chars[i..].iter().take(10).position(|ch| *ch == ';') {
                let entity: String = chars[i + 1..i + semi].iter().collect();
                let decoded = match entity.as_str() {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some('\u{a0}'),
                    e if e.starts_with("#x") || e.starts_with("#X") => {
                        u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32)
                    }
                    e if e.starts_with('#') => e[1..].parse::<u32>().ok().and_then(char::from_u32),
                    _ => None,
                };
                if let Some(d) = decoded {
                    parts.push(d);
                    i += semi + 1;
                    continue;
                }
            }
        }
        parts.push(c);
        i += 1;
    }
    let mut out = String::new();
    let mut newlines = 0;
    for ch in parts.chars() {
        if ch == '\n' {
            newlines += 1;
            if newlines == 1 {
                out.push('\n');
            }
        } else {
            newlines = 0;
            out.push(ch);
        }
    }
    out.trim().to_string()
}

fn json_array(value: &Value) -> Vec<Value> {
    match value {
        Value::Array(a) => a.clone(),
        Value::String(s) if s.trim().starts_with('[') => serde_json::from_str::<Value>(s)
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default(),
        _ => vec![],
    }
}

fn plain_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        other => super::chart_geometry::js_string(other),
    }
}

/// Text of a formatted field's value in a list cell, datasheet, or report.
pub fn field_text(value: &Value, format: &str) -> String {
    match format {
        "richText" => rich_text_plain(&plain_text(value)).replace('\n', " "),
        "attachment" => {
            let files: Vec<Value> = json_array(value)
                .into_iter()
                .filter(|f| f.get("id").is_some_and(Value::is_string))
                .collect();
            match files.as_slice() {
                [one] => one.get("name").map(plain_text).unwrap_or_default(),
                [] => String::new(),
                many => format!("{} files", many.len()),
            }
        }
        "multiSelect" => {
            if value.is_null() || value == &Value::String(String::new()) {
                return String::new();
            }
            let items = json_array(value);
            let choices: Vec<String> = if !items.is_empty()
                || value.as_str().is_some_and(|s| s.trim().starts_with('['))
            {
                items.iter().map(plain_text).collect()
            } else {
                vec![plain_text(value)]
            };
            choices.join(", ")
        }
        _ => plain_text(value),
    }
}

/// Rows of a table with its formatted fields as text (rich text as plain text, files by
/// name, choices joined), the way a report shows them.
pub fn field_text_rows(rows: Vec<Row>, config: &DocumentConfig, table: Option<&str>) -> Vec<Row> {
    let fields = table
        .and_then(|t| config.entities.iter().find(|e| e.table == t))
        .map(|e| e.fields.as_slice())
        .unwrap_or_default();
    let formatted: Vec<(&str, &str)> = fields
        .iter()
        .filter_map(|f| f.format.as_deref().map(|format| (f.column.as_str(), format)))
        .collect();
    if formatted.is_empty() {
        return rows;
    }
    rows.into_iter()
        .map(|mut row| {
            for (column, format) in &formatted {
                if let Some(value) = row.get(*column) {
                    let text = field_text(value, format);
                    row.insert((*column).to_string(), Value::String(text));
                }
            }
            row
        })
        .collect()
}
