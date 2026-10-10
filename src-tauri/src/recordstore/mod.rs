//! RecordStore (PRD §9.4, §19): the write side of the read/write split.
//! DuckDB (`data::ReadRuntime`) reads; a `RecordStore` writes to SQLite or
//! PostgreSQL directly. Both stores publish `StoreCapabilities` and pass the
//! same conformance suite (`recordstore/conformance.rs`).
pub mod capabilities;
pub mod commands;
#[cfg(test)]
pub(crate) mod conformance;
#[cfg(test)]
mod conformance_more;
#[cfg(test)]
mod fields_tests;
pub mod login;
pub mod model;
pub mod plan;
#[cfg(test)]
mod plan_tests;
pub mod runtime_login;
pub mod secrets;
pub mod sqlite;
pub mod sqlite_ddl;
pub mod sqlite_errors;
pub mod sqlite_store;
#[cfg(test)]
mod sqlite_tests;

pub use capabilities::StoreCapabilities;
pub use commands::*;
pub use model::*;

use crate::archive::{check_named_ids, DocumentConfig, Issue};
use crate::data::{
    AlterTable, CreateTable, DataValue, IndexDef, NamedValue, ReadRuntime, ReadTarget, TableDef,
};
use crate::manager::AppError;
use std::path::Path;

/// Script execution report for migrations (one transaction per script).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScriptReport {
    pub log: Vec<String>,
    pub health: Vec<String>,
}

/// Bookkeeping statement (SQL with `?` placeholders, text binds).
pub type Bookkeeping = (String, Vec<String>);

/// Bookkeeping bind replaced by the time the script finished, inside its transaction.
pub const BIND_FINISHED_AT: &str = "\u{0}ixtable:finished_at";
/// Bookkeeping bind replaced by the script's health lines, inside its transaction.
pub const BIND_HEALTH: &str = "\u{0}ixtable:health";

/// Binds with the finish-time and health placeholders filled in.
pub fn resolve_binds(binds: &[String], health: &[String]) -> Vec<String> {
    let finished = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    binds
        .iter()
        .map(|b| match b.as_str() {
            BIND_FINISHED_AT => finished.clone(),
            BIND_HEALTH => health.join("\n"),
            _ => b.clone(),
        })
        .collect()
}

pub trait RecordStore {
    fn kind(&self) -> &'static str;
    fn capabilities(&self) -> StoreCapabilities;
    fn table_names(&mut self) -> Result<Vec<String>, StoreError>;
    fn table_def(&mut self, table: &str) -> Result<TableDef, StoreError>;
    fn insert(&mut self, table: &str, values: &[NamedValue]) -> Result<Vec<DataValue>, StoreError>;
    /// `expected` holds original values; a mismatch fails with CONFLICT.
    fn update(
        &mut self,
        table: &str,
        values: &[NamedValue],
        identity: &[DataValue],
        expected: Option<&[NamedValue]>,
    ) -> Result<u64, StoreError>;
    fn delete(
        &mut self,
        table: &str,
        identity: &[DataValue],
        expected: Option<&[NamedValue]>,
    ) -> Result<u64, StoreError>;
    /// Runs every operation in ONE transaction.
    fn execute_batch(&mut self, ops: &[WriteOp]) -> Result<Vec<WriteOutcome>, StoreError>;
    fn create_table(&mut self, spec: &CreateTable) -> Result<Vec<String>, StoreError>;
    fn plan_alter(&mut self, table: &str, ops: &[AlterTable]) -> Result<ChangePlan, StoreError>;
    fn alter_table(&mut self, table: &str, ops: &[AlterTable]) -> Result<ChangePlan, StoreError>;
    fn drop_table(&mut self, table: &str) -> Result<(), StoreError>;
    fn create_index(&mut self, spec: &CreateIndex) -> Result<(), StoreError>;
    fn drop_index(&mut self, name: &str) -> Result<(), StoreError>;
    fn list_indexes(&mut self, table: Option<&str>) -> Result<Vec<IndexDef>, StoreError>;
    /// Rows, inbound foreign keys, and indexes of a table (config dependents are added by callers).
    fn impact(&mut self, table: &str) -> Result<TableImpact, StoreError>;
    /// Runs a script, health checks, then `record` bookkeeping (`?` binds) in ONE transaction; a dry run rolls back.
    fn run_script(
        &mut self,
        sql: &str,
        record: &[Bookkeeping],
        dry_run: bool,
    ) -> Result<ScriptReport, StoreError>;
    /// Executes internal bookkeeping SQL in its own transaction.
    fn execute_internal(&mut self, sql: &str, binds: &[String]) -> Result<(), StoreError>;
    /// Reads internal bookkeeping rows as text.
    fn query_internal(&mut self, sql: &str) -> Result<Vec<Vec<Option<String>>>, StoreError>;
    /// After explicit-key inserts, moves `table`'s key sequences past its largest key (a no-op for SQLite).
    fn sync_identity(&mut self, _table: &str) -> Result<(), StoreError> {
        Ok(())
    }
}

