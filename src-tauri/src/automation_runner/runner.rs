//! Declarative action runner (PRD §17.2; formerly `src/automation/runner.ts`). Every
//! expression goes through `crate::expr`, every record write through `crate::records`
//! (so triggers see it), and every read through the DuckDB read path.
//!
//! Failure behavior (`ActionDef::on_error`):
//! - stop: the first failing step ends the action; earlier writes stay committed.
//! - continue: failing steps are logged and skipped; the action reports ok.
//! - rollback: a transactional action. Record writes are collected and applied at the end
//!   as one RecordStore transaction (`records::write_batch`); a failing step or a failing
//!   commit saves none of them. Steps read the data as it was before the action (pending
//!   writes are not visible to later lookups, and `storeAs` on a created record holds its
//!   values, not its generated key). Navigation, messages and state changes are held back
//!   until the commit succeeds.
//!
//! A declined `confirm` always ends the action (saving nothing in rollback mode). A `fail`
//! step always ends the action with its message as the error, even under onError=continue.
//! Nested `runAction` steps join the caller's transaction when there is one. Updates and
//! deletes send the matched rows' original values (`expected`), so optimistic entities
//! reject them if the row changed since it was read. A row written earlier in the same run
//! carries its post-write values forward as the next write's `expected` (rollback mode,
//! where lookups see the data as it was before the action, and snapshot rows); a row
//! deleted earlier fails the step. On a `customAction` entity they run the entity's action
//! instead (`custom.rs`), joining the caller's transaction like a nested `runAction`. If
//! the writes commit but a sync trigger on them fails, the action reports that failure with
//! the changes saved; held-back effects and the refresh still run.

use super::context::{ActionContext, NavigationTarget, Tone};
use super::custom::{custom_action_for, custom_scope, CustomRequest};
use super::rows::{
    after_write, current_row, match_rows, row_key, with_keys, FoundRow,
};
use super::values::{row_to_object, text_of, to_named_values};
use crate::archive::DocumentConfig;
use crate::automation::{ActionDef, OnError, Step, BEFORE_CHANGE_STEPS};
use crate::data::{DataValue, NamedValue};
use crate::expr::{self, EvaluateOptions};
use crate::records::{self, RecordOperation, RecordWrite, RecordWriteMeta, WriteError};
use crate::trigger_auth::TriggerWrite;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

/// Maximum nesting of `runAction` steps.
pub const MAX_ACTION_DEPTH: usize = 10;

/// One logged step. `path` is nested for steps inside conditions or called actions
/// (`2.then.0`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StepLog {
    pub step_index: usize,
    #[serde(default)]
    pub path: String,
    pub kind: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skipped: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ActionResult {
    pub ok: bool,
    pub error: Option<String>,
    /// A confirm step was declined.
    pub cancelled: bool,
    /// A fail step ended the action (`error` is its message).
    pub aborted: bool,
    pub steps: Vec<StepLog>,
    /// Values stored by steps with `storeAs`.
    pub results: Map<String, Value>,
}

/// The label a step kind has in the editor and in logs.
pub fn step_label(kind: &str) -> &str {
    match kind {
        "createRecord" => "Create record",
        "updateRecord" => "Update record",
        "deleteRecord" => "Delete record",
        "runQuery" => "Run query",
        "navigate" => "Navigate",
        "openForm" => "Open form",
        "closeForm" => "Close popup form",
        "openReport" => "Open report",
        "openDashboard" => "Open page",
        "setState" => "Set state",
        "confirm" => "Confirm",
        "message" => "Show message",
        "condition" => "If / else",
        "runAction" => "Run action",
        "fail" => "Fail with message",
        "setField" => "Set field (before-change)",
        other => other,
    }
}

#[derive(Debug, Clone)]
enum StepError {
    /// A declined confirm.
    Cancelled,
    /// A `fail` step: a business-rule abort whose message is the action's error.
    Aborted(String),
    /// A step failed; the message already names it.
    Failure(String),
    /// Any other error, not yet wrapped with its step.
    Plain(String),
}

impl StepError {
    fn message(&self) -> String {
        match self {
            StepError::Cancelled => "Cancelled by user".into(),
            StepError::Aborted(m) | StepError::Failure(m) | StepError::Plain(m) => m.clone(),
        }
    }
}

