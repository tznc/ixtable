//! The one write path for record changes made by the UI and by automation
//! (formerly `src/lib/records.ts`). Every insert, update, delete, batch and
//! action query runs the record hooks registered for its window (before-change
//! and after-change triggers) and the router (custom-action entities).
//!
//! All functions block: call them from a background task (`ui::task::spawn_call`)
//! and announce `DocBus::records_changed` when the task completes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::data::{DataValue, NamedValue};
use crate::manager::AppError;
use crate::queries::action::{ActionRun, BeforeChange, Override, PendingRow};
use crate::recordstore::commands::TriggeredOutcome;
use crate::recordstore::WriteOp;
use crate::trigger_auth::TriggerWrite;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordOperation {
    Insert,
    Update,
    Delete,
}

/// Workflow context supplied by the caller (automation uses it for trigger semantics).
#[derive(Debug, Clone, Default)]
pub struct RecordWriteMeta {
    /// Trigger nesting depth of the workflow issuing this write (recursion guard).
    pub trigger_depth: u32,
    /// Previous values of the updated row, when known (`old` in trigger scopes).
    pub old: Option<serde_json::Map<String, Value>>,
    /// Values the edit started from; optimistic entities reject with CONFLICT if they changed.
    pub expected: Option<Vec<NamedValue>>,
    /// Identity of this write call; retries pass the same id so async triggers dedupe it.
    pub write_id: String,
    /// Write straight to the store even on a `customAction` entity (steps of a running action).
    pub direct: bool,
    /// Marks the write as a step of an app-mode trigger (see trigger_auth.rs).
    pub trigger: Option<TriggerWrite>,
    /// Set on the one write that stands for a whole action query.
    pub bulk: Option<Arc<ActionRun>>,
}

#[derive(Debug, Clone)]
pub struct RecordWrite {
    pub operation: RecordOperation,
    pub table: String,
    pub values: Vec<NamedValue>,
    pub identity: Option<Vec<DataValue>>,
    pub meta: RecordWriteMeta,
}

impl RecordWrite {
    pub fn insert(table: &str, values: Vec<NamedValue>) -> Self {
        Self::new(RecordOperation::Insert, table, values, None)
    }
    pub fn update(table: &str, values: Vec<NamedValue>, identity: Vec<DataValue>) -> Self {
        Self::new(RecordOperation::Update, table, values, Some(identity))
    }
    pub fn delete(table: &str, identity: Vec<DataValue>) -> Self {
        Self::new(RecordOperation::Delete, table, Vec::new(), Some(identity))
    }
    fn new(
        operation: RecordOperation,
        table: &str,
        values: Vec<NamedValue>,
        identity: Option<Vec<DataValue>>,
    ) -> Self {
        Self {
            operation,
            table: table.to_string(),
            values,
            identity,
            meta: RecordWriteMeta::default(),
        }
    }
    pub fn with_meta(mut self, meta: RecordWriteMeta) -> Self {
        self.meta = meta;
        self
    }
    fn with_write_id(mut self) -> Self {
        if self.meta.write_id.is_empty() {
            self.meta.write_id = uuid::Uuid::new_v4().to_string();
        }
        self
    }
    fn to_op(&self) -> WriteOp {
        let table = self.table.clone();
        let identity = self.identity.clone().unwrap_or_default();
        let expected = self.meta.expected.clone();
        match self.operation {
            RecordOperation::Insert => WriteOp::Insert {
                table,
                values: self.values.clone(),
            },
            RecordOperation::Update => WriteOp::Update {
                table,
                values: self.values.clone(),
                identity,
                expected,
            },
            RecordOperation::Delete => WriteOp::Delete {
                table,
                identity,
                expected,
            },
        }
    }
}

/// What a committed write produced: the new identity for inserts, the affected count otherwise.
#[derive(Debug, Clone, PartialEq)]
pub enum WriteResult {
    Inserted(Vec<DataValue>),
    Changed(u64),
}

impl WriteResult {
    fn of(write: &RecordWrite, outcome: &TriggeredOutcome) -> Self {
        match write.operation {
            RecordOperation::Insert => {
                Self::Inserted(outcome.outcome.identity.clone().unwrap_or_default())
            }
            _ => Self::Changed(outcome.outcome.changed),
        }
    }
    pub fn changed(&self) -> u64 {
        match self {
            Self::Inserted(_) => 1,
            Self::Changed(n) => *n,
        }
    }
}

/// A write error. `committed` is set when the write was saved but an after hook
/// (a sync trigger) failed; `results` then holds the write results.
#[derive(Debug, Clone)]
pub struct WriteError {
    pub error: AppError,
    pub committed: bool,
    pub results: Vec<WriteResult>,
}

