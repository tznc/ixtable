//! Saved and ad hoc read queries (PRD §12).
//!
//! Every query runs through DuckDB behind the same read-only guard as
//! `ReadRuntime::query`. `$name` placeholders are rewritten to DuckDB positional
//! parameters (`$1`, `$2`, ...) and bound as typed values; parameter values are
//! never interpolated into SQL text. Each run uses its own connection to the
//! session's DuckDB database so a long query does not hold the session lock, and
//! `cancel_query` interrupts it through a DuckDB `InterruptHandle`.
//! Command argument types are written as crate paths: the test bridge copies them
//! to the crate root.
use crate::archive::{DocumentConfig, QueryParameter, SavedQuery};
use crate::data::{self, DataValue, NamedValue};
use crate::manager::AppError;
use crate::validation::Issue;
use base64::{engine::general_purpose::STANDARD, Engine};
use duckdb::types::{TimeUnit, Value as DuckValue};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::Instant;

/// Rows returned when the caller passes no limit.
pub const DEFAULT_ROW_LIMIT: u64 = 10_000;
/// Hard cap on rows returned by one run.
pub const MAX_ROW_LIMIT: u64 = 100_000;

/// Result of one query run: rows up to the limit, plus whether more existed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryRun {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<DataValue>>,
    pub truncated: bool,
    pub row_limit: u64,
    pub elapsed_ms: u64,
}

/// SQL with `$name` placeholders rewritten to `$1..$n`; `names[i]` binds `$i+1`.
#[derive(Debug, Clone, PartialEq)]
pub struct Rewritten {
    pub sql: String,
    pub names: Vec<String>,
}

fn ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}
fn ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Rewrites `$name` placeholders outside strings, quoted identifiers, comments and
/// dollar-quoted strings. Rejects `?` and `$1` placeholders: saved queries name
/// their parameters.
pub fn rewrite_placeholders(sql: &str) -> Result<Rewritten, String> {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut names: Vec<String> = vec![];
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            '\'' | '"' => {
                // E'...' strings treat backslash as an escape.
                let escapes = c == '\''
                    && i > 0
                    && matches!(chars[i - 1], 'e' | 'E')
                    && (i < 2 || !ident_char(chars[i - 2]));
                out.push(c);
                i += 1;
                while i < chars.len() {
                    let d = chars[i];
                    out.push(d);
                    i += 1;
                    if escapes && d == '\\' {
                        if let Some(&e) = chars.get(i) {
                            out.push(e);
                            i += 1;
                        }
                    } else if d == c {
                        if chars.get(i) == Some(&c) {
                            out.push(c);
                            i += 1;
                        } else {
                            break;
                        }
                    }
                }
            }
            '-' if next == Some('-') => {
                while i < chars.len() && chars[i] != '\n' {
                    out.push(chars[i]);
                    i += 1;
                }
            }
            '/' if next == Some('*') => {
                out.push_str("/*");
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    out.push(chars[i]);
                    i += 1;
                }
                if i < chars.len() {
                    out.push_str("*/");
                    i += 2;
                }
            }
            '?' => return Err("Use named parameters like $customer instead of ?".into()),
            '$' => {
                let mut j = i + 1;
                while j < chars.len() && ident_char(chars[j]) {
                    j += 1;
                }
                let word: String = chars[i + 1..j].iter().collect();
                if chars.get(j) == Some(&'$') && word.chars().all(ident_char) {
                    // Dollar-quoted string: $tag$ ... $tag$
                    let tag: String = chars[i..=j].iter().collect();
                    let rest: String = chars[j + 1..].iter().collect();
                    let end = rest
                        .find(&tag)
                        .map(|p| j + 1 + rest[..p].chars().count() + tag.chars().count())
                        .unwrap_or(chars.len());
                    out.extend(&chars[i..end]);
                    i = end;
                } else if word.is_empty() {
                    out.push('$');
                    i += 1;
                } else if !ident_start(chars[i + 1]) {
                    return Err(format!(
                        "Use named parameters like $customer instead of ${word}"
                    ));
                } else {
                    let index = match names.iter().position(|n| n == &word) {
                        Some(p) => p + 1,
                        None => {
                            names.push(word);
                            names.len()
                        }
                    };
                    out.push_str(&format!("${index}"));
                    i = j;
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(Rewritten { sql: out, names })
}

/// The parameter names a query references, in first-use order.
pub fn placeholder_names(sql: &str) -> Result<Vec<String>, String> {
    rewrite_placeholders(sql).map(|r| r.names)
}

const MUTATING: [&str; 11] = [
    "INSERT", "UPDATE", "DELETE", "MERGE", "UPSERT", "REPLACE", "CREATE", "DROP", "ALTER",
    "TRUNCATE", "COPY",
];

/// Rewrites placeholders and applies the shared read-only guard. Mutating SQL gets
/// a message that points to migrations and actions (PRD §12).
pub fn prepare_sql(sql: &str) -> Result<Rewritten, AppError> {
    let mut rewritten =
        rewrite_placeholders(sql).map_err(|e| AppError::new("VALIDATION_ERROR", e))?;
    if let Err(message) = data::read_only_guard(&rewritten.sql) {
        let upper = data::sqltext::mask(&rewritten.sql).to_ascii_uppercase();
        let found = upper
            .split(|c: char| !ident_char(c))
            .find(|t| MUTATING.contains(t));
        let message = match found {
            Some(word) => format!(
                "Read queries are read-only and cannot run {word}. Change the schema with a migration, or make this an action query to change rows."
            ),
            None => message,
        };
        return Err(AppError::new("READ_ONLY", message));
    }
    if let Ok(statement) = data::read_only_guard(&rewritten.sql) {
        rewritten.sql = statement.to_string();
    }
    Ok(rewritten)
}

pub mod action;
pub(crate) mod action_exec;
#[cfg(test)]
pub(crate) mod action_tests;
mod page;
mod params;
mod run;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_page;
mod validate;
pub use page::*;
pub use params::*;
pub use run::*;
pub use validate::validate;

/// A saved read query by id; action queries run only through `run_action_query`.
fn find_saved<'a>(config: &'a DocumentConfig, id: &str) -> Result<&'a SavedQuery, AppError> {
    let query = config
        .saved_queries
        .iter()
        .find(|q| q.id == id)
        .ok_or_else(|| AppError::new("NOT_FOUND", format!("Saved query {id} not found")))?;
    if query.action.is_some() {
        return Err(AppError::new(
            "VALIDATION_ERROR",
            format!(
                "\"{}\" is an action query: it changes rows and returns none",
                query.name
            ),
        ));
    }
    Ok(query)
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::new("DATABASE_ERROR", e))?
}