impl From<String> for StepError {
    fn from(message: String) -> Self {
        StepError::Plain(message)
    }
}
impl From<&str> for StepError {
    fn from(message: &str) -> Self {
        StepError::Plain(message.into())
    }
}
impl From<crate::manager::AppError> for StepError {
    fn from(e: crate::manager::AppError) -> Self {
        StepError::Plain(e.message)
    }
}

type StepResult<T = ()> = Result<T, StepError>;
type Effect = Box<dyn FnOnce() -> Result<(), String>>;

/// Writes and UI effects held back until a rollback-mode action commits.
#[derive(Default)]
struct Transaction {
    writes: Vec<RecordWrite>,
    effects: Vec<Effect>,
}

struct Shared {
    logs: RefCell<Vec<StepLog>>,
    wrote: Cell<bool>,
    /// Rows written in this run: their `expected` from now on, or None once deleted.
    seen: RefCell<HashMap<String, Option<Vec<NamedValue>>>>,
}

#[derive(Clone)]
struct Frame {
    ctx: Rc<ActionContext>,
    /// Expression roots: record, old, form, app, params, results, steps.
    scope: Rc<RefCell<Map<String, Value>>>,
    shared: Rc<Shared>,
    on_error: OnError,
    tx: Option<Rc<RefCell<Transaction>>>,
    stack: Vec<String>,
}

struct Outcome {
    ok: bool,
    error: Option<String>,
    cancelled: bool,
    aborted: bool,
}

impl Outcome {
    fn ok() -> Self {
        Self {
            ok: true,
            error: None,
            cancelled: false,
            aborted: false,
        }
    }
    fn failed(error: String) -> Self {
        Self {
            ok: false,
            error: Some(error),
            cancelled: false,
            aborted: false,
        }
    }
}

fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Runs the action with this id against `ctx`. Never fails: problems are in the result.
pub fn run_action_by_id(id: &str, ctx: ActionContext) -> ActionResult {
    match ctx.config.actions.iter().find(|a| a.id == id).cloned() {
        Some(def) => run_action(&def, ctx),
        None => ActionResult {
            error: Some(format!("Action {id} does not exist")),
            ..Default::default()
        },
    }
}

/// Runs an action against `ctx`. Never fails: problems are in the result.
pub fn run_action(def: &ActionDef, ctx: ActionContext) -> ActionResult {
    if let Some(authorize) = &ctx.authorize {
        if !authorize("action", &def.id, "execute") {
            return ActionResult {
                error: Some("Not permitted".into()),
                ..Default::default()
            };
        }
    }
    let object = |m: &Option<Map<String, Value>>| Value::Object(m.clone().unwrap_or_default());
    let mut scope = Map::new();
    scope.insert(
        "record".into(),
        ctx.record.clone().map_or(Value::Null, Value::Object),
    );
    scope.insert(
        "old".into(),
        ctx.old.clone().map_or(Value::Null, Value::Object),
    );
    scope.insert("form".into(), object(&ctx.form));
    scope.insert("app".into(), Value::Object(ctx.app.clone()));
    scope.insert("params".into(), object(&ctx.params));
    scope.insert("results".into(), Value::Object(Map::new()));
    scope.insert("steps".into(), Value::Object(Map::new()));
    let mut seen = HashMap::new();
    if let Some(current) = &ctx.current {
        seen.insert(
            row_key(&current.table, &current.identity),
            Some(current.expected.clone()),
        );
    }
    let frame = Frame {
        ctx: Rc::new(ctx),
        scope: Rc::new(RefCell::new(scope)),
        shared: Rc::new(Shared {
            logs: RefCell::new(Vec::new()),
            wrote: Cell::new(false),
            seen: RefCell::new(seen),
        }),
        on_error: def.on_error,
        tx: None,
        stack: Vec::new(),
    };
    let outcome = execute(def, &frame, "");
    if frame.shared.wrote.get() {
        frame.ctx.host.refresh();
    }
    let results = match frame.scope.borrow().get("results") {
        Some(Value::Object(m)) => m.clone(),
        _ => Map::new(),
    };
    let steps = frame.shared.logs.borrow().clone();
    ActionResult {
        ok: outcome.ok,
        error: outcome.error,
        cancelled: outcome.cancelled,
        aborted: outcome.aborted,
        steps,
        results,
    }
}

