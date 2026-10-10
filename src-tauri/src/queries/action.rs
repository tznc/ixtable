//! Action queries (`docs/decisions/action-queries.md`): saved queries that
//! change rows. The statement is DuckDB SQL, run by a writer connection with
//! the datasource attached read-write (`data::write`), in one transaction.
//!
//! A run checks the role's table permission and the user-mode trigger
//! precheck, like a record write. When the target table has enabled `created`
//! or `updated` triggers, the run copies the table before the statement and
//! compares it after, by primary key, so the caller can fire those triggers
//! once per changed row. The run returns the created identities, the updated
//! identities with their old values, and one sync-trigger grant per event.
use super::{resolve_params, rewrite_placeholders};
use crate::archive::{ActionKind, ActionSpec, DocumentConfig, SavedQuery};
use crate::automation::TriggerEvent;
use crate::data::{self, sqltext, DataValue, NamedValue};
use crate::manager::AppError;
use serde::Serialize;

/// Words an action statement may not contain: DDL, catalog, settings and file access.
const FORBIDDEN: [&str; 37] = [
    "CREATE",
    "DROP",
    "ALTER",
    "TRUNCATE",
    "ATTACH",
    "DETACH",
    "INSTALL",
    "LOAD",
    "COPY",
    "EXPORT",
    "IMPORT",
    "CALL",
    "PRAGMA",
    "RESET",
    "CHECKPOINT",
    "VACUUM",
    "USE",
    "READ_CSV",
    "READ_CSV_AUTO",
    "READ_JSON",
    "READ_JSON_AUTO",
    "READ_PARQUET",
    "PARQUET_SCAN",
    "SQLITE_SCAN",
    "SQLITE_ATTACH",
    "SQLITE_QUERY",
    "READ_BLOB",
    "READ_TEXT",
    "GLOB",
    "POSTGRES_QUERY",
    "POSTGRES_EXECUTE",
    "POSTGRES_SCAN",
    "POSTGRES_ATTACH",
    "DUCKDB_DATABASES",
    "DUCKDB_SECRETS",
    "WHICH_SECRET",
    "BEGIN",
];

/// A word of masked SQL with its byte offset and parenthesis depth.
struct Word {
    text: String,
    at: usize,
    depth: usize,
}

fn words(masked: &str) -> Vec<Word> {
    let mut out = vec![];
    let mut depth = 0usize;
    let mut start: Option<usize> = None;
    let bytes = masked.as_bytes();
    for (i, &b) in bytes
        .iter()
        .enumerate()
        .chain(std::iter::once((bytes.len(), &b' ')))
    {
        let word_char = b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
        match (word_char, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push(Word {
                    text: masked[s..i].to_ascii_uppercase(),
                    at: s,
                    depth,
                });
                start = None;
            }
            _ => {}
        }
        if b == b'(' {
            depth += 1;
        } else if b == b')' {
            depth = depth.saturating_sub(1);
        }
    }
    out
}

/// Reads a possibly qualified identifier at `at`: its parts, unquoted, and its end offset.
fn identifier(sql: &str, at: usize) -> (Vec<String>, usize) {
    let chars: Vec<char> = sql[at..].chars().collect();
    let mut parts = vec![];
    let mut i = 0;
    loop {
        let mut part = String::new();
        if chars.get(i) == Some(&'"') {
            i += 1;
            while i < chars.len() {
                if chars[i] == '"' {
                    if chars.get(i + 1) == Some(&'"') {
                        part.push('"');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                part.push(chars[i]);
                i += 1;
            }
        } else {
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                part.push(chars[i]);
                i += 1;
            }
        }
        parts.push(part);
        if chars.get(i) != Some(&'.') {
            let end = at + chars[..i].iter().map(|c| c.len_utf8()).sum::<usize>();
            return (parts, end);
        }
        i += 1;
    }
}

/// A checked INSERT, UPDATE or DELETE statement.
pub struct Guarded<'a> {
    /// The statement without a trailing `;`.
    pub statement: &'a str,
    /// Where the target table's name is, in `statement`.
    pub target: std::ops::Range<usize>,
    /// Whether an alias follows the target (`UPDATE orders o SET ...`).
    pub aliased: bool,
}

/// `target`, returning only the statement.
pub fn guard<'a>(sql: &'a str, spec: &ActionSpec, schema: &str) -> Result<&'a str, String> {
    target(sql, spec, schema).map(|g| g.statement)
}

