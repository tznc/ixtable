//! Data export (PRD §28 Phase 6): tables, saved queries and ad hoc SQL to
//! CSV, XLSX or JSON. Rows are read through DuckDB like every other read
//! (`docs/decisions/data-export.md`) and streamed to a temp file next to the
//! destination, which is renamed into place only when the export completes.
mod run;
#[cfg(test)]
mod run_tests;
mod writers;
#[cfg(test)]
mod writers_tests;

use crate::data::LogicalType;
use crate::manager::AppError;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub use writers::{open_sink, RowSink, XLSX_MAX_ROWS};

/// File format of an export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Csv,
    Xlsx,
    Json,
}

/// One exported column: its header and, when known, its logical type (table
/// exports know it; query exports infer cell types from the values).
#[derive(Debug, Clone)]
pub struct ExportColumn {
    pub name: String,
    pub logical: Option<LogicalType>,
}

/// Result of a finished export.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub rows: u64,
    pub path: String,
}

/// Exports every row of `table` that `filters` match, in `sorts` order (then
/// the table's row identity, as pages are ordered). `run_id` makes it
/// cancellable with `cancel_query`.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn export_table(
    window_label: String,
    table: String,
    sorts: Vec<crate::data::Sort>,
    filters: Vec<crate::data::Filter>,
    format: crate::export::ExportFormat,
    path: String,
    run_id: Option<String>,
) -> Result<crate::export::ExportSummary, AppError> {
    blocking(move || {
        crate::authz::check(&window_label, "table", &table, crate::authz::Op::Read)?;
        let manager = crate::manager()?;
        let plan = manager.table_plan(&window_label, &table, &sorts, &filters)?;
        let connection = manager.read_connection(&window_label)?;
        run::table(
            &connection,
            &window_label,
            run_id,
            &plan,
            format,
            Path::new(&path),
        )
    })
    .await
}

/// Exports every row of a saved read query that `filters` match, in `sorts` order.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn export_saved_query(
    window_label: String,
    id: String,
    params: Vec<crate::data::NamedValue>,
    sorts: Vec<crate::data::Sort>,
    filters: Vec<crate::data::Filter>,
    format: crate::export::ExportFormat,
    path: String,
    run_id: Option<String>,
) -> Result<crate::export::ExportSummary, AppError> {
    blocking(move || {
        crate::authz::check(&window_label, "query", &id, crate::authz::Op::Read)?;
        let manager = crate::manager()?;
        let config = manager.config(&window_label)?;
        let query = crate::queries::find_saved(&config, &id)?;
        let (sql, values) =
            crate::queries::filtered_sql(&query.sql, &query.parameters, &params, &sorts, &filters)?;
        let connection = manager.read_connection(&window_label)?;
        let out = Path::new(&path);
        run::query(
            &connection,
            &window_label,
            run_id,
            &sql,
            &values,
            format,
            out,
        )
    })
    .await
}

/// Exports the rows of ad hoc read-only SQL (developer only, like running it).
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn export_sql_query(
    window_label: String,
    sql: String,
    params: Vec<crate::data::NamedValue>,
    parameters: Option<Vec<crate::archive::QueryParameter>>,
    format: crate::export::ExportFormat,
    path: String,
    run_id: Option<String>,
) -> Result<crate::export::ExportSummary, AppError> {
    blocking(move || {
        crate::authz::require_unrestricted(&window_label, "export ad hoc SQL")?;
        let declared = parameters.unwrap_or_default();
        let (sql, values) = crate::queries::filtered_sql(&sql, &declared, &params, &[], &[])?;
        let connection = crate::manager()?.read_connection(&window_label)?;
        let out = Path::new(&path);
        run::query(
            &connection,
            &window_label,
            run_id,
            &sql,
            &values,
            format,
            out,
        )
    })
    .await
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::new("DATABASE_ERROR", e))?
}
