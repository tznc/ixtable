//! Tauri commands for record writes, schema changes, indexes, and the
//! datasource. Every write commits in the RecordStore before the DuckDB
//! reader is refreshed, so the next read sees it (read-your-writes).
use super::{
    entity_policy, read_target, secrets, with_store, ChangePlan, Dependent, EntitySettings,
    FieldSettings, StoreCapabilities, TableImpact, WriteOp, WriteOutcome,
};
use crate::archive::DocumentConfig;
use crate::data::{AlterTable, CreateTable, IndexDef, NamedValue};
use crate::manager::{AppError, SessionState};
use serde::Serialize;
use serde_json::Value;

/// Refreshes the reader after a committed write. Embedded SQLite data lives
/// in the archive, so the document also becomes dirty; PostgreSQL does not.
pub fn after_write(window: &str) -> Result<SessionState, AppError> {
    let m = crate::manager()?;
    if m.config(window)?.datasource.is_postgres() {
        // A refresh failure after a committed write is a stale-read warning, never an error.
        m.with_session(window, |s| {
            crate::manager::refresh_after_write(s);
            Ok(s.state())
        })
    } else {
        m.mark_data_dirty(window)
    }
}
fn guard_definition(window: &str) -> Result<(), AppError> {
    crate::manager()?.with_session(window, |s| crate::manager::read_only_guard(s))
}
/// Optimistic, custom-action, and unresolved entities check original values;
/// last-write-wins ignores them.
fn effective_expected(
    window: &str,
    table: &str,
    expected: Option<Vec<NamedValue>>,
) -> Result<Option<Vec<NamedValue>>, AppError> {
    let config = crate::manager()?.config(window)?;
    resolve_expected(
        entity_policy(&config, table).map(|e| e.concurrency.as_str()),
        table,
        expected,
    )
}

/// Applies the concurrency policy (PRD §19) to a write's original values. Every
/// policy except `lastWriteWins` (an unresolved one included) needs them: an
/// update or delete without `expected` fails with `EXPECTED_REQUIRED` instead
/// of overwriting blindly.
///
/// `customAction` is enforced here like `optimistic`: routing the write to the
/// entity's action happens only in the TypeScript frontend (`src/automation/custom.ts`),
/// so a direct command with `expected` writes the row without running the action.
pub(crate) fn resolve_expected(
    policy: Option<&str>,
    table: &str,
    expected: Option<Vec<NamedValue>>,
) -> Result<Option<Vec<NamedValue>>, AppError> {
    if policy == Some("lastWriteWins") {
        return Ok(None);
    }
    match expected.filter(|e| !e.is_empty()) {
        Some(e) => Ok(Some(e)),
        None => Err(AppError::new(
            "EXPECTED_REQUIRED",
            format!(
                "{table} uses the {} concurrency policy: updates and deletes must send the original values they started from",
                policy.unwrap_or("optimistic")
            ),
        )),
    }
}

fn update_entities(window: &str, f: impl FnOnce(&mut Vec<EntitySettings>)) -> Result<(), AppError> {
    let m = crate::manager()?;
    let mut config = m.config(window)?;
    let before = config.entities.clone();
    f(&mut config.entities);
    if config.entities != before {
        m.update_config(window, config)?;
    }
    Ok(())
}

/// Creates a table and registers it as an entity with the optimistic policy.
pub fn create_table(window: &str, spec: &CreateTable) -> Result<SessionState, AppError> {
    guard_definition(window)?;
    with_store(window, |s| s.create_table(spec))?;
    update_entities(window, |entities| {
        if !entities.iter().any(|e| e.table == spec.name) {
            entities.push(EntitySettings {
                id: uuid::Uuid::now_v7().to_string(),
                table: spec.name.clone(),
                ..Default::default()
            });
        }
    })?;
    after_write(window)
}

pub fn alter_table(
    window: &str,
    table: &str,
    operations: &[AlterTable],
) -> Result<SessionState, AppError> {
    guard_definition(window)?;
    with_store(window, |s| s.alter_table(table, operations))?;
    update_entities(window, |entities| {
        for e in entities.iter_mut().filter(|e| e.table == table) {
            follow_columns(&mut e.fields, operations);
        }
    })?;
    if let Some(new_name) = operations.iter().rev().find_map(|op| match op {
        AlterTable::RenameTable { new_name } => Some(new_name.clone()),
        _ => None,
    }) {
        update_entities(window, |entities| {
            for e in entities.iter_mut().filter(|e| e.table == table) {
                e.table = new_name.clone();
            }
        })?;
    }
    after_write(window)
}