impl From<AppError> for WriteError {
    fn from(error: AppError) -> Self {
        Self {
            error,
            committed: false,
            results: Vec::new(),
        }
    }
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

/// Hooks on every record write of one window. Errors from `before`/`before_bulk` abort the write.
pub trait RecordHook: Send + Sync {
    /// Runs before the write; may replace it (before-change triggers).
    fn before(&self, write: RecordWrite) -> Result<RecordWrite, AppError> {
        Ok(write)
    }
    /// Runs before an action query writes its rows; returns the fields to set per row.
    fn before_bulk(
        &self,
        _table: &str,
        _rows: &[PendingRow],
        _depth: u32,
    ) -> Result<Vec<Override>, AppError> {
        Ok(Vec::new())
    }
    /// Runs after the write commits, with its result and app-mode trigger grant.
    fn after(
        &self,
        _write: &RecordWrite,
        _result: &WriteResult,
        _grant: Option<&str>,
    ) -> Result<(), AppError> {
        Ok(())
    }
}

/// Takes over an update or delete instead of writing it (custom-action entities).
/// Returns None to let the write go to the store.
pub trait RecordRouter: Send + Sync {
    fn route(&self, write: &RecordWrite) -> Option<Result<u64, AppError>>;
}

type HookList = Vec<(u64, Arc<dyn RecordHook>)>;

#[derive(Default)]
struct Registry {
    next: u64,
    hooks: HashMap<String, HookList>,
    routers: HashMap<String, (u64, Arc<dyn RecordRouter>)>,
}

fn registry() -> std::sync::MutexGuard<'static, Registry> {
    static REGISTRY: std::sync::OnceLock<Mutex<Registry>> = std::sync::OnceLock::new();
    REGISTRY
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Unregisters a hook or router when dropped.
#[must_use = "dropping the registration removes it"]
pub struct Registration {
    label: String,
    id: u64,
}

impl Drop for Registration {
    fn drop(&mut self) {
        let mut reg = registry();
        if let Some(list) = reg.hooks.get_mut(&self.label) {
            list.retain(|(id, _)| *id != self.id);
        }
        if reg
            .routers
            .get(&self.label)
            .is_some_and(|(id, _)| *id == self.id)
        {
            reg.routers.remove(&self.label);
        }
    }
}

/// Registers a hook on every record write of `label`.
pub fn register_hook(label: &str, hook: Arc<dyn RecordHook>) -> Registration {
    let mut reg = registry();
    reg.next += 1;
    let id = reg.next;
    reg.hooks
        .entry(label.to_string())
        .or_default()
        .push((id, hook));
    Registration {
        label: label.to_string(),
        id,
    }
}

/// Installs the update/delete router of `label` (one at a time; the latest wins).
pub fn set_router(label: &str, router: Arc<dyn RecordRouter>) -> Registration {
    let mut reg = registry();
    reg.next += 1;
    let id = reg.next;
    reg.routers.insert(label.to_string(), (id, router));
    Registration {
        label: label.to_string(),
        id,
    }
}

fn hooks(label: &str) -> Vec<Arc<dyn RecordHook>> {
    registry()
        .hooks
        .get(label)
        .map(|l| l.iter().map(|(_, h)| h.clone()).collect())
        .unwrap_or_default()
}

fn router(label: &str) -> Option<Arc<dyn RecordRouter>> {
    registry().routers.get(label).map(|(_, r)| r.clone())
}

fn before_all(write: RecordWrite, active: &[Arc<dyn RecordHook>]) -> Result<RecordWrite, AppError> {
    active.iter().try_fold(write, |w, hook| hook.before(w))
}

/// Runs the after hooks; a failure becomes a committed WriteError (the writes stay saved).
fn after_commit(
    writes: &[RecordWrite],
    outcomes: &[TriggeredOutcome],
    active: &[Arc<dyn RecordHook>],
) -> Result<Vec<WriteResult>, WriteError> {
    let results: Vec<WriteResult> = writes
        .iter()
        .zip(outcomes)
        .map(|(w, o)| WriteResult::of(w, o))
        .collect();
    let mut failures = Vec::new();
    for ((write, outcome), result) in writes.iter().zip(outcomes).zip(&results) {
        for hook in active {
            if let Err(e) = hook.after(write, result, outcome.trigger_grant.as_deref()) {
                failures.push(e.message);
            }
        }
    }
    if failures.is_empty() {
        return Ok(results);
    }
    Err(WriteError {
        error: AppError::new("TRIGGER_FAILED", failures.join("; ")),
        committed: true,
        results,
    })
}

/// Writes one record through the router and hooks.
pub fn write(label: &str, input: RecordWrite) -> Result<WriteResult, WriteError> {
    let record = input.with_write_id();
    if record.operation != RecordOperation::Insert && !record.meta.direct {
        if let Some(routed) = router(label).and_then(|r| r.route(&record)) {
            return Ok(WriteResult::Changed(routed?));
        }
    }
    let active = hooks(label);
    let record = before_all(record, &active)?;
    let label_s = label.to_string();
    let trigger = record.meta.trigger.clone();
    let expected = record.meta.expected.clone();
    let identity = record.identity.clone().unwrap_or_default();
    let table = record.table.clone();
    let outcome = match record.operation {
        RecordOperation::Insert => {
            crate::recordstore::commands::insert_row(label_s, table, record.values.clone(), trigger)
        }
        RecordOperation::Update => crate::recordstore::commands::update_row(
            label_s,
            table,
            record.values.clone(),
            identity,
            expected,
            trigger,
        ),
        RecordOperation::Delete => {
            crate::recordstore::commands::delete_row(label_s, table, identity, expected, trigger)
        }
    }?;
    let mut results = after_commit(std::slice::from_ref(&record), &[outcome], &active)?;
    Ok(results.remove(0))
}

/// Inserts a row and returns the new row's identity values.
pub fn insert(label: &str, table: &str, values: Vec<NamedValue>) -> Result<Vec<DataValue>, WriteError> {
    match write(label, RecordWrite::insert(table, values))? {
        WriteResult::Inserted(identity) => Ok(identity),
        WriteResult::Changed(_) => Ok(Vec::new()),
    }
}

/// Updates one row by identity and returns the affected row count.
pub fn update(
    label: &str,
    table: &str,
    values: Vec<NamedValue>,
    identity: Vec<DataValue>,
    meta: RecordWriteMeta,
) -> Result<u64, WriteError> {
    let write_ = RecordWrite::update(table, values, identity).with_meta(meta);
    Ok(write(label, write_)?.changed())
}

/// Deletes one row by identity and returns the affected row count.
pub fn delete(
    label: &str,
    table: &str,
    identity: Vec<DataValue>,
    meta: RecordWriteMeta,
) -> Result<u64, WriteError> {
    let write_ = RecordWrite::delete(table, identity).with_meta(meta);
    Ok(write(label, write_)?.changed())
}

/// Applies several writes as one RecordStore transaction: all succeed or none do.
/// Before hooks run for every write first; after hooks run once it commits.
/// Writes are never routed to custom actions here.
pub fn write_batch(label: &str, input: Vec<RecordWrite>) -> Result<Vec<WriteResult>, WriteError> {
    let active = hooks(label);
    let writes = input
        .into_iter()
        .map(|w| before_all(w.with_write_id(), &active))
        .collect::<Result<Vec<_>, _>>()?;
    let ops = writes.iter().map(RecordWrite::to_op).collect();
    let triggers = writes.iter().map(|w| w.meta.trigger.clone()).collect();
    let outcomes =
        crate::recordstore::commands::execute_write_batch(label.to_string(), ops, Some(triggers))?;
    after_commit(&writes, &outcomes, &active)
}

/// Runs a saved action query (docs/decisions/action-queries.md). On a table
/// with before-change triggers the first call writes nothing and returns the
/// pending rows; the `before_bulk` hooks decide on them and a second call
/// writes. Then every hook runs once with a single write standing for the
/// whole query (`meta.bulk`).
pub fn run_action_query(
    label: &str,
    query_id: &str,
    params: Vec<NamedValue>,
    dry_run: bool,
    trigger_depth: u32,
) -> Result<ActionRun, WriteError> {
    let call = |before: Option<BeforeChange>| {
        tauri::async_runtime::block_on(crate::queries::action::run_action_query(
            label.to_string(),
            query_id.to_string(),
            params.clone(),
            Some(dry_run),
            before,
        ))
    };
    let mut run = call(None)?;
    let active = hooks(label);
    if let Some(pending) = run.pending.take() {
        let mut overrides = Vec::new();
        for hook in &active {
            overrides.extend(hook.before_bulk(&run.table, &pending.rows, trigger_depth)?);
        }
        run = call(Some(BeforeChange {
            fingerprint: pending.fingerprint,
            overrides,
        }))?;
    }
    if run.dry_run {
        return Ok(run);
    }
    let operation = if run.rows.updated.is_empty() {
        RecordOperation::Insert
    } else {
        RecordOperation::Update
    };
    let bulk = Arc::new(run.clone());
    let record = RecordWrite {
        operation,
        table: run.table.clone(),
        values: Vec::new(),
        identity: None,
        meta: RecordWriteMeta {
            trigger_depth,
            bulk: Some(bulk),
            ..Default::default()
        },
    }
    .with_write_id();
    let outcome = TriggeredOutcome {
        outcome: crate::recordstore::WriteOutcome {
            changed: run.changed,
            identity: None,
        },
        trigger_grant: None,
    };
    after_commit(&[record], &[outcome], &active)?;
    Ok(run)
}

#[cfg(test)]
#[path = "records_tests.rs"]
mod tests;
