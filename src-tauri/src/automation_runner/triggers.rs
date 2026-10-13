//! Record triggers (PRD §17.3), hooked into `crate::records` (formerly `triggers.ts`).
//!
//! Before-change triggers run before a create or update is written: their actions may set
//! fields of the record (`setField`) or reject the save (a `fail` step or any failing
//! step), and cannot write records, so a rejection writes nothing. Action queries hand them
//! each pending row through `before_bulk`.
//!
//! Created, updated and deleted triggers run after the write commits. Sync triggers run
//! their action after the initiating write commits, inside the same workflow: the caller's
//! `records::insert/update/delete` waits for them and a failing trigger makes that call
//! report a committed write error. The write itself is already committed then; rollback-mode
//! actions only undo their own writes. Async triggers enqueue a job on the durable queue
//! (`jobs.rs`); the worker runs it. Writes carry their trigger depth; a chain deeper than
//! `MAX_TRIGGER_DEPTH` fails.
//!
//! Execution identity (`Trigger::run_as`): app-mode triggers (the default) run with the
//! grant Rust returned for the initiating write (sync) or their job lease (async), limited
//! to the writes they declare; user-mode triggers run under the signed-in user's role, and
//! Rust refuses the initiating write up front when the role could not run them.

use super::context::{ContextBase, TriggerEnv};
use super::rows::read_row;
use super::runner::run_action_by_id;
use super::values::{
    from_data_value, js_string, named_to_object, stable_hash, to_named_values,
};
use crate::archive::DocumentConfig;
use crate::automation::{Trigger, TriggerEvent, TriggerMode};
use crate::data::DataValue;
use crate::expr::{self, EvaluateOptions};
use crate::manager::AppError;
use crate::queries::action::{ActionRun, Override, PendingRow};
use crate::records::{
    self, RecordHook, RecordOperation, RecordWrite, RecordWriteMeta, Registration, WriteResult,
};
use crate::trigger_auth::TriggerWrite;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub const MAX_TRIGGER_DEPTH: u32 = 5;

/// What an async job carries to the worker.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JobPayload {
    pub table: String,
    pub event: String,
    pub record: Map<String, Value>,
    pub old: Option<Map<String, Value>>,
    pub identity: Vec<Value>,
    pub trigger_depth: u32,
    /// Role active when the job was created (None: developer); the worker never exceeds it.
    pub role_id: Option<String>,
}

pub fn event_key(event: TriggerEvent) -> &'static str {
    match event {
        TriggerEvent::Created => "created",
        TriggerEvent::Updated => "updated",
        TriggerEvent::Deleted => "deleted",
        TriggerEvent::BeforeChange => "beforeChange",
    }
}

fn enabled_for(
    config: &DocumentConfig,
    table: &str,
    event: TriggerEvent,
) -> Vec<Trigger> {
    config
        .triggers
        .iter()
        .filter(|t| t.enabled && t.table == table && t.event == event)
        .cloned()
        .collect()
}

fn failed(code: &str, message: impl ToString) -> AppError {
    AppError::new(code, message)
}

/// The role the session enforces now (None: developer access).
pub fn active_role_id(label: &str) -> Option<String> {
    crate::manager()
        .and_then(|m| {
            m.with_session(label, |s| {
                Ok(crate::authz::effective_role(s).map(|r| r.id))
            })
        })
        .ok()
        .flatten()
}

/// Registers the trigger hook on record writes of the environment's window. Before an
/// update with `updated` or `beforeChange` triggers, or a delete with `deleted` triggers,
/// it reads the row so `old` is available even when the caller did not pass `meta.old`.
/// Before-change triggers then decide the write.
pub fn install_triggers(env: Arc<dyn TriggerEnv>) -> Registration {
    let label = env.label().to_string();
    records::register_hook(
        &label,
        Arc::new(TriggerHook {
            env,
            previous: Mutex::new(HashMap::new()),
        }),
    )
}

struct TriggerHook {
    env: Arc<dyn TriggerEnv>,
    /// Old rows read before a write, by write id, until its after hook has run.
    previous: Mutex<HashMap<String, Option<Map<String, Value>>>>,
}

impl TriggerHook {
    fn remember(&self, write_id: &str, old: Option<Map<String, Value>>) {
        let mut previous = self.previous.lock().unwrap_or_else(|e| e.into_inner());
        // Entries of writes that never committed would pile up; keep the map small.
        if previous.len() > 1000 {
            previous.clear();
        }
        previous.insert(write_id.to_string(), old);
    }
    fn take(&self, write_id: &str) -> Option<Map<String, Value>> {
        self.previous
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(write_id)
            .flatten()
    }
}