fn execute(action: &ActionDef, parent: &Frame, prefix: &str) -> Outcome {
    let own = (action.on_error == OnError::Rollback && parent.tx.is_none())
        .then(|| Rc::new(RefCell::new(Transaction::default())));
    let mut stack = parent.stack.clone();
    stack.push(action.id.clone());
    let frame = Frame {
        on_error: action.on_error,
        tx: parent.tx.clone().or_else(|| own.clone()),
        stack,
        ..parent.clone()
    };
    if let Err(e) = run_steps(&action.steps, &frame, prefix) {
        let unsaved = match &own {
            Some(tx) if !tx.borrow().writes.is_empty() => "; no record changes were saved",
            _ => "",
        };
        return Outcome {
            ok: false,
            error: Some(format!("{}{unsaved}", e.message())),
            cancelled: matches!(e, StepError::Cancelled),
            aborted: matches!(e, StepError::Aborted(_)),
        };
    }
    let Some(own) = own else {
        return Outcome::ok();
    };
    let (writes, effects) = {
        let mut tx = own.borrow_mut();
        (std::mem::take(&mut tx.writes), std::mem::take(&mut tx.effects))
    };
    let mut trigger_error = None;
    if !writes.is_empty() {
        match records::write_batch(&frame.ctx.label, writes) {
            Ok(_) => frame.shared.wrote.set(true),
            Err(e) if !e.committed => {
                return Outcome::failed(format!(
                    "Transaction failed: {}; no record changes were saved",
                    e.error.message
                ))
            }
            Err(e) => {
                frame.shared.wrote.set(true);
                trigger_error = Some(format!(
                    "The record changes were saved, but {}",
                    lower_first(&e.error.message)
                ));
            }
        }
    }
    for effect in effects {
        if let Err(e) = effect() {
            return Outcome::failed(format!("{e} (after the record changes were saved)"));
        }
    }
    match trigger_error {
        Some(error) => Outcome::failed(error),
        None => Outcome::ok(),
    }
}

