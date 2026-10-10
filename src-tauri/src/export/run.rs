//! Streams a DuckDB result into an export file (see `writers`). Rows go to a
//! hidden temp file beside the destination, which replaces the destination
//! only after the writer finishes, so a failed or cancelled export never
//! leaves a truncated file or clobbers an existing one.
use super::{open_sink, ExportColumn, ExportFormat, ExportSummary, RowSink};
use crate::data::page::PagePlan;
use crate::data::{duck_value, DataValue};
use crate::manager::AppError;
use crate::queries::RunGuard;
use duckdb::types::Value as DuckValue;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

/// Exports a table page plan: typed columns, the trailing rowid dropped.
pub fn table(
    connection: &duckdb::Connection,
    window: &str,
    run_id: Option<String>,
    plan: &PagePlan,
    format: ExportFormat,
    path: &Path,
) -> Result<ExportSummary, AppError> {
    let columns: Vec<ExportColumn> = plan
        .columns
        .iter()
        .map(|c| ExportColumn {
            name: c.name.clone(),
            logical: Some(c.logical_type.clone()),
        })
        .collect();
    stream(
        connection,
        window,
        run_id,
        &plan.sql,
        &plan.binds,
        format,
        path,
        |_| Ok(columns.clone()),
        |row| {
            let mut values = plan.read_row(row)?;
            values.truncate(plan.columns.len());
            Ok(values)
        },
    )
}

/// Exports a query's result: column names from the statement, values as read.
pub fn query(
    connection: &duckdb::Connection,
    window: &str,
    run_id: Option<String>,
    sql: &str,
    values: &[DuckValue],
    format: ExportFormat,
    path: &Path,
) -> Result<ExportSummary, AppError> {
    stream(
        connection,
        window,
        run_id,
        sql,
        values,
        format,
        path,
        |stmt| {
            Ok(stmt
                .column_names()
                .into_iter()
                .map(|name| ExportColumn {
                    name,
                    logical: None,
                })
                .collect())
        },
        |row| {
            let count = row.as_ref().column_count();
            (0..count)
                .map(|i| Ok(duck_value(row.get::<_, DuckValue>(i)?)))
                .collect()
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn stream(
    connection: &duckdb::Connection,
    window: &str,
    run_id: Option<String>,
    sql: &str,
    values: &[DuckValue],
    format: ExportFormat,
    path: &Path,
    columns: impl FnOnce(&duckdb::Statement) -> Result<Vec<ExportColumn>, AppError>,
    read: impl Fn(&duckdb::Row) -> duckdb::Result<Vec<DataValue>>,
) -> Result<ExportSummary, AppError> {
    let run_id = run_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let guard = RunGuard::register(window, &run_id, connection);
    let fail = |e: String| {
        if guard.cancelled.load(Ordering::SeqCst) || e.to_ascii_lowercase().contains("interrupt") {
            AppError::new("CANCELLED", "Export cancelled")
        } else {
            AppError::new("DATABASE_ERROR", e)
        }
    };
    let tmp = temp_path(path)?;
    let result = (|| -> Result<u64, AppError> {
        let mut stmt = connection.prepare(sql).map_err(|e| fail(e.to_string()))?;
        let mut rows = stmt
            .query(duckdb::params_from_iter(values.iter()))
            .map_err(|e| fail(e.to_string()))?;
        let header = columns(rows.as_ref().expect("an executed statement"))?;
        let mut sink: Box<dyn RowSink> =
            open_sink(format, &tmp, &header).map_err(|e| AppError::new("IO_ERROR", e))?;
        let mut count = 0u64;
        while let Some(row) = rows.next().map_err(|e| fail(e.to_string()))? {
            let values = read(row).map_err(|e| fail(e.to_string()))?;
            sink.row(&values)
                .map_err(|e| AppError::new("VALIDATION_ERROR", e))?;
            count += 1;
        }
        sink.finish().map_err(|e| AppError::new("IO_ERROR", e))?;
        // Windows flushes only through a handle opened for writing.
        OpenOptions::new()
            .write(true)
            .open(&tmp)
            .and_then(|f| f.sync_all())
            .map_err(|e| AppError::new("IO_ERROR", e))?;
        fs::rename(&tmp, path).map_err(|e| AppError::new("IO_ERROR", e))?;
        Ok(count)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    Ok(ExportSummary {
        rows: result?,
        path: path.to_string_lossy().into_owned(),
    })
}

/// A hidden sibling of `path` (same directory, so the final rename is atomic).
fn temp_path(path: &Path) -> Result<PathBuf, AppError> {
    let name = path
        .file_name()
        .ok_or_else(|| AppError::new("VALIDATION_ERROR", "Export path has no file name"))?
        .to_string_lossy();
    let dir = path.parent().unwrap_or(Path::new("."));
    if !dir.as_os_str().is_empty() && !dir.is_dir() {
        return Err(AppError::new(
            "IO_ERROR",
            format!("Folder {} does not exist", dir.display()),
        ));
    }
    Ok(dir.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4())))
}
