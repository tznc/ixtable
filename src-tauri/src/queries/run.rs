//! Statement execution with row limits and cancellation.
use super::*;

/// Runs a prepared statement, stopping after `limit` rows.
pub fn execute(
    connection: &duckdb::Connection,
    sql: &str,
    values: &[DuckValue],
    limit: u64,
) -> Result<QueryRun, String> {
    let started = Instant::now();
    let mut stmt = connection.prepare(sql).map_err(|e| e.to_string())?;
    let mut cursor = stmt
        .query(duckdb::params_from_iter(values.iter()))
        .map_err(|e| e.to_string())?;
    let mut rows = vec![];
    let mut truncated = false;
    while let Some(row) = cursor.next().map_err(|e| e.to_string())? {
        if rows.len() as u64 >= limit {
            truncated = true;
            break;
        }
        let count = row.as_ref().column_count();
        rows.push(
            (0..count)
                .map(|i| data::duck_value(row.get::<_, DuckValue>(i).unwrap_or(DuckValue::Null)))
                .collect(),
        )
    }
    drop(cursor);
    let columns = stmt.column_names().iter().map(|x| x.to_string()).collect();
    Ok(QueryRun {
        columns,
        rows,
        truncated,
        row_limit: limit,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

struct Running {
    window: String,
    run_id: String,
    handle: Arc<duckdb::InterruptHandle>,
    cancelled: Arc<AtomicBool>,
}

fn running() -> &'static Mutex<Vec<Running>> {
    static RUNNING: OnceLock<Mutex<Vec<Running>>> = OnceLock::new();
    RUNNING.get_or_init(|| Mutex::new(vec![]))
}

/// Deregisters a run when it finishes, however it finishes.
pub(crate) struct RunGuard {
    run_id: String,
    pub(crate) cancelled: Arc<AtomicBool>,
}
impl RunGuard {
    pub(crate) fn register(window: &str, run_id: &str, connection: &duckdb::Connection) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        running().lock().unwrap().push(Running {
            window: window.into(),
            run_id: run_id.into(),
            handle: connection.interrupt_handle(),
            cancelled: cancelled.clone(),
        });
        Self {
            run_id: run_id.into(),
            cancelled,
        }
    }
}
impl Drop for RunGuard {
    fn drop(&mut self) {
        running()
            .lock()
            .unwrap()
            .retain(|r| r.run_id != self.run_id);
    }
}

/// Interrupts running queries for a window (one run, or all of them). Returns the
/// number of runs interrupted.
pub fn cancel(window: &str, run_id: Option<&str>) -> usize {
    let all = running().lock().unwrap();
    let mut count = 0;
    for r in all
        .iter()
        .filter(|r| r.window == window && run_id.is_none_or(|id| r.run_id == id))
    {
        r.cancelled.store(true, Ordering::SeqCst);
        r.handle.interrupt();
        count += 1;
    }
    count
}

/// Runs `sql` on `connection` with cancellation registered under `window`/`run_id`.
#[allow(clippy::too_many_arguments)]
pub fn run_on(
    connection: &duckdb::Connection,
    window: &str,
    run_id: Option<String>,
    sql: &str,
    declared: &[QueryParameter],
    supplied: &[NamedValue],
    declared_only: bool,
    limit: Option<u64>,
) -> Result<QueryRun, AppError> {
    let rewritten = prepare_sql(sql)?;
    let values = resolve_params(&rewritten.names, declared, supplied, declared_only)?;
    let limit = limit.unwrap_or(DEFAULT_ROW_LIMIT).clamp(1, MAX_ROW_LIMIT);
    let run_id = run_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let guard = RunGuard::register(window, &run_id, connection);
    execute(connection, &rewritten.sql, &values, limit).map_err(|e| {
        if guard.cancelled.load(Ordering::SeqCst) || e.to_ascii_lowercase().contains("interrupt") {
            AppError::new("CANCELLED", "Query cancelled")
        } else {
            AppError::new("DATABASE_ERROR", e)
        }
    })
}
