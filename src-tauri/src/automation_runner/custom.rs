//! The `customAction` concurrency policy (PRD §19): an update or delete of such an entity
//! runs the entity's configured action instead of writing the row. The action decides what
//! to write; its own record steps write directly (no re-routing) under the action's
//! onError semantics. Inserts are unaffected. (Formerly `src/automation/custom.ts`.)
//!
//! The action runs with:
//! - `record`: the row as the caller wants it (current values with `changes` applied; the
//!   current row for a delete), so `match: current` targets it;
//! - `old`: the row before the change;
//! - `params.operation` (`update` or `delete`), `params.table`, `params.changes` (requested
//!   column values; `{}` for a delete), and `params.expected` (original values the caller
//!   started from, or null).
//!
//! A `match: current` step on the routed row sends those original values (else the row as
//! read when the write began) as its `expected`, not a fresh read. The action runs under
//! the user's role (which must be allowed to run it), and Rust authorizes each of its
//! writes like any other record write.

use super::context::{ContextBase, CurrentRow, TriggerEnv};
use super::rows::{expected_of, read_row};
use super::runner::run_action;
use super::values::named_to_object;
use crate::archive::DocumentConfig;
use crate::automation::ActionDef;
use crate::data::NamedValue;
use crate::manager::AppError;
use crate::records::{self, RecordOperation, RecordRouter, RecordWrite, Registration};
use serde_json::{Map, Value};
use std::sync::Arc;

/// The action a `customAction` entity routes its updates and deletes to, if any.
pub fn custom_action_for(
    config: &DocumentConfig,
    table: &str,
) -> Result<Option<ActionDef>, String> {
    let Some(entity) = config.entities.iter().find(|e| e.table == table) else {
        return Ok(None);
    };
    if entity.concurrency != "customAction" {
        return Ok(None);
    }
    config
        .actions
        .iter()
        .find(|a| Some(&a.id) == entity.action_id.as_ref())
        .cloned()
        .map(Some)
        .ok_or_else(|| {
            format!("{table} uses a custom concurrency action, but its action does not exist")
        })
}

#[derive(Debug, Clone)]
pub struct CustomRequest {
    /// `update` or `delete`.
    pub operation: String,
    pub table: String,
    pub values: Vec<NamedValue>,
    pub old: Map<String, Value>,
    pub expected: Option<Vec<NamedValue>>,
}

/// The `record`, `old` and `params` a custom action runs with.
pub fn custom_scope(request: &CustomRequest) -> Map<String, Value> {
    let changes = if request.operation == "update" {
        named_to_object(&request.values)
    } else {
        Map::new()
    };
    let mut record = request.old.clone();
    for (key, value) in &changes {
        record.insert(key.clone(), value.clone());
    }
    let mut params = Map::new();
    params.insert("operation".into(), Value::String(request.operation.clone()));
    params.insert("table".into(), Value::String(request.table.clone()));
    params.insert("changes".into(), Value::Object(changes));
    params.insert(
        "expected".into(),
        request
            .expected
            .as_ref()
            .map_or(Value::Null, |e| Value::Object(named_to_object(e))),
    );
    let mut scope = Map::new();
    scope.insert("record".into(), Value::Object(record));
    scope.insert("old".into(), Value::Object(request.old.clone()));
    scope.insert("params".into(), Value::Object(params));
    scope
}

struct CustomRouter(Arc<dyn TriggerEnv>);

impl RecordRouter for CustomRouter {
    fn route(&self, write: &RecordWrite) -> Option<Result<u64, AppError>> {
        if write.operation == RecordOperation::Insert || write.identity.is_none() {
            return None;
        }
        let env = &self.0;
        match custom_action_for(&env.config(), &write.table) {
            Ok(None) => None,
            Ok(Some(action)) => Some(run_custom(&action, write, env.as_ref())),
            Err(e) => Some(Err(AppError::new("ACTION_FAILED", e))),
        }
    }
}

fn run_custom(
    action: &ActionDef,
    write: &RecordWrite,
    env: &dyn TriggerEnv,
) -> Result<u64, AppError> {
    let identity = write.identity.clone().unwrap_or_default();
    let old = write
        .meta
        .old
        .clone()
        .or_else(|| read_row(env.label(), &write.table, &identity))
        .ok_or_else(|| {
            AppError::new(
                "NOT_FOUND",
                format!("The {} record no longer exists", write.table),
            )
        })?;
    let operation = match write.operation {
        RecordOperation::Delete => "delete",
        _ => "update",
    };
    let request = CustomRequest {
        operation: operation.into(),
        table: write.table.clone(),
        values: write.values.clone(),
        old: old.clone(),
        expected: write.meta.expected.clone(),
    };
    let mut scope = custom_scope(&request);
    let as_map = |value: Option<Value>| match value {
        Some(Value::Object(m)) => Some(m),
        _ => None,
    };
    // `match: current` writes against the caller's snapshot, so a concurrent change raises CONFLICT.
    let expected = match &write.meta.expected {
        Some(expected) => expected.clone(),
        None => expected_of(env.label(), &write.table, &old)
            .map_err(|e| AppError::new("ACTION_FAILED", e))?,
    };
    let ctx = env.context(ContextBase {
        record: as_map(scope.remove("record")),
        old: Some(old.clone()),
        params: as_map(scope.remove("params")),
        trigger_depth: write.meta.trigger_depth,
        direct_writes: true,
        snapshot: Some(old),
        current: Some(CurrentRow {
            table: write.table.clone(),
            identity,
            expected,
        }),
        ..Default::default()
    });
    let outcome = run_action(action, ctx);
    if !outcome.ok {
        return Err(AppError::new(
            "ACTION_FAILED",
            outcome
                .error
                .unwrap_or_else(|| format!("Action {} failed", action.name)),
        ));
    }
    Ok(1)
}

/// Routes updates and deletes of custom-action entities (forms, grids) to their action.
pub fn install_custom_actions(env: Arc<dyn TriggerEnv>) -> Registration {
    records::set_router(&env.label().to_string(), Arc::new(CustomRouter(env)))
}