/// Checks an INSERT, UPDATE or DELETE statement: exactly one statement, of the
/// declared kind, writing to the declared table (optionally qualified by the
/// `data` catalog or the datasource schema), with no forbidden word.
pub fn target<'a>(sql: &'a str, spec: &ActionSpec, schema: &str) -> Result<Guarded<'a>, String> {
    let trimmed = sql.trim();
    let masked = sqltext::mask(trimmed);
    let code = masked.trim_end();
    let code = code.strip_suffix(';').unwrap_or(code);
    if code.trim().is_empty() || code.contains(';') {
        return Err("An action query is exactly one statement".into());
    }
    let statement = trimmed[..code.len()].trim_end();
    let all = words(code);
    if let Some(w) = all.iter().find(|w| FORBIDDEN.contains(&w.text.as_str())) {
        return Err(format!("Action queries cannot use {}", w.text));
    }
    let file_function = all.iter().find(|w| {
        (w.text.starts_with("READ_") || w.text.ends_with("_SCAN"))
            && code[w.at + w.text.len()..].trim_start().starts_with('(')
    });
    if let Some(w) = file_function {
        return Err(format!("Action queries cannot read files ({})", w.text));
    }
    let verb = match spec.kind {
        ActionKind::Insert => "INSERT",
        ActionKind::Update => "UPDATE",
        ActionKind::Delete => "DELETE",
        ActionKind::Replace => return Err("A replace query is a SELECT".into()),
    };
    let top: Vec<&Word> = all.iter().filter(|w| w.depth == 0).collect();
    let first = top.first().map(|w| w.text.as_str()).unwrap_or("");
    let found = match first {
        "WITH" => top
            .iter()
            .position(|w| matches!(w.text.as_str(), "INSERT" | "UPDATE" | "DELETE" | "SELECT")),
        _ => Some(0),
    };
    let Some(index) = found.filter(|&i| top[i].text == verb) else {
        return Err(format!("This action query must be one {verb} statement"));
    };
    // The table follows INSERT [OR ...] INTO, UPDATE, or DELETE FROM.
    let mut next = index + 1;
    if verb == "INSERT" && top.get(next).is_some_and(|w| w.text == "OR") {
        next += 2;
    }
    if verb != "UPDATE" {
        if top.get(next).map(|w| w.text.as_str())
            != Some(if verb == "INSERT" { "INTO" } else { "FROM" })
        {
            return Err(format!("This action query must be one {verb} statement"));
        }
        next += 1;
    }
    let after = top
        .get(next - 1)
        .map(|w| w.at + w.text.len())
        .unwrap_or(statement.len());
    let rest = &statement[after..];
    let target_at = after + (rest.len() - rest.trim_start().len());
    let (mut parts, target_end) = identifier(statement, target_at);
    let table = parts.pop().unwrap_or_default();
    let qualified_ok = parts
        .iter()
        .enumerate()
        .all(|(i, p)| (i == 0 && p.eq_ignore_ascii_case("data")) || p.eq_ignore_ascii_case(schema));
    if table != spec.table || !qualified_ok {
        return Err(format!(
            "This action query must write to the table \"{}\"",
            spec.table
        ));
    }
    let following = all
        .iter()
        .find(|w| w.at >= target_end)
        .map(|w| w.text.as_str());
    let aliased = !matches!(
        following,
        None | Some(
            "SET" | "WHERE" | "USING" | "VALUES" | "SELECT" | "BY" | "DEFAULT" | "RETURNING" | "ON"
        )
    ) && !masked[target_end..].trim_start().starts_with('(');
    Ok(Guarded {
        statement,
        target: target_at..target_end,
        aliased,
    })
}

/// Created and updated rows, for firing triggers once per row.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChangedRows {
    /// Identities (primary key values, or rowid) of new rows.
    pub created: Vec<Vec<DataValue>>,
    pub updated: Vec<UpdatedRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdatedRow {
    pub identity: Vec<DataValue>,
    /// Every column's value before the statement.
    pub old: Vec<NamedValue>,
}

/// The outcome of one run.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRun {
    /// Rows the statement matched (for `replace`: rows inserted).
    pub changed: u64,
    /// Rows `replace` removed first.
    pub removed: u64,
    pub dry_run: bool,
    pub table: String,
    #[serde(flatten)]
    pub rows: ChangedRows,
    /// Sync app-mode trigger grants for the `created` and `updated` rows.
    pub created_grant: Option<String>,
    pub updated_grant: Option<String>,
}

/// The trigger events a kind of action query can raise.
fn events(kind: ActionKind) -> &'static [TriggerEvent] {
    match kind {
        ActionKind::Insert | ActionKind::Replace => &[TriggerEvent::Created],
        ActionKind::Update => &[TriggerEvent::Updated],
        ActionKind::Delete => &[],
    }
}