impl RecordHook for TriggerHook {
    fn before(&self, write: RecordWrite) -> Result<RecordWrite, AppError> {
        let config = self.env.config();
        let has = |event| !enabled_for(&config, &write.table, event).is_empty();
        let needs_old = match write.operation {
            RecordOperation::Update => has(TriggerEvent::Updated) || has(TriggerEvent::BeforeChange),
            RecordOperation::Delete => has(TriggerEvent::Deleted),
            RecordOperation::Insert => false,
        };
        let given = write.meta.old.is_some();
        let old = write.meta.old.clone().or_else(|| {
            if needs_old {
                write
                    .identity
                    .as_ref()
                    .and_then(|id| read_row(self.env.label(), &write.table, id))
            } else {
                None
            }
        });
        let write_id = write.meta.write_id.clone();
        let next = if write.operation == RecordOperation::Delete || !has(TriggerEvent::BeforeChange)
        {
            write
        } else {
            before_change(write, old.as_ref(), self.env.as_ref())?
        };
        if needs_old && !given {
            self.remember(&write_id, old);
        }
        Ok(next)
    }

    fn before_bulk(
        &self,
        table: &str,
        rows: &[PendingRow],
        depth: u32,
    ) -> Result<Vec<Override>, AppError> {
        if enabled_for(&self.env.config(), table, TriggerEvent::BeforeChange).is_empty() {
            return Ok(Vec::new());
        }
        let mut overrides = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let old = row.old.as_ref().map(|o| named_to_object(o));
            let set = decide(
                self.env.as_ref(),
                table,
                named_to_object(&row.values),
                old.as_ref(),
                depth,
            )?;
            if !set.is_empty() {
                overrides.push(Override {
                    row: i,
                    values: to_named_values(&set),
                });
            }
        }
        Ok(overrides)
    }

    fn after(
        &self,
        write: &RecordWrite,
        result: &WriteResult,
        grant: Option<&str>,
    ) -> Result<(), AppError> {
        let old = self.take(&write.meta.write_id);
        match &write.meta.bulk {
            Some(bulk) => dispatch_bulk(write, bulk, self.env.as_ref()),
            None => dispatch_triggers(write, result, self.env.as_ref(), old, grant),
        }
    }
}

/// Default idempotency key: `triggerId:table:pk:event:hash(values)[:writeId]`. The write id
/// (one per `records` write call) makes it dedupe retries of the same write only; a later
/// identical write is a new event and enqueues again.
pub fn default_idempotency_key(
    trigger: &Trigger,
    identity: &[Value],
    values: &Map<String, Value>,
    write_id: Option<&str>,
) -> String {
    let ids: Vec<String> = identity.iter().map(js_string).collect();
    let key = format!(
        "{}:{}:{}:{}:{}",
        trigger.id,
        trigger.table,
        ids.join(","),
        event_key(trigger.event),
        stable_hash(&Value::Object(values.clone()))
    );
    match write_id.filter(|w| !w.is_empty()) {
        Some(id) => format!("{key}:{id}"),
        None => key,
    }
}

/// Runs the after hooks' trigger work for one committed write and ends its grant.
pub fn dispatch_triggers(
    write: &RecordWrite,
    result: &WriteResult,
    env: &dyn TriggerEnv,
    old_row: Option<Map<String, Value>>,
    grant: Option<&str>,
) -> Result<(), AppError> {
    let outcome = dispatch(write, result, env, old_row, grant);
    // The grant is single use: it ends with this trigger run.
    if let Some(grant) = grant {
        let _ = crate::trigger_auth::release_trigger_grant(env.label().into(), grant.into());
    }
    outcome
}

fn scope_of(record: &Map<String, Value>, old: Option<&Map<String, Value>>, app: &Map<String, Value>) -> Map<String, Value> {
    let mut scope = Map::new();
    scope.insert("record".into(), Value::Object(record.clone()));
    scope.insert("old".into(), old.map_or(Value::Null, |o| Value::Object(o.clone())));
    scope.insert("app".into(), Value::Object(app.clone()));
    scope
}

fn condition_holds(trigger: &Trigger, scope: &Map<String, Value>) -> Result<bool, AppError> {
    match trigger.condition.as_deref().filter(|c| !c.trim().is_empty()) {
        None => Ok(true),
        Some(src) => expr::evaluate_boolean(src, scope, EvaluateOptions::default())
            .map_err(|e| failed("TRIGGER_CONDITION", e.message)),
    }
}