/// Opens the store configured for a document.
pub fn for_config(
    datasource: &DatasourceConfig,
    db_path: &Path,
) -> Result<Box<dyn RecordStore>, StoreError> {
    if datasource.is_postgres() {
        secrets::ensure_transport(datasource)?;
        let (ds, password) = secrets::connection(datasource)?;
        Ok(Box::new(crate::postgres::PostgresRecordStore::connect(
            &ds,
            password.as_deref(),
        )?))
    } else {
        Ok(Box::new(sqlite::SqliteRecordStore::new(db_path)))
    }
}

/// Runs store work on a plain thread. The PostgreSQL client drives its own
/// runtime and cannot block inside an async command executor.
pub fn blocking<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        s.spawn(f)
            .join()
            .unwrap_or_else(|e| std::panic::resume_unwind(e))
    })
}

/// Opens the session's store and runs `f` with it off the async executor.
pub fn with_store<T: Send>(
    window: &str,
    f: impl FnOnce(&mut dyn RecordStore) -> Result<T, StoreError> + Send,
) -> Result<T, AppError> {
    blocking(move || {
        let mut store = for_session(window)?;
        f(store.as_mut()).map_err(AppError::from)
    })
}

/// The RecordStore of a window's session.
pub fn for_session(window: &str) -> Result<Box<dyn RecordStore>, AppError> {
    let m = crate::manager()?;
    let config = m.config(window)?;
    let path = m.database_path(window)?;
    Ok(for_config(&config.datasource, &path)?)
}

/// What the DuckDB reader should attach for a datasource.
pub fn read_target(datasource: &DatasourceConfig) -> Result<ReadTarget, String> {
    if !datasource.is_postgres() {
        return Ok(ReadTarget::Sqlite);
    }
    secrets::ensure_transport(datasource).map_err(|e| e.message)?;
    let (ds, password) = secrets::connection(datasource).map_err(|e| e.message)?;
    Ok(ReadTarget::Postgres {
        conninfo: crate::postgres::conninfo(&ds, password.as_deref()),
        schema: datasource.schema.clone(),
    })
}

/// Points a session reader at the configured datasource. Failures are kept
/// on the reader (reads then fail with CONNECTION) so a document with an
/// unreachable database still opens.
pub fn attach_configured(reader: &mut ReadRuntime, config: &DocumentConfig) {
    let target = read_target(&config.datasource);
    let files = crate::import::sources::views(&reader.workspace, &config.file_sources);
    let result = match target {
        Ok(t) => reader.configure(t, files),
        Err(e) => Err(e),
    };
    if let Err(e) = result {
        crate::logging::warn("recordstore", &format!("datasource not attached: {e}"));
    }
}

/// The resolved concurrency policy for a table, if any.
pub fn entity_policy<'a>(config: &'a DocumentConfig, table: &str) -> Option<&'a EntitySettings> {
    config.entities.iter().find(|e| e.table == table)
}