fn has_triggers(config: &DocumentConfig, table: &str, event: TriggerEvent) -> bool {
    config
        .triggers
        .iter()
        .any(|t| t.enabled && t.table == table && t.event == event)
}

/// The schema the `data` catalog shows: `main` for SQLite, else the configured one.
pub fn schema_of(config: &DocumentConfig) -> String {
    if config.datasource.is_postgres() {
        config.datasource.schema.clone()
    } else {
        "main".into()
    }
}

fn invalid(message: String) -> AppError {
    AppError::new("VALIDATION_ERROR", message)
}

/// Validates an action query's SQL (Problems tab): placeholders and the guard.
pub fn check_sql(sql: &str, spec: &ActionSpec, schema: &str) -> Result<super::Rewritten, AppError> {
    if spec.table.trim().is_empty() {
        return Err(invalid("an action query needs a target table".into()));
    }
    let mut rewritten = rewrite_placeholders(sql).map_err(invalid)?;
    rewritten.sql = match spec.kind {
        ActionKind::Replace => data::read_only_guard(&rewritten.sql)
            .map_err(|e| invalid(format!("a replace query is one SELECT: {e}")))?
            .to_string(),
        _ => guard(&rewritten.sql, spec, schema)
            .map_err(invalid)?
            .to_string(),
    };
    Ok(rewritten)
}

/// How a run executes.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// Statements run on the read-write attachment, in one transaction.
    Direct(Vec<String>),
    /// An embedded-SQLite UPDATE computed on a copy, then written back through DuckDB (see the record).
    Copy(String),
}

/// The copy an embedded-SQLite UPDATE runs on (`action_exec`).
pub const WORK: &str = "temp.main.__ixtable_work";

/// Plans a run: the statements, with `$n` placeholders, and the parameter names.
pub fn plan(
    query: &SavedQuery,
    spec: &ActionSpec,
    schema: &str,
    sqlite: bool,
) -> Result<(Plan, Vec<String>), AppError> {
    let rewritten = rewrite_placeholders(&query.sql).map_err(invalid)?;
    let table = format!("data.{}.{}", data::q(schema), data::q(&spec.table));
    let plan = match spec.kind {
        ActionKind::Replace => {
            let select = data::read_only_guard(&rewritten.sql)
                .map_err(|e| invalid(format!("A replace query is one SELECT: {e}")))?;
            Plan::Direct(vec![
                format!("DELETE FROM {table}"),
                format!("INSERT INTO {table} BY NAME ({select})"),
            ])
        }
        ActionKind::Update if sqlite => {
            let g = target(&rewritten.sql, spec, schema).map_err(invalid)?;
            let alias = if g.aliased {
                String::new()
            } else {
                format!(" AS {}", data::q(&spec.table))
            };
            Plan::Copy(format!(
                "{}{WORK}{alias}{}",
                &g.statement[..g.target.start],
                &g.statement[g.target.end..]
            ))
        }
        _ => Plan::Direct(vec![guard(&rewritten.sql, spec, schema)
            .map_err(invalid)?
            .to_string()]),
    };
    Ok((plan, rewritten.names))
}