/// Field settings follow column renames and drop with their column.
pub fn follow_columns(fields: &mut Vec<FieldSettings>, operations: &[AlterTable]) {
    for op in operations {
        match op {
            AlterTable::RenameColumn { column, new_name } => fields
                .iter_mut()
                .filter(|f| &f.column == column)
                .for_each(|f| f.column = new_name.clone()),
            AlterTable::DropColumn { column } => fields.retain(|f| &f.column != column),
            _ => {}
        }
    }
}

/// Saved queries, forms, reports, dashboards, and actions that mention a table.
pub fn config_dependents(config: &DocumentConfig, table: &str) -> Vec<Dependent> {
    fn hit(v: &Value, table: &str) -> bool {
        match v {
            Value::String(s) => {
                s.eq_ignore_ascii_case(table)
                    || (s.contains(char::is_whitespace) && super::plan::mentions(s, table))
            }
            Value::Array(a) => a.iter().any(|x| hit(x, table)),
            Value::Object(o) => o
                .iter()
                .any(|(k, x)| !DESCRIPTIVE_KEYS.contains(&k.as_str()) && hit(x, table)),
            _ => false,
        }
    }
    /// Enum-like keys (control kinds, types) that never hold a table or column reference.
    const DESCRIPTIVE_KEYS: &[&str] = &[
        "kind",
        "type",
        "logicalType",
        "declaredType",
        "physicalType",
        "format",
        "direction",
        "aggregate",
        "mode",
        "objectKind",
        "objectType",
        "status",
        "align",
        "variant",
        "label",
    ];
    /// Reports the shallowest objects with an id and a name whose subtree mentions the table.
    fn walk(kind: &str, v: &Value, table: &str, out: &mut Vec<Dependent>) {
        match v {
            Value::Array(items) => items.iter().for_each(|x| walk(kind, x, table, out)),
            Value::Object(o) => match (o.get("id"), o.get("name").or_else(|| o.get("title"))) {
                (Some(Value::String(id)), Some(Value::String(name))) => {
                    if hit(v, table) {
                        out.push(Dependent {
                            kind: kind.into(),
                            id: id.clone(),
                            name: name.clone(),
                        });
                    }
                }
                _ => o.values().for_each(|x| walk(kind, x, table, out)),
            },
            _ => {}
        }
    }
    let Ok(Value::Object(root)) = serde_json::to_value(config) else {
        return vec![];
    };
    let mut out = vec![];
    for (key, kind) in [
        ("savedQueries", "query"),
        ("design", "form"),
        ("reports", "report"),
        ("dashboards", "dashboard"),
        ("actions", "action"),
        ("triggers", "trigger"),
    ] {
        if let Some(value) = root.get(key) {
            walk(kind, value, table, &mut out);
        }
    }
    out
}