fn run_steps(steps: &[Step], frame: &Frame, prefix: &str) -> StepResult {
    for (index, step) in steps.iter().enumerate() {
        let path = if prefix.is_empty() {
            index.to_string()
        } else {
            format!("{prefix}.{index}")
        };
        let slot = {
            let mut logs = frame.shared.logs.borrow_mut();
            logs.push(StepLog {
                step_index: index,
                path: path.clone(),
                kind: step.kind.clone(),
                ok: true,
                skipped: false,
                error: None,
                duration_ms: 0,
            });
            logs.len() - 1
        };
        let started = Instant::now();
        let result = (|| -> StepResult<bool> {
            if step.kind != "condition" {
                if let Some(when) = step.when.as_deref().filter(|w| !w.trim().is_empty()) {
                    if !condition(when, frame)? {
                        return Ok(false);
                    }
                }
            }
            run_step(step, frame, &path)?;
            Ok(true)
        })();
        let elapsed = started.elapsed().as_millis() as u64;
        let mut failure = None;
        {
            let mut logs = frame.shared.logs.borrow_mut();
            let log = &mut logs[slot];
            log.duration_ms = elapsed;
            match &result {
                Ok(false) => log.skipped = true,
                Ok(true) => {}
                Err(e) => {
                    log.ok = false;
                    log.error = Some(e.message());
                    failure = Some(e.clone());
                }
            }
        }
        if let Some(e) = failure {
            match e {
                StepError::Cancelled | StepError::Aborted(_) => return Err(e),
                _ if frame.on_error != OnError::Continue => {
                    return Err(match e {
                        StepError::Failure(_) => e,
                        other => StepError::Failure(format!(
                            "Step {} ({}) failed: {}",
                            index + 1,
                            step.kind,
                            other.message()
                        )),
                    })
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn opts(frame: &Frame) -> EvaluateOptions {
    EvaluateOptions {
        now: frame.ctx.now,
    }
}

fn expression(src: Option<&str>, frame: &Frame, what: &str) -> StepResult<Value> {
    let Some(src) = src.filter(|s| !s.trim().is_empty()) else {
        return Err(format!("{what} has no expression").into());
    };
    let scope = frame.scope.borrow();
    expr::evaluate(src, &scope, opts(frame)).map_err(|e| StepError::Plain(e.message))
}

fn condition(src: &str, frame: &Frame) -> StepResult<bool> {
    let scope = frame.scope.borrow();
    expr::evaluate_boolean(src, &scope, opts(frame)).map_err(|e| StepError::Plain(e.message))
}

fn eval_map(map: Option<&Value>, frame: &Frame) -> StepResult<Map<String, Value>> {
    let mut out = Map::new();
    if let Some(Value::Object(entries)) = map {
        for (key, src) in entries {
            let value = expression(src.as_str(), frame, &format!("Value for {key}"))?;
            out.insert(key.clone(), value);
        }
    }
    Ok(out)
}

fn field<'a>(step: &'a Step, key: &str) -> &'a str {
    step.fields.get(key).and_then(Value::as_str).unwrap_or("")
}

fn authorize(frame: &Frame, kind: &str, id: &str, op: &str) -> StepResult {
    match &frame.ctx.authorize {
        Some(check) if !check(kind, id, op) => Err("Not permitted".into()),
        _ => Ok(()),
    }
}

/// An app-mode trigger's proof for one step's writes and lookups (`trigger_auth.rs`).
fn step_auth(frame: &Frame, step: &Step) -> Option<TriggerWrite> {
    frame.ctx.trigger_auth.clone().map(|mut auth| {
        auth.step_id = step.id.clone();
        auth
    })
}

fn write_meta(
    frame: &Frame,
    step: &Step,
    old: Option<Map<String, Value>>,
    expected: Option<Vec<NamedValue>>,
) -> RecordWriteMeta {
    RecordWriteMeta {
        trigger_depth: frame.ctx.trigger_depth,
        direct: frame.ctx.direct_writes,
        old,
        expected,
        trigger: step_auth(frame, step),
        ..Default::default()
    }
}

/// Runs a direct (non-transactional) write; a committed write counts even if a trigger failed.
fn direct<T>(frame: &Frame, run: impl FnOnce() -> Result<T, WriteError>) -> StepResult<T> {
    match run() {
        Ok(value) => {
            frame.shared.wrote.set(true);
            Ok(value)
        }
        Err(e) if e.committed => {
            frame.shared.wrote.set(true);
            Err(format!(
                "The record was saved, but {}",
                lower_first(&e.error.message)
            )
            .into())
        }
        Err(e) => Err(e.error.message.into()),
    }
}

/// Runs a UI effect now, or after the commit inside a transaction.
fn effect(frame: &Frame, run: impl FnOnce() -> Result<(), String> + 'static) -> StepResult {
    match &frame.tx {
        Some(tx) => {
            tx.borrow_mut().effects.push(Box::new(run));
            Ok(())
        }
        None => run().map_err(StepError::Plain),
    }
}

/// The custom action an update or delete step routes to (none inside a custom action).
fn routed_to(frame: &Frame, table: &str) -> StepResult<Option<ActionDef>> {
    if frame.ctx.direct_writes {
        return Ok(None);
    }
    Ok(custom_action_for(&frame.ctx.config, table)?)
}

/// Runs a custom-action entity's action for one row, joining the caller's transaction.
fn run_custom(
    action: &ActionDef,
    frame: &Frame,
    path: &str,
    request: CustomRequest,
    identity: &[DataValue],
) -> StepResult {
    if frame.stack.len() >= MAX_ACTION_DEPTH {
        return Err(format!("Actions nested deeper than {MAX_ACTION_DEPTH}").into());
    }
    // Like a form save routed to it (custom.rs), the role must be allowed to run it.
    authorize(frame, "action", &action.id, "execute")?;
    let scope = custom_scope(&request);
    let mut child_ctx = (*frame.ctx).clone();
    child_ctx.direct_writes = true;
    child_ctx.snapshot = Some(request.old.clone());
    let mut child_scope = frame.scope.borrow().clone();
    for (key, value) in scope {
        child_scope.insert(key, value);
    }
    child_scope.insert("form".into(), Value::Object(Map::new()));
    child_scope.insert("results".into(), Value::Object(Map::new()));
    child_scope.insert("steps".into(), Value::Object(Map::new()));
    let child = Frame {
        ctx: Rc::new(child_ctx),
        scope: Rc::new(RefCell::new(child_scope)),
        ..frame.clone()
    };
    if let Some(expected) = &request.expected {
        frame
            .shared
            .seen
            .borrow_mut()
            .insert(row_key(&request.table, identity), Some(expected.clone()));
    }
    let outcome = execute(action, &child, &format!("{path}.custom"));
    finish_nested(outcome, action)
}

fn finish_nested(outcome: Outcome, action: &ActionDef) -> StepResult {
    if outcome.cancelled {
        return Err(StepError::Cancelled);
    }
    let message = outcome
        .error
        .unwrap_or_else(|| format!("Action {} failed", action.name));
    if outcome.aborted {
        return Err(StepError::Aborted(message));
    }
    if !outcome.ok {
        return Err(message.into());
    }
    Ok(())
}

fn query_ops(kind: crate::archive::ActionKind) -> &'static [&'static str] {
    use crate::archive::ActionKind::*;
    match kind {
        Insert => &["create"],
        Update => &["update"],
        Delete => &["delete"],
        Replace => &["delete", "create"],
    }
}

/// A runQuery step on an action query: it writes at once, so it cannot join a rollback-mode
/// action's batch. `storeAs` holds `{ changed, removed }`.
fn run_action_query_step(
    frame: &Frame,
    step: &Step,
    action: &crate::archive::ActionSpec,
) -> StepResult {
    if frame.tx.is_some() {
        return Err("Action queries write at once, so they cannot run inside an action that rolls back on error".into());
    }
    for op in query_ops(action.kind) {
        authorize(frame, "table", &action.table, op)?;
    }
    let params = to_named_values(&eval_map(step.fields.get("params"), frame)?);
    let run = direct(frame, || {
        records::run_action_query(
            &frame.ctx.label,
            field(step, "queryId"),
            params,
            false,
            frame.ctx.trigger_depth,
        )
    })?;
    store(frame, field(step, "storeAs"), {
        let mut counts = Map::new();
        counts.insert("changed".into(), Value::from(run.changed));
        counts.insert("removed".into(), Value::from(run.removed));
        Value::Object(counts)
    });
    Ok(())
}

/// Stores a step's result under `results` (and its `steps` alias).
fn store(frame: &Frame, key: &str, value: Value) {
    if key.trim().is_empty() {
        return;
    }
    let mut scope = frame.scope.borrow_mut();
    for name in ["results", "steps"] {
        if let Some(Value::Object(map)) = scope.get_mut(name) {
            map.insert(key.to_string(), value.clone());
        }
    }
}

fn record_of(frame: &Frame) -> Option<Map<String, Value>> {
    match frame.scope.borrow().get("record") {
        Some(Value::Object(m)) => Some(m.clone()),
        _ => None,
    }
}

fn find_rows(
    table: &str,
    spec: Option<&Value>,
    frame: &Frame,
    trigger: Option<&TriggerWrite>,
) -> StepResult<Vec<FoundRow>> {
    let rows = if spec.and_then(Value::as_str) == Some("current") {
        current_row(
            &frame.ctx.label,
            table,
            record_of(frame).as_ref(),
            frame.ctx.snapshot.as_ref(),
            trigger,
        )?
    } else {
        let criteria = eval_map(spec, frame)?;
        if criteria.is_empty() {
            return Err("Match has no columns".into());
        }
        match_rows(&frame.ctx.label, table, &criteria, trigger)?
    };
    let seen = frame.shared.seen.borrow();
    rows.into_iter()
        .map(|row| match seen.get(&row_key(table, &row.identity)) {
            Some(None) => Err(format!("A {table} row was already deleted by this action").into()),
            Some(Some(known)) => Ok(FoundRow {
                expected: known.clone(),
                ..row
            }),
            None => Ok(row),
        })
        .collect()
}

/// Records a row this run wrote. Lookups after an immediate write re-read the row (seeing
/// trigger changes too), so its entry is dropped; rollback-mode and snapshot rows keep
/// their post-write values as the next `expected`.
fn remember(frame: &Frame, table: &str, row: &FoundRow, next: Option<Vec<NamedValue>>) {
    let key = row_key(table, &row.identity);
    let mut seen = frame.shared.seen.borrow_mut();
    if next.is_none() || frame.tx.is_some() || !row.fresh {
        seen.insert(key, next);
    } else {
        seen.remove(&key);
    }
}

fn record_write(
    operation: RecordOperation,
    table: &str,
    values: Vec<NamedValue>,
    identity: Option<Vec<DataValue>>,
    meta: RecordWriteMeta,
) -> RecordWrite {
    RecordWrite {
        operation,
        table: table.into(),
        values,
        identity,
        meta,
    }
}

fn run_step(step: &Step, frame: &Frame, path: &str) -> StepResult {
    let ctx = &frame.ctx;
    if ctx.before_change.is_some() {
        refuse_in_before_change(step, &ctx.config)?;
    }
    match step.kind.as_str() {
        "createRecord" => {
            let table = field(step, "table");
            authorize(frame, "table", table, "create")?;
            let values = eval_map(step.fields.get("values"), frame)?;
            let named = to_named_values(&values);
            let meta = write_meta(frame, step, None, None);
            let store_as = field(step, "storeAs");
            if let Some(tx) = &frame.tx {
                tx.borrow_mut()
                    .writes
                    .push(record_write(RecordOperation::Insert, table, named, None, meta));
                store(frame, store_as, Value::Object(values));
                return Ok(());
            }
            let result = direct(frame, || {
                records::write(
                    &ctx.label,
                    record_write(RecordOperation::Insert, table, named, None, meta),
                )
            })?;
            if !store_as.is_empty() {
                let identity = match result {
                    records::WriteResult::Inserted(identity) => identity,
                    records::WriteResult::Changed(_) => Vec::new(),
                };
                let keyed = with_keys(
                    &ctx.label,
                    table,
                    &values,
                    &identity,
                    step_auth(frame, step).as_ref(),
                );
                store(frame, store_as, Value::Object(keyed));
            }
            Ok(())
        }
        "updateRecord" => {
            let table = field(step, "table");
            authorize(frame, "table", table, "update")?;
            let auth = step_auth(frame, step);
            let rows = find_rows(table, step.fields.get("match"), frame, auth.as_ref())?;
            let values = eval_map(step.fields.get("values"), frame)?;
            let named = to_named_values(&values);
            let custom = routed_to(frame, table)?;
            for row in rows {
                let meta = write_meta(frame, step, Some(row.object.clone()), Some(row.expected.clone()));
                if let Some(action) = &custom {
                    run_custom(
                        action,
                        frame,
                        path,
                        CustomRequest {
                            operation: "update".into(),
                            table: table.into(),
                            values: named.clone(),
                            old: row.object.clone(),
                            expected: Some(row.expected.clone()),
                        },
                        &row.identity,
                    )?;
                } else {
                    let write = record_write(
                        RecordOperation::Update,
                        table,
                        named.clone(),
                        Some(row.identity.clone()),
                        meta,
                    );
                    match &frame.tx {
                        Some(tx) => tx.borrow_mut().writes.push(write),
                        None => {
                            direct(frame, || records::write(&ctx.label, write))?;
                        }
                    }
                    remember(frame, table, &row, Some(after_write(&row.expected, &named)));
                }
            }
            if step.fields.get("match").and_then(Value::as_str) == Some("current") {
                if let Some(Value::Object(record)) = frame.scope.borrow_mut().get_mut("record") {
                    for (key, value) in values {
                        record.insert(key, value);
                    }
                }
            }
            Ok(())
        }
        "deleteRecord" => {
            let table = field(step, "table");
            authorize(frame, "table", table, "delete")?;
            let auth = step_auth(frame, step);
            let rows = find_rows(table, step.fields.get("match"), frame, auth.as_ref())?;
            let custom = routed_to(frame, table)?;
            for row in rows {
                let meta = write_meta(frame, step, Some(row.object.clone()), Some(row.expected.clone()));
                if let Some(action) = &custom {
                    run_custom(
                        action,
                        frame,
                        path,
                        CustomRequest {
                            operation: "delete".into(),
                            table: table.into(),
                            values: Vec::new(),
                            old: row.object.clone(),
                            expected: Some(row.expected.clone()),
                        },
                        &row.identity,
                    )?;
                } else {
                    let write = record_write(
                        RecordOperation::Delete,
                        table,
                        Vec::new(),
                        Some(row.identity.clone()),
                        meta,
                    );
                    match &frame.tx {
                        Some(tx) => tx.borrow_mut().writes.push(write),
                        None => {
                            direct(frame, || records::write(&ctx.label, write))?;
                        }
                    }
                    remember(frame, table, &row, None);
                }
            }
            Ok(())
        }
        "runQuery" => {
            let query_id = field(step, "queryId");
            let action = ctx
                .config
                .saved_queries
                .iter()
                .find(|q| q.id == query_id)
                .and_then(|q| q.action.clone());
            if let Some(action) = action {
                return run_action_query_step(frame, step, &action);
            }
            authorize(frame, "query", query_id, "read")?;
            let params = to_named_values(&eval_map(step.fields.get("params"), frame)?);
            let run = tauri::async_runtime::block_on(crate::queries::run_saved_query(
                ctx.label.clone(),
                query_id.into(),
                params,
                None,
                None,
            ))?;
            let rows: Vec<Value> = run
                .rows
                .iter()
                .map(|row| Value::Object(row_to_object(&run.columns, row)))
                .collect();
            store(frame, field(step, "storeAs"), Value::Array(rows));
            Ok(())
        }
        "navigate" => {
            let target = step.fields.get("target").and_then(Value::as_object);
            let get = |key: &str| {
                target
                    .and_then(|t| t.get(key))
                    .and_then(Value::as_str)
                    .unwrap_or("")
            };
            if get("id").is_empty() {
                return Err("Navigation target is not set".into());
            }
            exists(&ctx.config, get("kind"), get("id"))?;
            let mut to = NavigationTarget::new(get("kind"), get("id"));
            if !get("mode").is_empty() {
                to.mode = Some(get("mode").into());
            }
            if !get("recordId").trim().is_empty() {
                to.record_id = Some(expression(Some(get("recordId")), frame, "Record id")?);
            }
            let host = ctx.host.clone();
            effect(frame, move || host.navigate(&to))
        }
        "openForm" => {
            let form_id = field(step, "formId");
            exists(&ctx.config, "form", form_id)?;
            let mut to = NavigationTarget::new("form", form_id);
            if !field(step, "mode").is_empty() {
                to.mode = Some(field(step, "mode").into());
            }
            if !field(step, "recordId").trim().is_empty() {
                to.record_id = Some(expression(Some(field(step, "recordId")), frame, "Record id")?);
            }
            if step.fields.get("popup").and_then(Value::as_bool) == Some(true) {
                // It opens at once, even in a rollback-mode action: the caller needs its
                // value, and the popup's own saves commit on their own.
                let value = match ctx.host.open_popup(&to)? {
                    Some(value) => value,
                    None => {
                        ctx.host.navigate(&to)?;
                        Value::Null
                    }
                };
                store(frame, field(step, "storeAs"), value);
                return Ok(());
            }
            let host = ctx.host.clone();
            effect(frame, move || host.navigate(&to))
        }
        "closeForm" => {
            let value = if field(step, "value").trim().is_empty() {
                Value::Null
            } else {
                expression(Some(field(step, "value")), frame, "Return value")?
            };
            let host = ctx.host.clone();
            effect(frame, move || {
                host.close_form(&value);
                Ok(())
            })
        }
        "openReport" => {
            let id = field(step, "reportId");
            exists(&ctx.config, "report", id)?;
            let params = eval_map(step.fields.get("params"), frame)?;
            let mut to = NavigationTarget::new("report", id);
            to.params = Some(params);
            let host = ctx.host.clone();
            effect(frame, move || host.navigate(&to))
        }
        "openDashboard" => {
            let id = field(step, "dashboardId");
            exists(&ctx.config, "page", id)?;
            let params = eval_map(step.fields.get("params"), frame)?;
            let mut to = NavigationTarget::new("page", id);
            to.params = Some(params);
            let host = ctx.host.clone();
            effect(frame, move || host.navigate(&to))
        }
        "setState" => {
            let key = field(step, "key");
            if key.trim().is_empty() {
                return Err("State key is not set".into());
            }
            let value = expression(step.fields.get("value").and_then(Value::as_str), frame, &format!("State {key}"))?;
            let scope_name = if field(step, "scope") == "form" { "form" } else { "app" };
            let host = ctx.host.clone();
            {
                let (scope_name, key, value) = (scope_name.to_string(), key.to_string(), value.clone());
                effect(frame, move || host.set_state(&scope_name, &key, &value))?;
            }
            let mut scope = frame.scope.borrow_mut();
            if let Some(Value::Object(bucket)) = scope.get_mut(scope_name) {
                bucket.insert(key.to_string(), value);
            }
            Ok(())
        }
        "confirm" => {
            let text = expression(step.fields.get("message").and_then(Value::as_str), frame, "Confirmation message")?;
            if !ctx.host.confirm(&text_of(&text))? {
                return Err(StepError::Cancelled);
            }
            Ok(())
        }
        "message" => {
            let text = text_of(&expression(step.fields.get("text").and_then(Value::as_str), frame, "Message")?);
            let tone = Tone::from_key(field(step, "tone"));
            let host = ctx.host.clone();
            effect(frame, move || {
                host.notify(&text, tone);
                Ok(())
            })
        }
        "condition" => {
            let when = step.when.as_deref().unwrap_or("");
            if when.trim().is_empty() {
                return Err("Condition has no expression".into());
            }
            let branch = if condition(when, frame)? { "then" } else { "else" };
            let steps: Vec<Step> = match step.fields.get(branch) {
                Some(Value::Array(items)) => items
                    .iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect(),
                _ => Vec::new(),
            };
            run_steps(&steps, frame, &format!("{path}.{branch}"))
        }
        "runAction" => {
            let id = field(step, "actionId");
            let Some(child) = ctx.config.actions.iter().find(|a| a.id == id) else {
                return Err(format!("Action {id} does not exist").into());
            };
            if frame.stack.contains(&child.id) {
                let name = |id: &str| {
                    ctx.config
                        .actions
                        .iter()
                        .find(|a| a.id == id)
                        .map_or(id.to_string(), |a| a.name.clone())
                };
                let chain: Vec<String> = frame
                    .stack
                    .iter()
                    .map(|id| name(id))
                    .chain(std::iter::once(name(&child.id)))
                    .collect();
                return Err(format!("Recursive action call: {}", chain.join(" → ")).into());
            }
            if frame.stack.len() >= MAX_ACTION_DEPTH {
                return Err(format!("Actions nested deeper than {MAX_ACTION_DEPTH}").into());
            }
            authorize(frame, "action", &child.id, "execute")?;
            let outcome = execute(child, frame, &format!("{path}.action"));
            finish_nested(outcome, child)
        }
        "fail" => {
            let text = expression(step.fields.get("message").and_then(Value::as_str), frame, "Failure message")?;
            let text = text_of(&text);
            Err(StepError::Aborted(if text.is_empty() {
                "The action failed".into()
            } else {
                text
            }))
        }
        "setField" => {
            let Some(set) = &ctx.before_change else {
                return Err("Set field steps only run in before-change triggers".into());
            };
            let name = field(step, "field");
            if name.trim().is_empty() {
                return Err("Field is not set".into());
            }
            let value = expression(step.fields.get("value").and_then(Value::as_str), frame, &format!("Value for {name}"))?;
            set.lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(name.to_string(), value.clone());
            let mut scope = frame.scope.borrow_mut();
            let mut record = match scope.get("record") {
                Some(Value::Object(m)) => m.clone(),
                _ => Map::new(),
            };
            record.insert(name.to_string(), value);
            scope.insert("record".into(), Value::Object(record));
            Ok(())
        }
        other => Err(format!("Unknown step kind {other}").into()),
    }
}

/// A before-change trigger only computes fields or rejects: it never writes or touches the UI.
fn refuse_in_before_change(step: &Step, config: &DocumentConfig) -> StepResult {
    let action_query = step.kind == "runQuery"
        && config
            .saved_queries
            .iter()
            .any(|q| q.id == field(step, "queryId") && q.action.is_some());
    if BEFORE_CHANGE_STEPS.contains(&step.kind.as_str()) && !action_query {
        return Ok(());
    }
    let what = if action_query {
        "Run action query"
    } else {
        step_label(&step.kind)
    };
    Err(StepError::Aborted(format!(
        "A before-change trigger cannot run \"{what}\" steps; use an after trigger"
    )))
}

fn exists(config: &DocumentConfig, kind: &str, id: &str) -> StepResult {
    if id.is_empty() {
        return Err(format!("No {kind} selected").into());
    }
    let found = match kind {
        "form" => Some(config.design.forms.iter().any(|f| f.id == id)),
        "report" => Some(config.reports.iter().any(|r| r.id == id)),
        "page" => Some(config.pages.iter().any(|d| d.id == id)),
        _ => None,
    };
    match found {
        Some(false) => Err(format!("The {kind} {id} does not exist").into()),
        _ => Ok(()),
    }
}