pub fn validate(config: &DocumentConfig) -> Vec<Issue> {
    let mut issues = check_named_ids(
        "entity",
        config
            .entities
            .iter()
            .map(|e| (e.id.as_str(), e.table.as_str())),
    );
    let ds = &config.datasource;
    match ds.kind.as_str() {
        "sqlite" => {}
        "postgres" => {
            for (field, value) in [
                ("host", &ds.host),
                ("database", &ds.database),
                ("user", &ds.user),
            ] {
                if value.trim().is_empty() {
                    issues.push(Issue::error(
                        "datasource",
                        "datasource",
                        format!("PostgreSQL datasource needs a {field}"),
                    ));
                }
            }
            if !matches!(
                ds.sslmode.as_str(),
                "disable" | "allow" | "prefer" | "require" | "verify-ca" | "verify-full"
            ) {
                issues.push(Issue::error(
                    "datasource",
                    "datasource",
                    format!("unknown sslmode {}", ds.sslmode),
                ));
            }
            if ds.allows_plaintext() && !ds.insecure_transport_confirmed {
                issues.push(Issue::error(
                    "datasource",
                    "datasource",
                    "The datasource allows connections without TLS; confirm the security override in Datasource settings",
                ));
            } else if ds.allows_plaintext() {
                issues.push(Issue::warning(
                    "datasource",
                    "datasource",
                    "Security override: credentials and records may travel without TLS",
                ));
            }
            if ds.credential_mode == "shared" {
                issues.push(Issue::warning(
                    "datasource",
                    "datasource",
                    "Shared credentials reduce revocation and database-level attribution",
                ));
            } else if ds.credential_mode != "perUser" {
                issues.push(Issue::error(
                    "datasource",
                    "datasource",
                    format!("unknown credential mode {}", ds.credential_mode),
                ));
            }
        }
        other => issues.push(Issue::error(
            "datasource",
            "datasource",
            format!("unknown datasource kind {other}"),
        )),
    }
    for e in &config.entities {
        if !POLICIES.contains(&e.concurrency.as_str()) {
            issues.push(Issue::warning(
                "entity",
                &e.id,
                format!("{} has no resolved concurrency policy", e.table),
            ));
        } else if e.concurrency == "customAction"
            && !e
                .action_id
                .as_deref()
                .is_some_and(|a| config.actions.iter().any(|x| x.id == a))
        {
            issues.push(Issue::error(
                "entity",
                &e.id,
                format!(
                    "{} uses a custom transactional action that does not exist",
                    e.table
                ),
            ));
        }
    }
    for e in &config.entities {
        field_issues(e, &mut issues);
    }
    issues
}

/// Field settings: one per column, a known format, and choices for a multi-select.
fn field_issues(entity: &EntitySettings, issues: &mut Vec<Issue>) {
    let mut seen = std::collections::HashSet::new();
    for f in &entity.fields {
        let name = format!("{}.{}", entity.table, f.column);
        if f.column.is_empty() || !seen.insert(f.column.as_str()) {
            issues.push(Issue::error(
                "entity",
                &entity.id,
                format!("{name} has more than one field setting"),
            ));
        }
        match f.format.as_deref() {
            Some(format) if !FIELD_FORMATS.contains(&format) => issues.push(Issue::warning(
                "entity",
                &entity.id,
                format!("{name} uses an unknown field format {format}"),
            )),
            Some("multiSelect") if f.options.is_empty() => issues.push(Issue::warning(
                "entity",
                &entity.id,
                format!("{name} is a multi-select field with no choices"),
            )),
            _ => {}
        }
    }
}

/// Session-aware checks: tables in the store without a concurrency policy.
pub fn table_issues(window: &str, config: &DocumentConfig) -> Vec<Issue> {
    let tables: Vec<String> = crate::manager()
        .and_then(|m| m.database_objects(window))
        .map(|objects| {
            objects
                .into_iter()
                .filter(|o| o.object_type == "table")
                .map(|o| o.name)
                .collect()
        })
        .unwrap_or_default();
    validate_tables(config, &tables)
}

/// Warns about tables that have no concurrency policy (PRD §19).
pub fn validate_tables(config: &DocumentConfig, tables: &[String]) -> Vec<Issue> {
    tables
        .iter()
        .filter(|t| entity_policy(config, t).is_none())
        .map(|t| {
            Issue::warning(
                "entity",
                t,
                format!(
                    "Table {t} has no resolved concurrency policy; set one in Entities settings"
                ),
            )
        })
        .collect()
}