/// Runs read-only SQL with `$name` parameters. `parameters` optionally declares
/// types, defaults, and required flags (as a saved query would).
#[tauri::command]
pub async fn execute_parameterized_query(
    window_label: String,
    sql: String,
    params: Vec<crate::data::NamedValue>,
    parameters: Option<Vec<crate::archive::QueryParameter>>,
    limit: Option<u64>,
    run_id: Option<String>,
) -> Result<QueryRun, AppError> {
    blocking(move || {
        crate::authz::require_unrestricted(&window_label, "run ad hoc SQL")?;
        let connection = crate::manager()?.read_connection(&window_label)?;
        let declared = parameters.unwrap_or_default();
        run_on(
            &connection,
            &window_label,
            run_id,
            &sql,
            &declared,
            &params,
            false,
            limit,
        )
    })
    .await
}

/// Runs a saved query by id; missing values fall back to parameter defaults.
#[tauri::command]
pub async fn run_saved_query(
    window_label: String,
    id: String,
    params: Vec<crate::data::NamedValue>,
    limit: Option<u64>,
    run_id: Option<String>,
) -> Result<QueryRun, AppError> {
    blocking(move || {
        crate::authz::check(&window_label, "query", &id, crate::authz::Op::Read)?;
        let manager = crate::manager()?;
        let config = manager.config(&window_label)?;
        let query = find_saved(&config, &id)?;
        let connection = manager.read_connection(&window_label)?;
        run_on(
            &connection,
            &window_label,
            run_id,
            &query.sql,
            &query.parameters,
            &params,
            true,
            limit,
        )
    })
    .await
}

/// Reads one page of a saved query: filters, sorts, LIMIT/OFFSET and the total
/// count run in DuckDB over the query as a subquery (see `page_on`).
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn run_saved_query_page(
    window_label: String,
    id: String,
    params: Vec<crate::data::NamedValue>,
    offset: u64,
    limit: u64,
    sorts: Vec<crate::data::Sort>,
    filters: Vec<crate::data::Filter>,
    run_id: Option<String>,
) -> Result<QueryPage, AppError> {
    blocking(move || {
        crate::authz::check(&window_label, "query", &id, crate::authz::Op::Read)?;
        let manager = crate::manager()?;
        let config = manager.config(&window_label)?;
        let query = find_saved(&config, &id)?;
        let connection = manager.read_connection(&window_label)?;
        let request = PageSpec {
            offset,
            limit,
            sorts: &sorts,
            filters: &filters,
        };
        page_on(
            &connection,
            &window_label,
            run_id,
            &query.sql,
            &query.parameters,
            &params,
            &request,
        )
    })
    .await
}

/// Checks that `sql` is one read-only statement DuckDB can prepare (tables and
/// columns exist), without running it. Returns the referenced parameter names.
pub fn check_on(connection: &duckdb::Connection, sql: &str) -> Result<Vec<String>, AppError> {
    let rewritten = prepare_sql(sql)?;
    connection
        .prepare(&rewritten.sql)
        .map_err(|e| AppError::new("DATABASE_ERROR", e.to_string()))?;
    Ok(rewritten.names)
}

/// Validates query SQL before it is saved (see `check_on`).
#[tauri::command]
pub async fn check_query_sql(window_label: String, sql: String) -> Result<Vec<String>, AppError> {
    blocking(move || check_on(&*crate::manager()?.read_connection(&window_label)?, &sql)).await
}

/// Interrupts a running query (`run_id`) or every running query of the window.
#[tauri::command]
pub fn cancel_query(window_label: String, run_id: Option<String>) -> Result<usize, AppError> {
    Ok(cancel(&window_label, run_id.as_deref()))
}