/// Runs an action query. `dry_run` rolls the transaction back and reports the counts.
pub fn run(
    window: &str,
    id: &str,
    supplied: &[NamedValue],
    dry_run: bool,
) -> Result<ActionRun, AppError> {
    let manager = crate::manager()?;
    let config = manager.config(window)?;
    let query = config
        .saved_queries
        .iter()
        .find(|q| q.id == id)
        .ok_or_else(|| AppError::new("NOT_FOUND", format!("Saved query {id} not found")))?;
    let spec = query
        .action
        .clone()
        .ok_or_else(|| invalid(format!("\"{}\" is not an action query", query.name)))?;
    for &op in crate::trigger_auth::action_query_ops(spec.kind) {
        crate::authz::check(window, "table", &spec.table, op)?;
    }
    for &event in events(spec.kind) {
        crate::trigger_auth::precheck(window, &spec.table, event)?;
    }
    let custom = crate::recordstore::entity_policy(&config, &spec.table)
        .is_some_and(|e| e.concurrency == "customAction");
    if custom && spec.kind != ActionKind::Insert {
        return Err(invalid(format!(
            "The table \"{}\" routes updates and deletes to a custom action, so action queries cannot change it",
            spec.table
        )));
    }
    let target = crate::recordstore::read_target(&config.datasource)
        .map_err(|e| AppError::new("CONNECTION", e))?;
    let sqlite = target == data::read::ReadTarget::Sqlite;
    let schema = schema_of(&config);
    let (plan, names) = plan(query, &spec, &schema, sqlite)?;
    let values = resolve_params(&names, &query.parameters, supplied, true)?;
    let watch = events(spec.kind)
        .iter()
        .any(|&e| has_triggers(&config, &spec.table, e));
    let (workspace, extension, def) = manager.with_session(window, |s| {
        let def = s
            .reader
            .table_def(&spec.table)
            .map_err(|e| AppError::new("NOT_FOUND", e))?;
        Ok((
            s.workspace.clone(),
            s.reader.sqlite_extension().to_path_buf(),
            def,
        ))
    })?;
    let keys = def.primary_key;
    let logical: Vec<(String, crate::data::logical::LogicalType)> = def
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.logical_type.clone()))
        .collect();
    if watch && keys.is_empty() && !matches!(plan, Plan::Copy(_)) {
        return Err(invalid(format!(
            "The table \"{}\" has triggers but no primary key, so an action query cannot tell which rows changed",
            spec.table
        )));
    }
    let job = super::action_exec::Job {
        table: format!("data.{}.{}", data::q(&schema), data::q(&spec.table)),
        name: spec.table.clone(),
        keys,
        rowid: sqlite && !def.without_rowid,
        kind: spec.kind,
        watch,
        dry_run,
    };
    let outcome = crate::recordstore::blocking(|| {
        let _gate = if sqlite {
            Some(
                data::gate::exclusive(&workspace.join("data.db"))
                    .map_err(|e| AppError::new("BUSY", e))?,
            )
        } else {
            None
        };
        let open = |text: bool| {
            data::write::open_writer(&workspace, &extension, &target, text)
                .map_err(|e| AppError::new("CONNECTION", e))
        };
        let connection = open(false)?;
        match &plan {
            Plan::Direct(sql) => super::action_exec::direct(&connection, sql, &values, &job),
            Plan::Copy(sql) => {
                let computed = super::action_exec::compute_update(&connection, sql, &values, &job)?;
                drop(connection);
                if !dry_run && !computed.changes.rows.is_empty() {
                    let text = super::action_exec::needs_text(&computed.changes, &spec.table)?;
                    super::action_exec::write_back(
                        &open(text)?,
                        &job,
                        &computed.changes,
                        &logical,
                        text,
                    )?;
                }
                Ok(computed.outcome)
            }
        }
    })?;
    let mut run = ActionRun {
        changed: outcome.changed,
        removed: outcome.removed,
        dry_run,
        table: spec.table.clone(),
        rows: outcome.rows,
        created_grant: None,
        updated_grant: None,
    };
    if !dry_run {
        crate::recordstore::commands::after_write(window)?;
        let now = std::time::Instant::now();
        if !run.rows.created.is_empty() {
            run.created_grant = crate::trigger_auth::issue(
                window,
                &config,
                &spec.table,
                TriggerEvent::Created,
                now,
            );
        }
        if !run.rows.updated.is_empty() {
            run.updated_grant = crate::trigger_auth::issue(
                window,
                &config,
                &spec.table,
                TriggerEvent::Updated,
                now,
            );
        }
    }
    Ok(run)
}

/// Runs an action query by id (`docs/decisions/action-queries.md`). With
/// `dry_run` the changes roll back and only the counts come back.
#[tauri::command]
pub async fn run_action_query(
    window_label: String,
    id: String,
    params: Vec<crate::data::NamedValue>,
    dry_run: Option<bool>,
) -> Result<crate::queries::action::ActionRun, AppError> {
    super::blocking(move || run(&window_label, &id, &params, dry_run.unwrap_or(false))).await
}

/// Checks an action query's SQL before it is saved (the guard and placeholders).
#[tauri::command]
pub fn check_action_query_sql(
    window_label: String,
    sql: String,
    action: crate::archive::ActionSpec,
) -> Result<(), AppError> {
    let config = crate::manager()?.config(&window_label)?;
    check_sql(&sql, &action, &schema_of(&config)).map(|_| ())
}

/// Binds an action query's statements without running them (`prepare`), on a
/// writer connection. Catches unknown tables and columns.
pub fn prepare_check(
    connection: &duckdb::Connection,
    query: &SavedQuery,
    spec: &ActionSpec,
    schema: &str,
) -> Result<(), String> {
    let (plan, _) = plan(query, spec, schema, false).map_err(|e| e.message)?;
    let Plan::Direct(statements) = plan else {
        return Ok(());
    };
    for statement in statements {
        connection.prepare(&statement).map_err(|e| e.to_string())?;
    }
    Ok(())
}
