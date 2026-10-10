//! `ReadRuntime`: the session's DuckDB reader (PRD §10). Every app read —
//! object lists, table inspection, record pages, saved queries — runs here,
//! over the embedded SQLite file or an attached PostgreSQL database. Writes
//! never enter this connection.
use super::extensions::postgres_extension_path;
use super::files::{create_views, lock_down, FileView};
use super::logical::LogicalType;
use super::support::{logical_from_duckdb, redact};
use super::{
    ddl::{self, TableDef},
    duck_value, q, read_only_guard, DataValue, DbObject, QueryResult, TableSchema,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

/// What the reader's `data` catalog is attached to.
#[derive(Debug, Clone, PartialEq)]
pub enum ReadTarget {
    /// `<workspace>/data.db` through the bundled sqlite_scanner.
    Sqlite,
    /// A PostgreSQL database through the bundled postgres_scanner.
    Postgres {
        /// libpq connection string (contains the credential; never logged).
        conninfo: String,
        schema: String,
    },
}

/// Session-scoped DuckDB reader. Production callers pass the signed extension
/// copied from the application resources; no extension may be auto-installed.
///
/// App SQL never runs with external access: the database is opened with access
/// enabled only long enough to load the bundled extensions and attach the
/// datasource, then `enable_external_access=false` and `lock_configuration=true`
/// are set. The only file left reachable is `<workspace>/data.db` (via
/// `allowed_paths`), so a SQLite refresh re-attaches in place. A PostgreSQL
/// refresh clears postgres_scanner's catalog cache on the live attachment; a
/// datasource switch (or a failed attach) builds a fresh locked database,
/// because DuckDB refuses Postgres ATTACH once external access is off.
/// Bundled file sources are views in the `files` catalog; their extracted
/// files are added to `allowed_paths` and nothing else.
pub struct ReadRuntime {
    pub workspace: PathBuf,
    pub(super) connection: duckdb::Connection,
    pub(super) target: ReadTarget,
    sqlite_extension: PathBuf,
    /// Set when the configured datasource could not be attached; reads fail with this message (code CONNECTION) instead of silently reading the embedded file.
    attach_error: Option<String>,
    files: Vec<FileView>,
    /// File sources whose view could not be created (`name: error`).
    file_errors: Vec<String>,
}

impl ReadRuntime {
    pub fn new(workspace: &Path, sqlite_extension: &Path) -> Result<Self, String> {
        let (connection, attached, file_errors) =
            open_locked(workspace, sqlite_extension, &ReadTarget::Sqlite, &[])?;
        Ok(Self {
            workspace: workspace.to_owned(),
            connection,
            target: ReadTarget::Sqlite,
            sqlite_extension: sqlite_extension.to_owned(),
            attach_error: attached.err(),
            files: vec![],
            file_errors,
        })
    }
    pub fn target(&self) -> &ReadTarget {
        &self.target
    }
    pub fn attach_error(&self) -> Option<&str> {
        self.attach_error.as_deref()
    }
    /// Points the reader at another datasource. A failed attach is remembered (see `attach_error`) and also returned.
    pub fn set_target(&mut self, target: ReadTarget) -> Result<(), String> {
        let files = self.files.clone();
        self.configure(target, files)
    }
    /// Sets the datasource and the file views, rebuilding the database when either changed.
    pub fn configure(&mut self, target: ReadTarget, files: Vec<FileView>) -> Result<(), String> {
        if target == self.target && files == self.files && self.attach_error.is_none() {
            return Ok(());
        }
        self.target = target;
        self.files = files;
        self.rebuild()
    }
    pub fn file_errors(&self) -> &[String] {
        &self.file_errors
    }
    pub(super) fn schema_name(&self) -> &str {
        match &self.target {
            ReadTarget::Sqlite => "main",
            ReadTarget::Postgres { schema, .. } => schema,
        }
    }
    /// Replaces the DuckDB database with a freshly attached, locked one.
    fn rebuild(&mut self) -> Result<(), String> {
        let (connection, attached, file_errors) = open_locked(
            &self.workspace,
            &self.sqlite_extension,
            &self.target,
            &self.files,
        )?;
        self.connection = connection;
        self.attach_error = attached.as_ref().err().cloned();
        self.file_errors = file_errors;
        attached
    }
    /// Makes committed writes and DDL visible to the next read. SQLite re-attaches
    /// `data`. PostgreSQL keeps its attachment: postgres_scanner reads rows live in a
    /// new PostgreSQL transaction per DuckDB transaction, so only its catalog cache
    /// (tables, columns) can be stale, and `pg_clear_cache()` drops it. A failed clear
    /// or a failed earlier attach rebuilds the reader.
    pub fn refresh(&mut self) -> Result<(), String> {
        if self.attach_error.is_some() {
            return self.rebuild();
        }
        // Exclusive: connections cloned from this database (`read_connection`) share
        // the attached catalog, so none may run while `data` is re-attached, or load
        // postgres_scanner's catalog while it is cleared (a load racing the clear can
        // cache the pre-DDL catalog again).
        let _gate = super::gate::exclusive(&self.workspace.join("data.db"))?;
        if let ReadTarget::Postgres { .. } = self.target {
            return match self.connection.execute_batch("CALL pg_clear_cache()") {
                Ok(()) => Ok(()),
                Err(_) => self.rebuild(),
            };
        }
        let _ = self.connection.execute_batch("USE memory; DETACH data");
        let result = attach(&self.connection, &self.workspace, &self.target);
        self.attach_error = result.as_ref().err().cloned();
        result
    }
    pub fn connection(&self) -> &duckdb::Connection {
        &self.connection
    }
    /// The verified sqlite_scanner this reader loaded (the action-query writer loads it too).
    pub fn sqlite_extension(&self) -> &Path {
        &self.sqlite_extension
    }
    /// Shared access to the `data` catalog for one read (see `data::gate`): it
    /// excludes RecordStore writes to the embedded file and every `refresh`.
    pub fn read_gate(&self) -> Result<Option<super::gate::GateGuard>, String> {
        super::gate::shared(&self.workspace.join("data.db")).map(Some)
    }
    fn ready(&self) -> Result<(), String> {
        match &self.attach_error {
            Some(e) => Err(format!("Datasource unavailable: {e}")),
            None => Ok(()),
        }
    }
    /// The attached SQLite file's schema table, read through the sqlite scanner.
    fn sqlite_master(&self) -> String {
        format!(
            "sqlite_scan('{}', 'sqlite_master')",
            sql_path(&self.workspace)
        )
    }
    pub(super) fn from(&self, table: &str) -> String {
        format!("data.{}.{}", q(self.schema_name()), q(table))
    }

    pub fn objects(&self) -> Result<Vec<DbObject>, String> {
        self.ready()?;
        let _gate = self.read_gate()?;
        let mut stmt = self
            .connection
            .prepare(
                "SELECT table_name, CASE table_type WHEN 'VIEW' THEN 'view' ELSE 'table' END \
                 FROM information_schema.tables WHERE table_catalog='data' AND table_schema=? \
                 AND table_name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
                 AND table_name NOT LIKE '\\_ixtable\\_%' ESCAPE '\\' ORDER BY lower(table_name)",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([self.schema_name()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        let mut out = vec![];
        for row in rows {
            let (name, object_type) = row.map_err(|e| e.to_string())?;
            let row_count = self
                .connection
                .query_row(
                    &format!("SELECT count(*) FROM {}", self.from(&name)),
                    [],
                    |r| r.get::<_, u64>(0),
                )
                .ok();
            out.push(DbObject {
                name,
                object_type,
                row_count,
            })
        }
        Ok(out)
    }
    /// Counts rows of any table, including internal `_ixtable_` tables.
    pub fn row_count(&self, table: &str) -> Result<u64, String> {
        self.ready()?;
        let _gate = self.read_gate()?;
        self.connection
            .query_row(
                &format!("SELECT count(*) FROM {}", self.from(table)),
                [],
                |r| r.get::<_, u64>(0),
            )
            .map_err(|e| e.to_string())
    }
    pub fn query(&self, sql: &str) -> Result<QueryResult, String> {
        self.ready()?;
        let _gate = self.read_gate()?;
        let trimmed = read_only_guard(sql)?;
        let mut stmt = self
            .connection
            .prepare(trimmed)
            .map_err(|e| e.to_string())?;
        let mut cursor = stmt.query([]).map_err(|e| e.to_string())?;
        let mut rows = vec![];
        while let Some(row) = cursor.next().map_err(|e| e.to_string())? {
            let count = row.as_ref().column_count();
            rows.push(
                (0..count)
                    .map(|i| duck_value(row.get::<_, duckdb::types::Value>(i).unwrap()))
                    .collect(),
            )
        }
        drop(cursor);
        let columns = stmt.column_names().iter().map(|x| x.to_string()).collect();
        Ok(QueryResult { columns, rows })
    }
    fn table_exists(&self, table: &str) -> Result<Option<String>, String> {
        self.connection
            .query_row(
                "SELECT CASE table_type WHEN 'VIEW' THEN 'view' ELSE 'table' END FROM information_schema.tables \
                 WHERE table_catalog='data' AND table_schema=? AND table_name=?",
                [self.schema_name(), table],
                |r| r.get::<_, String>(0),
            )
            .map(Some)
            .or_else(|e| match e {
                duckdb::Error::QueryReturnedNoRows => Ok(None),
                e => Err(e.to_string()),
            })
    }
    /// The full definition (columns, keys, constraints, indexes) of a table or view.
    pub fn table_def(&self, table: &str) -> Result<TableDef, String> {
        self.ready()?;
        let _gate = self.read_gate()?;
        let Some(kind) = self.table_exists(table)? else {
            return Err(format!("Table or view {table:?} does not exist"));
        };
        let parsed = match &self.target {
            ReadTarget::Sqlite if kind == "table" => {
                let master = self.sqlite_master();
                let sql: Option<String> = self
                    .connection
                    .query_row(
                        &format!("SELECT sql FROM {master} WHERE type='table' AND name=?"),
                        [table],
                        |r| r.get(0),
                    )
                    .ok();
                let mut def = sql
                    .as_deref()
                    .map(ddl::parse_sqlite_create_table)
                    .transpose()?;
                if let Some(def) = def.as_mut() {
                    mark_rowid_alias(def);
                    let mut stmt = self
                        .connection
                        .prepare(&format!("SELECT sql FROM {master} WHERE type='index' AND tbl_name=? AND sql IS NOT NULL ORDER BY name"))
                        .map_err(|e| e.to_string())?;
                    let sqls = stmt
                        .query_map([table], |r| r.get::<_, String>(0))
                        .map_err(|e| e.to_string())?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| e.to_string())?;
                    for sql in sqls {
                        if let Ok(index) = ddl::parse_sqlite_create_index(&sql) {
                            def.indexes.push(index)
                        }
                    }
                }
                def
            }
            ReadTarget::Postgres { schema, .. } if kind == "table" => {
                let sql = crate::postgres::table_def_query(schema, table);
                let json: Option<String> = self
                    .connection
                    .query_row(
                        &format!(
                            "SELECT definition FROM postgres_query('data', '{}')",
                            sql.replace('\'', "''")
                        ),
                        [],
                        |r| r.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                let mut def = json
                    .map(|j| serde_json::from_str::<TableDef>(&j).map_err(|e| e.to_string()))
                    .transpose()?;
                for c in def.iter_mut().flat_map(|d| d.columns.iter_mut()) {
                    c.logical_type = LogicalType::from_postgres(&c.declared_type);
                }
                def
            }
            _ => None,
        };
        match parsed {
            Some(def) => Ok(def),
            None => self.def_from_information_schema(table),
        }
    }
    /// Views (and tables whose DDL cannot be read) are described from DuckDB's information schema; logical types come from DuckDB's column types.
    fn def_from_information_schema(&self, table: &str) -> Result<TableDef, String> {
        let mut stmt = self
            .connection
            .prepare(
                "SELECT column_name, data_type, is_nullable, column_default FROM information_schema.columns \
                 WHERE table_catalog='data' AND table_schema=? AND table_name=? ORDER BY ordinal_position",
            )
            .map_err(|e| e.to_string())?;
        let columns = stmt
            .query_map([self.schema_name(), table], |r| {
                let data_type: String = r.get(1)?;
                Ok(ddl::ColumnDef {
                    name: r.get(0)?,
                    logical_type: logical_from_duckdb(&data_type),
                    declared_type: data_type,
                    nullable: r.get::<_, String>(2)? == "YES",
                    default_expression: r.get(3)?,
                    generated_expression: None,
                    identity: false,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let mut pk = self
            .connection
            .prepare("SELECT k.column_name FROM information_schema.table_constraints t JOIN information_schema.key_column_usage k USING (constraint_catalog,constraint_schema,constraint_name) WHERE t.table_catalog='data' AND t.table_schema=? AND t.table_name=? AND t.constraint_type='PRIMARY KEY' ORDER BY k.ordinal_position")
            .map_err(|e| e.to_string())?;
        let primary_key = pk
            .query_map([self.schema_name(), table], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(TableDef {
            name: table.into(),
            columns,
            primary_key,
            ..Default::default()
        })
    }
    /// Columns DuckDB can read (SQLite generated columns are not scanned).
    pub(super) fn duckdb_columns(&self, table: &str) -> Result<HashSet<String>, String> {
        let _gate = self.read_gate()?;
        let mut stmt = self
            .connection
            .prepare("SELECT column_name FROM information_schema.columns WHERE table_catalog='data' AND table_schema=? AND table_name=?")
            .map_err(|e| e.to_string())?;
        let names = stmt
            .query_map([self.schema_name(), table], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<HashSet<_>, _>>()
            .map_err(|e| e.to_string());
        names
    }
    pub fn schema(&self, table: &str) -> Result<TableSchema, String> {
        let _gate = self.read_gate()?;
        let def = self.table_def(table)?;
        let object_type = self.table_exists(table)?.unwrap_or_else(|| "table".into());
        Ok(TableSchema::from_def(def, object_type))
    }
    #[cfg(test)]
    pub(crate) fn isolated_for_test() -> Result<Self, String> {
        let config = duckdb::Config::default()
            .enable_autoload_extension(false)
            .map_err(|e| e.to_string())?;
        let connection =
            duckdb::Connection::open_in_memory_with_flags(config).map_err(|e| e.to_string())?;
        Ok(Self {
            workspace: PathBuf::new(),
            connection,
            target: ReadTarget::Sqlite,
            sqlite_extension: PathBuf::new(),
            attach_error: None,
            files: vec![],
            file_errors: vec![],
        })
    }
    /// A reader over `<workspace>/data.db` using the bundled extension for this platform (or `IXTABLE_DUCKDB_SQLITE_EXTENSION`).
    #[cfg(test)]
    pub(crate) fn for_test(workspace: &Path) -> Result<Self, String> {
        // Unverified dev copy first: hashing it per reader would slow every test.
        let dev = super::extensions::resource_dirs()[0].join("sqlite_scanner.duckdb_extension");
        let ext = std::env::var_os("IXTABLE_DUCKDB_SQLITE_EXTENSION")
            .map(PathBuf::from)
            .or_else(|| dev.is_file().then(|| dev.clone()))
            .unwrap_or_else(|| super::extensions::sqlite_extension_path().unwrap_or(dev));
        Self::new(workspace, &ext)
    }
}

/// Opens an in-memory DuckDB database, loads the extensions `target` needs,
/// attaches the datasource as `data`, and creates the file views. External
/// access is then disabled and the configuration locked whether or not the
/// attach succeeded; the attach result and file view errors are returned
/// separately so a failed datasource still yields a safe connection.
type Opened = (duckdb::Connection, Result<(), String>, Vec<String>);
fn open_locked(
    workspace: &Path,
    sqlite_extension: &Path,
    target: &ReadTarget,
    files: &[FileView],
) -> Result<Opened, String> {
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
    let _gate = match target {
        ReadTarget::Sqlite => Some(super::gate::shared(&workspace.join("data.db"))?),
        ReadTarget::Postgres { .. } => None,
    };
    let attached = match target {
        ReadTarget::Sqlite => Ok(()),
        ReadTarget::Postgres { .. } => {
            postgres_extension_path().and_then(|p| load(&p, "PostgreSQL"))
        }
    }
    .and_then(|()| attach(&connection, workspace, target));
    let file_errors = create_views(&connection, files);
    let db = workspace.join("data.db");
    let mut allowed: Vec<&Path> = vec![&db];
    allowed.extend(files.iter().map(|f| f.path.as_path()));
    lock_down(&connection, &allowed)?;
    Ok((connection, attached, file_errors))
}

/// Flags SQLite's `INTEGER PRIMARY KEY` (a rowid alias) as filled in by the database.
pub(crate) fn mark_rowid_alias(def: &mut TableDef) {
    if def.without_rowid || def.primary_key.len() != 1 {
        return;
    }
    let key = def.primary_key[0].clone();
    for c in &mut def.columns {
        if c.name.eq_ignore_ascii_case(&key) && c.declared_type.eq_ignore_ascii_case("INTEGER") {
            c.identity = true;
        }
    }
}

/// `<workspace>/data.db`, quoted for a SQL string literal.
pub(super) fn sql_path(workspace: &Path) -> String {
    workspace
        .join("data.db")
        .to_string_lossy()
        .replace('\'', "''")
}

fn attach(
    connection: &duckdb::Connection,
    workspace: &Path,
    target: &ReadTarget,
) -> Result<(), String> {
    match target {
        ReadTarget::Sqlite => connection
            .execute_batch(&format!(
                "ATTACH '{}' AS data (TYPE SQLITE, READ_ONLY); USE data",
                sql_path(workspace)
            ))
            .map_err(|e| format!("SQLite attachment: {e}")),
        ReadTarget::Postgres { conninfo, schema } => connection
            .execute_batch(&format!(
                "ATTACH '{}' AS data (TYPE POSTGRES, READ_ONLY, SCHEMA '{}'); USE data.{}",
                conninfo.replace('\'', "''"),
                schema.replace('\'', "''"),
                q(schema)
            ))
            .map_err(|e| format!("PostgreSQL connection failed: {}", redact(&e.to_string()))),
    }
}

pub(crate) fn duck_bind(value: &DataValue) -> Result<duckdb::types::Value, String> {
    use duckdb::types::Value as V;
    Ok(match value {
        DataValue::Null => V::Null,
        DataValue::Integer(v) => V::BigInt(*v),
        DataValue::Real(v) if v.is_finite() => V::Double(*v),
        DataValue::Real(_) => return Err("Real values must be finite".into()),
        DataValue::Boolean(v) => V::Boolean(*v),
        DataValue::Blob(v) => V::Blob(
            STANDARD
                .decode(v)
                .map_err(|_| "Blob values must be valid base64")?,
        ),
        DataValue::Text(v)
        | DataValue::Date(v)
        | DataValue::Timestamp(v)
        | DataValue::Decimal(v)
        | DataValue::Time(v) => V::Text(v.clone()),
    })
}
