//! The action-query writer (`docs/decisions/action-queries.md`): a short-lived
//! DuckDB database with the datasource attached read-write. It loads only the
//! bundled extensions, then locks down like the reader (`enable_external_access`
//! off, `lock_configuration` on), so the statement it runs can reach the
//! datasource and nothing else. For the embedded SQLite file the caller holds
//! the exclusive `gate` from before the attach until the connection is dropped:
//! the scanner's copy of SQLite is one more writer the gate must serialize.
use super::extensions::postgres_extension_path;
use super::files::lock_down;
use super::q;
use super::read::{sql_path, ReadTarget};
use super::support::redact;
use std::path::Path;

/// DuckDB evaluates a SQLite column's DEFAULT when it inserts a row. The
/// `CURRENT_DATE` and `CURRENT_TIME` defaults need DuckDB's ICU extension,
/// which ixtable does not bundle; these macros give the same UTC values
/// SQLite would (`CURRENT_TIMESTAMP` needs nothing).
const SQLITE_DEFAULTS: &str =
    "CREATE TEMP MACRO current_date() AS CAST(CAST(now() AS TIMESTAMP) AS DATE); \
     CREATE TEMP MACRO get_current_time() AS CAST(CAST(now() AS TIMESTAMP) AS TIME);";

/// The file's journal mode, passed back unchanged when the writer attaches it.
fn journal_mode(db: &Path) -> Result<String, String> {
    let c = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("SQLite attachment: {e}"))?;
    let mode: String = c
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .map_err(|e| format!("SQLite attachment: {e}"))?;
    if !mode.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(format!("SQLite attachment: unexpected journal mode {mode}"));
    }
    Ok(mode)
}

/// SQLite enforces foreign keys (and runs `ON DELETE` actions) only when
/// `PRAGMA foreign_keys` is on for the connection. rusqlite's SQLite turns it
/// on by default, and on Linux the scanner binds to that copy, but on macOS
/// and Windows it uses its own, where it is off. The scanner has no option for
/// it: its `JOURNAL_MODE` option is run as `PRAGMA journal_mode=<value>`
/// through `sqlite3_exec`, so the writer appends the pragma there. The
/// extension is pinned by hash, and `queries::action_tests` checks enforcement
/// on every platform CI builds.
const FOREIGN_KEYS: &str = "PRAGMA foreign_keys=ON";

/// `text` attaches the embedded file with every column as VARCHAR
/// (`sqlite_all_varchar`): the scanner can then UPDATE date and timestamp
/// columns, which it cannot bind typed, and SQLite's column affinity stores
/// the text as the column's type.
pub fn open_writer(
    workspace: &Path,
    sqlite_extension: &Path,
    target: &ReadTarget,
    text: bool,
) -> Result<duckdb::Connection, String> {
    let config = duckdb::Config::default()
        .enable_autoload_extension(false)
        .map_err(|e| e.to_string())?
        .enable_external_access(true)
        .map_err(|e| e.to_string())?;
    let connection = duckdb::Connection::open_in_memory_with_flags(config)
        .map_err(|e| format!("DuckDB startup: {e}"))?;
    let load = |path: &Path, what: &str| {
        let extension = path.to_string_lossy().replace('\'', "''");
        connection
            .execute_batch(&format!("LOAD '{extension}'"))
            .map_err(|e| format!("{what} extension startup: {e}"))
    };
    load(sqlite_extension, "SQLite")?;
    if text {
        connection
            .execute_batch("SET sqlite_all_varchar = true")
            .map_err(|e| format!("SQLite attachment: {e}"))?;
    }
    match target {
        ReadTarget::Sqlite => {
            let mode = journal_mode(&workspace.join("data.db"))?;
            connection
                .execute_batch(&format!(
                    "ATTACH '{}' AS data (TYPE SQLITE, JOURNAL_MODE '{mode}; {FOREIGN_KEYS}'); \
                     {SQLITE_DEFAULTS} USE data",
                    sql_path(workspace)
                ))
                .map_err(|e| format!("SQLite attachment: {e}"))?
        }
        ReadTarget::Postgres { conninfo, schema } => {
            load(&postgres_extension_path()?, "PostgreSQL")?;
            connection
                .execute_batch(&format!(
                    "ATTACH '{}' AS data (TYPE POSTGRES, SCHEMA '{}'); USE data.{}",
                    conninfo.replace('\'', "''"),
                    schema.replace('\'', "''"),
                    q(schema)
                ))
                .map_err(|e| format!("PostgreSQL connection failed: {}", redact(&e.to_string())))?
        }
    }
    lock_down(&connection, &[&workspace.join("data.db")])?;
    Ok(connection)
}
