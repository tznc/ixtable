//! Runs the saved queries a dashboard uses (the blocking half of `useDashboardData.ts`).

use crate::data::NamedValue;
use crate::forms::values::to_data_value;
use crate::manager::AppError;
use crate::queries::QueryRun;
use serde_json::{Map, Value};

/// Runs a saved query on the calling thread: `run_saved_query` without the async wrapper,
/// so `cancel_query` can interrupt it by `run_id`. Missing values fall back to defaults.
pub fn run_saved(label: &str, id: &str, params: &Map<String, Value>, limit: Option<u64>, run_id: &str) -> Result<QueryRun, AppError> {
    crate::authz::check(label, "query", id, crate::authz::Op::Read)?;
    let manager = crate::manager()?;
    let config = manager.config(label)?;
    let query = crate::queries::find_saved(&config, id)?;
    let connection = manager.read_connection(label)?;
    let supplied: Vec<NamedValue> =
        params.iter().map(|(name, value)| NamedValue { column: name.clone(), value: to_data_value(value) }).collect();
    crate::queries::run_on(&connection, label, Some(run_id.to_string()), &query.sql, &query.parameters, &supplied, true, limit)
}

/// Interrupts the run `run_id`; true when something was running.
pub fn cancel(label: &str, run_id: &str) -> bool {
    crate::queries::cancel_query(label.to_string(), Some(run_id.to_string())).is_ok_and(|n| n > 0)
}