/// Definitions that refer to the names a batch of operations renames, with one
/// warning per rename. Renames do not rewrite these definitions, so the impact
/// preview lists them before the change is applied (PRD §11).
pub fn rename_dependents(
    config: &DocumentConfig,
    table: &str,
    operations: &[AlterTable],
) -> (Vec<Dependent>, Vec<String>) {
    let of_table = config_dependents(config, table);
    let mut dependents: Vec<Dependent> = vec![];
    let mut warnings = vec![];
    let list = |deps: &[Dependent]| {
        deps.iter()
            .map(|d| format!("{} \u{201c}{}\u{201d}", d.kind, d.name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    for op in operations {
        let (old, new, found) = match op {
            AlterTable::RenameTable { new_name } => (table, new_name, of_table.clone()),
            AlterTable::RenameColumn { column, new_name } => {
                let of_column = config_dependents(config, column);
                let found = of_table
                    .iter()
                    .filter(|d| of_column.iter().any(|c| c.id == d.id))
                    .cloned()
                    .collect();
                (column.as_str(), new_name, found)
            }
            _ => continue,
        };
        if found.is_empty() {
            continue;
        }
        warnings.push(format!(
            "Renaming {old} to {new} does not update definitions that use the old name: {}. Update them after applying, or they will fail to load their data.",
            list(&found)
        ));
        for d in found {
            if !dependents.iter().any(|x| x.id == d.id) {
                dependents.push(d);
            }
        }
    }
    (dependents, warnings)
}

#[tauri::command]
pub fn store_capabilities(window_label: String) -> Result<StoreCapabilities, AppError> {
    let config = crate::manager()?.config(&window_label)?;
    Ok(if config.datasource.is_postgres() {
        super::capabilities::postgres()
    } else {
        super::capabilities::sqlite()
    })
}

#[tauri::command]
pub fn table_drop_impact(window_label: String, table: String) -> Result<TableImpact, AppError> {
    let mut impact = with_store(&window_label, |s| s.impact(&table))?;
    impact.dependents = config_dependents(&crate::manager()?.config(&window_label)?, &table);
    Ok(impact)
}

#[tauri::command]
pub fn drop_database_table(window_label: String, table: String) -> Result<SessionState, AppError> {
    guard_definition(&window_label)?;
    with_store(&window_label, |s| s.drop_table(&table))?;
    update_entities(&window_label, |entities| {
        entities.retain(|e| e.table != table)
    })?;
    after_write(&window_label)
}

#[tauri::command]
pub fn preview_table_changes(
    window_label: String,
    table: String,
    operations: Vec<crate::data::AlterTable>,
) -> Result<ChangePlan, AppError> {
    let (mut plan, impact) = with_store(&window_label, |s| {
        let plan = s.plan_alter(&table, &operations)?;
        let impact = if plan.destructive || plan.rebuild {
            Some(s.impact(&table)?)
        } else {
            None
        };
        Ok((plan, impact))
    })?;
    let config = crate::manager()?.config(&window_label)?;
    let (renamed, warnings) = rename_dependents(&config, &table, &operations);
    plan.warnings.extend(warnings);
    let impact = match impact {
        Some(mut impact) => {
            impact.dependents = config_dependents(&config, &table);
            Some(impact)
        }
        // A rename that leaves definitions pointing at the old name is reviewed like a destructive change.
        None if !renamed.is_empty() => {
            let mut impact = with_store(&window_label, |s| s.impact(&table))?;
            impact.dependents = renamed;
            Some(impact)
        }
        None => None,
    };
    if let Some(mut impact) = impact {
        impact.statements = plan.statements.clone();
        plan.impact = Some(impact);
    }
    Ok(plan)
}

#[tauri::command]
pub fn apply_table_changes(
    window_label: String,
    table: String,
    operations: Vec<crate::data::AlterTable>,
) -> Result<SessionState, AppError> {
    alter_table(&window_label, &table, &operations)
}

#[tauri::command]
pub fn create_index(
    window_label: String,
    spec: crate::recordstore::CreateIndex,
) -> Result<SessionState, AppError> {
    guard_definition(&window_label)?;
    with_store(&window_label, |s| s.create_index(&spec))?;
    after_write(&window_label)
}

#[tauri::command]
pub fn drop_index(window_label: String, name: String) -> Result<SessionState, AppError> {
    guard_definition(&window_label)?;
    with_store(&window_label, |s| s.drop_index(&name))?;
    after_write(&window_label)
}

#[tauri::command]
pub fn list_indexes(
    window_label: String,
    table: Option<String>,
) -> Result<Vec<IndexDef>, AppError> {
    with_store(&window_label, |s| s.list_indexes(table.as_deref()))
}

fn op_kind(op: &WriteOp) -> crate::authz::Op {
    match op {
        WriteOp::Insert { .. } => crate::authz::Op::Create,
        WriteOp::Update { .. } => crate::authz::Op::Update,
        WriteOp::Delete { .. } => crate::authz::Op::Delete,
    }
}
fn trigger_event(op: &WriteOp) -> Option<crate::automation::TriggerEvent> {
    match op {
        WriteOp::Insert { .. } => Some(crate::automation::TriggerEvent::Created),
        WriteOp::Update { .. } => Some(crate::automation::TriggerEvent::Updated),
        WriteOp::Delete { .. } => None,
    }
}

/// A committed write's outcome plus the sync trigger grant it issued.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggeredOutcome {
    #[serde(flatten)]
    pub outcome: WriteOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_grant: Option<String>,
}

/// Authorizes each op (role, or a verified trigger step) and refuses writes
/// whose user-mode triggers the role could not run.
fn authorize_ops(
    window: &str,
    ops: &[WriteOp],
    triggers: &[Option<crate::trigger_auth::TriggerWrite>],
) -> Result<(), AppError> {
    for (i, op) in ops.iter().enumerate() {
        let auth = triggers.get(i).and_then(Option::as_ref);
        crate::trigger_auth::authorize(window, op.table(), op_kind(op), auth)?;
        if let Some(event) = trigger_event(op) {
            crate::trigger_auth::precheck(window, op.table(), event)?;
        }
    }
    Ok(())
}

fn with_grants(
    window: &str,
    ops: &[WriteOp],
    outcomes: Vec<WriteOutcome>,
) -> Result<Vec<TriggeredOutcome>, AppError> {
    let config = crate::manager()?.config(window)?;
    let now = std::time::Instant::now();
    Ok(ops
        .iter()
        .zip(outcomes)
        .map(|(op, outcome)| {
            let trigger_grant = trigger_event(op).and_then(|event| {
                crate::trigger_auth::issue(window, &config, op.table(), event, now)
            });
            TriggeredOutcome {
                outcome,
                trigger_grant,
            }
        })
        .collect())
}

/// Applies `effective_expected` to every update and delete in a batch.
fn resolve_batch_expected(window: &str, ops: Vec<WriteOp>) -> Result<Vec<WriteOp>, AppError> {
    let mut resolved = Vec::with_capacity(ops.len());
    for op in ops {
        resolved.push(match op {
            WriteOp::Update {
                table,
                values,
                identity,
                expected,
            } => {
                let expected = effective_expected(window, &table, expected)?;
                WriteOp::Update {
                    table,
                    values,
                    identity,
                    expected,
                }
            }
            WriteOp::Delete {
                table,
                identity,
                expected,
            } => {
                let expected = effective_expected(window, &table, expected)?;
                WriteOp::Delete {
                    table,
                    identity,
                    expected,
                }
            }
            other => other,
        });
    }
    Ok(resolved)
}

/// Applies `ops` as one transaction. `triggers[i]` marks op i as a trigger
/// step (see trigger_auth.rs); each insert/update returns the grant for its
/// app-mode sync triggers.
#[tauri::command]
pub fn execute_write_batch(
    window_label: String,
    ops: Vec<crate::recordstore::WriteOp>,
    triggers: Option<Vec<Option<crate::trigger_auth::TriggerWrite>>>,
) -> Result<Vec<TriggeredOutcome>, AppError> {
    authorize_ops(&window_label, &ops, &triggers.unwrap_or_default())?;
    let resolved = resolve_batch_expected(&window_label, ops)?;
    let out = with_store(&window_label, |s| s.execute_batch(&resolved))?;
    after_write(&window_label)?;
    with_grants(&window_label, &resolved, out)
}

/// One record write (`insert_row`, `update_row`, `delete_row`):
/// `execute_write_batch` with one op.
pub fn write_one(
    window_label: String,
    op: crate::recordstore::WriteOp,
    trigger: Option<crate::trigger_auth::TriggerWrite>,
) -> Result<TriggeredOutcome, AppError> {
    let mut out = execute_write_batch(window_label, vec![op], Some(vec![trigger]))?;
    out.pop()
        .ok_or_else(|| AppError::new("INTERNAL", "The write returned no outcome"))
}

/// Record writes return `{changed, identity?, triggerGrant?}` (see
/// `write_one`); `trigger` marks an app-mode trigger step.
#[tauri::command]
pub fn insert_row(
    window_label: String,
    table: String,
    values: Vec<crate::data::NamedValue>,
    trigger: Option<crate::trigger_auth::TriggerWrite>,
) -> Result<TriggeredOutcome, AppError> {
    write_one(window_label, WriteOp::Insert { table, values }, trigger)
}
/// `expected` carries the values the user started from; entities with the
/// optimistic policy reject the update with CONFLICT when they changed, and
/// with EXPECTED_REQUIRED when it is missing (`resolve_expected`).
#[tauri::command]
pub fn update_row(
    window_label: String,
    table: String,
    values: Vec<crate::data::NamedValue>,
    identity: Vec<crate::data::DataValue>,
    expected: Option<Vec<crate::data::NamedValue>>,
    trigger: Option<crate::trigger_auth::TriggerWrite>,
) -> Result<TriggeredOutcome, AppError> {
    let op = WriteOp::Update {
        table,
        values,
        identity,
        expected,
    };
    write_one(window_label, op, trigger)
}
#[tauri::command]
pub fn delete_row(
    window_label: String,
    table: String,
    identity: Vec<crate::data::DataValue>,
    expected: Option<Vec<crate::data::NamedValue>>,
    trigger: Option<crate::trigger_auth::TriggerWrite>,
) -> Result<TriggeredOutcome, AppError> {
    let op = WriteOp::Delete {
        table,
        identity,
        expected,
    };
    write_one(window_label, op, trigger)
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionReport {
    pub ok: bool,
    pub store: String,
    pub server_version: Option<String>,
    pub encrypted: bool,
    pub message: String,
}

/// Tries a datasource without saving it. A blank password uses the stored one.
#[tauri::command]
pub fn test_datasource_connection(
    window_label: String,
    datasource: crate::recordstore::DatasourceConfig,
    password: Option<String>,
) -> Result<ConnectionReport, AppError> {
    crate::manager()?.config(&window_label)?;
    if !datasource.is_postgres() {
        return Ok(ConnectionReport {
            ok: true,
            store: "sqlite".into(),
            server_version: Some(rusqlite::version().into()),
            encrypted: false,
            message: "The embedded SQLite store is always available.".into(),
        });
    }
    secrets::ensure_transport(&datasource)?;
    let password = match password.filter(|p| !p.is_empty()) {
        Some(p) => Some(p),
        None => secrets::datasource_credential(&datasource)?,
    };
    Ok(
        match super::blocking(|| crate::postgres::probe(&datasource, password.as_deref())) {
            Ok((version, encrypted)) => ConnectionReport {
                ok: true,
                store: "postgres".into(),
                server_version: Some(version),
                encrypted,
                message: if encrypted {
                    "Connected over TLS.".into()
                } else {
                    "Connected WITHOUT TLS: credentials and records are not encrypted in transit."
                        .into()
                },
            },
            Err(e) => ConnectionReport {
                ok: false,
                store: "postgres".into(),
                server_version: None,
                encrypted: false,
                message: e.message,
            },
        },
    )
}

/// Stores a datasource password in the local secret store; returns the reference to keep in config.
#[tauri::command]
pub fn set_datasource_password(
    window_label: String,
    datasource_id: String,
    password: String,
    datasource: Option<crate::recordstore::DatasourceConfig>,
) -> Result<String, AppError> {
    let config = crate::manager()?.config(&window_label)?;
    if datasource_id.trim().is_empty() {
        return Err(AppError::new(
            "VALIDATION_ERROR",
            "The datasource needs an id",
        ));
    }
    // The password is bound to the server it is entered for (secrets::datasource_target).
    let mut target = datasource.unwrap_or(config.datasource);
    target.id = datasource_id;
    secrets::store_datasource_password(&target, &password).map_err(|e| AppError::new("IO_ERROR", e))
}

#[tauri::command]
pub fn clear_datasource_password(
    window_label: String,
    password_ref: String,
) -> Result<(), AppError> {
    crate::manager()?.config(&window_label)?;
    secrets::SecretStore::default_location()
        .and_then(|s| s.delete(&password_ref))
        .map_err(|e| AppError::new("IO_ERROR", e))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasourceStatus {
    pub kind: String,
    pub attached: bool,
    pub error: Option<String>,
    pub has_password: bool,
}

/// Re-attaches the reader to the configured datasource and reports the result.
#[tauri::command]
pub fn connect_datasource(window_label: String) -> Result<DatasourceStatus, AppError> {
    let m = crate::manager()?;
    let config = m.config(&window_label)?;
    let has_password = secrets::datasource_password(&config.datasource)
        .ok()
        .flatten()
        .is_some();
    let target = read_target(&config.datasource);
    m.with_session(&window_label, |s| {
        let error = match target {
            Ok(t) => s.reader.set_target(t).err(),
            Err(e) => Some(e),
        };
        Ok(DatasourceStatus {
            kind: config.datasource.kind.clone(),
            attached: error.is_none(),
            error,
            has_password,
        })
    })
}