/// Runs the before-change triggers of a create or update: returns the write with the fields
/// they set, or an error that rejects the save.
fn before_change(
    write: RecordWrite,
    old: Option<&Map<String, Value>>,
    env: &dyn TriggerEnv,
) -> Result<RecordWrite, AppError> {
    let values = named_to_object(&write.values);
    let record = if write.operation == RecordOperation::Update {
        let mut merged = old.cloned().unwrap_or_default();
        merged.extend(values);
        merged
    } else {
        values
    };
    let old = if write.operation == RecordOperation::Insert {
        None
    } else {
        old
    };
    let set = decide(env, &write.table, record, old, write.meta.trigger_depth)?;
    if set.is_empty() {
        return Ok(write);
    }
    let mut next = write;
    next.values.retain(|v| !set.contains_key(&v.column));
    next.values.extend(to_named_values(&set));
    Ok(next)
}

/// Runs a table's before-change triggers in order on `record` (each sees the fields earlier
/// ones set) and returns the fields they set. A failing action rejects: a `fail` step with
/// its own message, anything else naming the trigger.
fn decide(
    env: &dyn TriggerEnv,
    table: &str,
    record: Map<String, Value>,
    old: Option<&Map<String, Value>>,
    trigger_depth: u32,
) -> Result<Map<String, Value>, AppError> {
    let set = Arc::new(Mutex::new(Map::new()));
    let app = env.app();
    for trigger in enabled_for(&env.config(), table, TriggerEvent::BeforeChange) {
        let mut current = record.clone();
        current.extend(set.lock().unwrap_or_else(|e| e.into_inner()).clone());
        if !condition_holds(&trigger, &scope_of(&current, old, &app))? {
            continue;
        }
        let outcome = run_action_by_id(
            &trigger.action_id,
            env.context(ContextBase {
                record: Some(current),
                old: old.cloned(),
                trigger_depth,
                before_change: Some(set.clone()),
                // App mode skips the role's checks; it holds no grant, since the action cannot write.
                trigger_auth: (!trigger.runs_as_user()).then(|| TriggerWrite {
                    trigger_id: trigger.id.clone(),
                    grant: Some(String::new()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        );
        if !outcome.ok {
            let error = outcome.error.unwrap_or_else(|| "unknown error".into());
            return Err(failed(
                "BEFORE_CHANGE_REJECTED",
                if outcome.aborted {
                    format!("Not saved: {error}")
                } else {
                    format!("Not saved: trigger \"{}\" failed: {error}", trigger.name)
                },
            ));
        }
    }
    let set = set.lock().unwrap_or_else(|e| e.into_inner()).clone();
    Ok(set)
}

/// Fires an action query's triggers once per created, updated or deleted row, in order, as if
/// each row had been written alone. The rows of one event share Rust's grant, released when
/// all of them have run. Failures are collected so one bad row does not stop the others'.
fn dispatch_bulk(
    write: &RecordWrite,
    bulk: &ActionRun,
    env: &dyn TriggerEnv,
) -> Result<(), AppError> {
    let mut failures = Vec::new();
    let depth = write.meta.trigger_depth;
    let write_id = &write.meta.write_id;
    let mut each = |row: RecordWrite, result: WriteResult, grant: Option<&String>| {
        if let Err(e) = dispatch(&row, &result, env, None, grant.map(String::as_str)) {
            failures.push(e.message);
        }
    };
    let row = |operation, suffix: String, identity: Option<Vec<DataValue>>, old: Option<Map<String, Value>>| RecordWrite {
        operation,
        table: write.table.clone(),
        values: Vec::new(),
        identity,
        meta: RecordWriteMeta {
            trigger_depth: depth,
            write_id: format!("{write_id}:{suffix}"),
            old,
            ..Default::default()
        },
    };
    for (i, identity) in bulk.rows.created.iter().enumerate() {
        each(
            row(RecordOperation::Insert, format!("c{i}"), None, None),
            WriteResult::Inserted(identity.clone()),
            bulk.created_grant.as_ref(),
        );
    }
    for (i, changed) in bulk.rows.updated.iter().enumerate() {
        each(
            row(
                RecordOperation::Update,
                format!("u{i}"),
                Some(changed.identity.clone()),
                Some(named_to_object(&changed.old)),
            ),
            WriteResult::Changed(1),
            bulk.updated_grant.as_ref(),
        );
    }
    for (i, deleted) in bulk.rows.deleted.iter().enumerate() {
        each(
            row(
                RecordOperation::Delete,
                format!("d{i}"),
                Some(deleted.identity.clone()),
                Some(named_to_object(&deleted.values)),
            ),
            WriteResult::Changed(1),
            bulk.deleted_grant.as_ref(),
        );
    }
    for grant in [&bulk.created_grant, &bulk.updated_grant, &bulk.deleted_grant]
        .into_iter()
        .flatten()
    {
        let _ = crate::trigger_auth::release_trigger_grant(env.label().into(), grant.clone());
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failed("TRIGGER_FAILED", failures.join("; ")))
    }
}

fn dispatch(
    write: &RecordWrite,
    result: &WriteResult,
    env: &dyn TriggerEnv,
    old_row: Option<Map<String, Value>>,
    grant: Option<&str>,
) -> Result<(), AppError> {
    let event = match write.operation {
        RecordOperation::Insert => TriggerEvent::Created,
        RecordOperation::Update => TriggerEvent::Updated,
        RecordOperation::Delete => TriggerEvent::Deleted,
    };
    let triggers = enabled_for(&env.config(), &write.table, event);
    if triggers.is_empty() {
        return Ok(());
    }
    let depth = write.meta.trigger_depth + 1;
    if depth > MAX_TRIGGER_DEPTH {
        return Err(failed(
            "TRIGGER_FAILED",
            format!(
                "Trigger recursion limit ({MAX_TRIGGER_DEPTH}) exceeded on {}; check triggers that write to their own table",
                write.table
            ),
        ));
    }
    let identity: Vec<DataValue> = match (write.operation, result) {
        (RecordOperation::Insert, WriteResult::Inserted(identity)) => identity.clone(),
        (RecordOperation::Insert, _) => Vec::new(),
        _ => write.identity.clone().unwrap_or_default(),
    };
    let values = named_to_object(&write.values);
    let old = write.meta.old.clone().or(old_row);
    // A deleted row is gone: its triggers see the values it had.
    let record = if event == TriggerEvent::Deleted {
        old.clone().unwrap_or_default()
    } else {
        read_row(env.label(), &write.table, &identity).unwrap_or_else(|| {
            let mut fallback = values.clone();
            if identity.len() == 1 {
                fallback.insert("rowid".into(), from_data_value(&identity[0]));
            }
            fallback
        })
    };
    let app = env.app();
    for trigger in triggers {
        let scope = scope_of(&record, old.as_ref(), &app);
        if !condition_holds(&trigger, &scope)? {
            continue;
        }
        if trigger.mode == TriggerMode::Async {
            enqueue(write, env, &trigger, event, &record, old.as_ref(), &identity, &values, depth, &scope)?;
            continue;
        }
        let outcome = run_action_by_id(
            &trigger.action_id,
            env.context(ContextBase {
                record: Some(record.clone()),
                old: old.clone(),
                trigger_depth: depth,
                trigger_auth: (!trigger.runs_as_user()).then(|| TriggerWrite {
                    trigger_id: trigger.id.clone(),
                    grant: Some(grant.unwrap_or("").to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        );
        if !outcome.ok {
            return Err(failed(
                "TRIGGER_FAILED",
                format!(
                    "Trigger \"{}\" failed: {}",
                    trigger.name,
                    outcome.error.unwrap_or_else(|| "unknown error".into())
                ),
            ));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn enqueue(
    write: &RecordWrite,
    env: &dyn TriggerEnv,
    trigger: &Trigger,
    event: TriggerEvent,
    record: &Map<String, Value>,
    old: Option<&Map<String, Value>>,
    identity: &[DataValue],
    values: &Map<String, Value>,
    depth: u32,
    scope: &Map<String, Value>,
) -> Result<(), AppError> {
    let plain: Vec<Value> = identity.iter().map(from_data_value).collect();
    let key = match trigger
        .idempotency_key
        .as_deref()
        .filter(|k| !k.trim().is_empty())
    {
        Some(src) => {
            let mut with_trigger = scope.clone();
            with_trigger.insert("trigger".into(), serde_json::json!({ "id": trigger.id }));
            let value = expr::evaluate(src, &with_trigger, EvaluateOptions::default())
                .map_err(|e| failed("TRIGGER_FAILED", e.message))?;
            js_string(&value)
        }
        None => default_idempotency_key(trigger, &plain, values, Some(&write.meta.write_id)),
    };
    let payload = JobPayload {
        table: write.table.clone(),
        event: event_key(event).into(),
        record: record.clone(),
        old: old.cloned(),
        identity: plain,
        trigger_depth: depth,
        role_id: active_role_id(env.label()),
    };
    crate::jobs::enqueue_job(
        env.label().into(),
        crate::jobs::EnqueueJob {
            trigger_id: trigger.id.clone(),
            action_id: trigger.action_id.clone(),
            payload: serde_json::to_value(payload).unwrap_or(Value::Null),
            idempotency_key: key,
            max_attempts: Some(trigger.max_attempts),
            backoff_ms: Some(trigger.backoff_ms),
        },
    )?;
    super::worker::notify_jobs_changed(env.label());
    Ok(())
}
