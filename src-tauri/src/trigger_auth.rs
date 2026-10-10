//! Trigger execution identity (docs/decisions/async-trigger-queue.md,
//! cloud-security-model.md).
//!
//! A trigger runs as the app (`runAs` absent or "app", definer context) or as
//! the signed-in user ("user").
//!
//! App mode: when Rust commits an initiating insert/update on a table with
//! enabled sync app-mode triggers for that event, it issues a short-lived
//! grant bound to the window, the trigger ids, the table and the event.
//! The TS runner passes the grant, trigger id and step id on each step write
//! (and on the reads those steps need); Rust accepts the write without role
//! checks only when the grant is live for this window and the trigger is
//! enabled, runs as the app and declares a step with that id, table and
//! operation (nested branches, called actions, and the custom action an
//! update or delete of a `customAction` entity is routed to included). The runner
//! releases the grant when the trigger finishes, so it is single use. Async
//! app-mode jobs authorize with the job id and its current lease token
//! instead. A modified client can still choose the values written, but only
//! into tables and operations the trigger declares.
//!
//! User mode: steps run under the role. Before the initiating write commits,
//! `check_user_triggers` refuses it when the role lacks a permission the
//! trigger's steps need, so a save never commits and then fails half way.
use crate::archive::DocumentConfig;
use crate::authz::Op;
use crate::automation::{all_steps, Step, Trigger, TriggerEvent, TriggerMode};
use crate::manager::AppError;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long a sync trigger grant stays valid.
pub const GRANT_TTL: Duration = Duration::from_secs(120);

/// Proof that a write is a trigger step (sent by the TS runner).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerWrite {
    pub trigger_id: String,
    #[serde(default)]
    pub step_id: String,
    #[serde(default)]
    pub grant: Option<String>,
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub lease_token: Option<String>,
}

#[derive(Debug, Clone)]
struct Grant {
    window: String,
    triggers: Vec<String>,
    table: String,
    event: TriggerEvent,
    expires: Instant,
}

fn grants() -> &'static Mutex<HashMap<String, Grant>> {
    static GRANTS: OnceLock<Mutex<HashMap<String, Grant>>> = OnceLock::new();
    GRANTS.get_or_init(Default::default)
}

fn forbidden(message: String) -> AppError {
    AppError::new("FORBIDDEN", message)
}

fn enabled_for<'a>(
    config: &'a DocumentConfig,
    table: &'a str,
    event: TriggerEvent,
) -> impl Iterator<Item = &'a Trigger> {
    config
        .triggers
        .iter()
        .filter(move |t| t.enabled && t.table == table && t.event == event)
}

/// Issues a grant for the enabled sync app-mode triggers of a committed write;
/// None when there are none.
pub fn issue(
    window: &str,
    config: &DocumentConfig,
    table: &str,
    event: TriggerEvent,
    now: Instant,
) -> Option<String> {
    let triggers: Vec<String> = enabled_for(config, table, event)
        .filter(|t| t.mode == TriggerMode::Sync && !t.runs_as_user())
        .map(|t| t.id.clone())
        .collect();
    if triggers.is_empty() {
        return None;
    }
    let token = uuid::Uuid::new_v4().to_string();
    let mut map = grants().lock().unwrap_or_else(|e| e.into_inner());
    map.retain(|_, g| g.expires > now);
    map.insert(
        token.clone(),
        Grant {
            window: window.into(),
            triggers,
            table: table.into(),
            event,
            expires: now + GRANT_TTL,
        },
    );
    Some(token)
}

/// Ends a grant (the trigger run finished). Only its own window may release it.
pub fn release(window: &str, token: &str) {
    let mut map = grants().lock().unwrap_or_else(|e| e.into_inner());
    if map.get(token).is_some_and(|g| g.window == window) {
        map.remove(token);
    }
}

/// The step kind that performs `op`.
fn step_kind(op: Op) -> Option<&'static str> {
    match op {
        Op::Create => Some("createRecord"),
        Op::Update => Some("updateRecord"),
        Op::Delete => Some("deleteRecord"),
        _ => None,
    }
}

/// The action an update or delete step runs instead of writing, when its table
/// uses the `customAction` concurrency policy (src/automation/custom.ts).
fn routed_action(config: &DocumentConfig, step: &Step) -> Option<String> {
    if !matches!(step.kind.as_str(), "updateRecord" | "deleteRecord") {
        return None;
    }
    crate::recordstore::entity_policy(config, step_table(step))
        .filter(|e| e.concurrency == "customAction")
        .and_then(|e| e.action_id.clone())
}

/// Every step the action runs, following condition branches, runAction calls
/// and the custom actions its updates and deletes are routed to.
pub fn action_steps(config: &DocumentConfig, action_id: &str) -> Vec<Step> {
    let mut out = vec![];
    let mut seen = HashSet::new();
    let mut queue = vec![action_id.to_string()];
    while let Some(id) = queue.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(action) = config.actions.iter().find(|a| a.id == id) else {
            continue;
        };
        for step in all_steps(&action.steps) {
            if step.kind == "runAction" {
                if let Some(child) = step.fields.get("actionId").and_then(Value::as_str) {
                    queue.push(child.to_string());
                }
            }
            queue.extend(routed_action(config, &step));
            out.push(step);
        }
    }
    out
}

fn step_table(step: &Step) -> &str {
    step.fields
        .get("table")
        .and_then(Value::as_str)
        .unwrap_or("")
}

/// Whether the trigger's action declares step `step_id` doing `op` on `table`
/// (`Op::Read`: any record step on that table, for the lookups steps make).
fn declares(
    config: &DocumentConfig,
    trigger: &Trigger,
    step_id: &str,
    table: &str,
    op: Op,
) -> bool {
    action_steps(config, &trigger.action_id).iter().any(|s| {
        s.id == step_id
            && step_table(s) == table
            && match step_kind(op) {
                Some(kind) => s.kind == kind,
                None => {
                    op == Op::Read
                        && matches!(
                            s.kind.as_str(),
                            "createRecord" | "updateRecord" | "deleteRecord"
                        )
                }
            }
    })
}

/// Verifies a trigger step write (or read) for `window` against `config`.
pub fn verify(
    window: &str,
    config: &DocumentConfig,
    auth: &TriggerWrite,
    table: &str,
    op: Op,
    now: Instant,
    job_lease: &dyn Fn(&str, &str) -> Result<crate::jobs::Job, AppError>,
) -> Result<(), AppError> {
    let trigger = config
        .triggers
        .iter()
        .find(|t| t.id == auth.trigger_id && t.enabled)
        .ok_or_else(|| forbidden(format!("Trigger {} is not enabled", auth.trigger_id)))?;
    if trigger.runs_as_user() {
        return Err(forbidden(format!(
            "Trigger \"{}\" runs as the signed-in user",
            trigger.name
        )));
    }
    if !declares(config, trigger, &auth.step_id, table, op) {
        return Err(forbidden(format!(
            "Trigger \"{}\" has no step {} that writes to table \"{table}\"",
            trigger.name, auth.step_id
        )));
    }
    if let Some(token) = &auth.grant {
        let mut map = grants().lock().unwrap_or_else(|e| e.into_inner());
        let grant = map.get_mut(token).filter(|g| {
            g.window == window
                && g.expires > now
                && g.triggers.contains(&trigger.id)
                && g.table == trigger.table
                && g.event == trigger.event
        });
        // Each use extends the grant, so one grant can serve the rows of an action query in turn.
        let live = grant.map(|g| g.expires = now + GRANT_TTL).is_some();
        return if live {
            Ok(())
        } else {
            Err(forbidden(format!(
                "Trigger \"{}\" has no valid grant for this write",
                trigger.name
            )))
        };
    }
    match (&auth.job_id, &auth.lease_token) {
        (Some(job), Some(lease)) if trigger.mode == TriggerMode::Async => {
            let job = job_lease(job, lease)?;
            if job.trigger_id == trigger.id {
                Ok(())
            } else {
                Err(forbidden(format!(
                    "Job {} does not belong to trigger \"{}\"",
                    job.id, trigger.name
                )))
            }
        }
        _ => Err(forbidden(format!(
            "Trigger \"{}\" write has no grant",
            trigger.name
        ))),
    }
}

/// Authorizes an operation: the session's role, else a verified trigger step.
pub fn authorize(
    window: &str,
    table: &str,
    op: Op,
    trigger: Option<&TriggerWrite>,
) -> Result<(), AppError> {
    let by_role = crate::authz::check(window, "table", table, op);
    let (Err(refused), Some(auth)) = (&by_role, trigger) else {
        return by_role;
    };
    let config = crate::manager()?.config(window)?;
    let lease = |job: &str, lease: &str| {
        crate::jobs::store()?.active_lease(
            &crate::jobs::document(window)?,
            job,
            lease,
            chrono::Utc::now(),
        )
    };
    verify(window, &config, auth, table, op, Instant::now(), &lease)
        .map_err(|e| AppError::new(&e.code, format!("{} ({})", refused.message, e.message)))
}

/// The permissions the steps of a user-mode trigger need, as (kind, id, op).
pub fn needed_grants(config: &DocumentConfig, trigger: &Trigger) -> Vec<(String, String, Op)> {
    let mut out = vec![("action".to_string(), trigger.action_id.clone(), Op::Execute)];
    for step in action_steps(config, &trigger.action_id) {
        let field = |k: &str| {
            step.fields
                .get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        match step.kind.as_str() {
            "createRecord" => out.push(("table".into(), field("table"), Op::Create)),
            "updateRecord" => out.push(("table".into(), field("table"), Op::Update)),
            "deleteRecord" => out.push(("table".into(), field("table"), Op::Delete)),
            "runQuery" => {
                let id = field("queryId");
                let action = config
                    .saved_queries
                    .iter()
                    .find(|q| q.id == id)
                    .and_then(|q| q.action.as_ref());
                match action {
                    // An action query needs the table operations it performs.
                    Some(spec) => out.extend(
                        action_query_ops(spec.kind)
                            .iter()
                            .map(|&op| ("table".into(), spec.table.clone(), op)),
                    ),
                    None => out.push(("query".into(), id, Op::Read)),
                }
            }
            "runAction" => out.push(("action".into(), field("actionId"), Op::Execute)),
            _ => {}
        }
        if let Some(custom) = routed_action(config, &step) {
            out.push(("action".into(), custom, Op::Execute));
        }
    }
    out
}

/// The table operations an action query of `kind` performs.
pub fn action_query_ops(kind: crate::archive::ActionKind) -> &'static [Op] {
    use crate::archive::ActionKind::*;
    match kind {
        Insert => &[Op::Create],
        Update => &[Op::Update],
        Delete => &[Op::Delete],
        Replace => &[Op::Delete, Op::Create],
    }
}

fn op_label(op: Op) -> &'static str {
    match op {
        Op::Read => "read",
        Op::Create => "create",
        Op::Update => "update",
        Op::Delete => "delete",
        Op::Execute => "execute",
    }
}

/// Refuses an initiating write up front when a user-mode trigger it fires
/// needs a permission the session's role lacks.
pub fn check_user_triggers(
    s: &crate::manager::Session,
    table: &str,
    event: TriggerEvent,
) -> Result<(), AppError> {
    let Some(role) = crate::authz::effective_role(s) else {
        return Ok(());
    };
    for trigger in enabled_for(&s.doc.config, table, event).filter(|t| t.runs_as_user()) {
        let mut missing: Vec<String> = vec![];
        for (kind, id, op) in needed_grants(&s.doc.config, trigger) {
            let what = format!("{} on {kind} \"{id}\"", op_label(op));
            if crate::authz::check_session(s, &kind, &id, op).is_err() && !missing.contains(&what) {
                missing.push(what);
            }
        }
        if !missing.is_empty() {
            return Err(forbidden(format!(
                "Not saved: trigger \"{}\" runs as the signed-in user, and the role \"{}\" lacks {}",
                trigger.name,
                role.name,
                missing.join(", ")
            )));
        }
    }
    Ok(())
}

/// `check_user_triggers` for the window.
pub fn precheck(window: &str, table: &str, event: TriggerEvent) -> Result<(), AppError> {
    crate::manager()?.with_session(window, |s| check_user_triggers(s, table, event))
}

/// Ends a sync trigger grant once the trigger run finished.
#[tauri::command]
pub fn release_trigger_grant(window_label: String, grant: String) -> Result<(), AppError> {
    release(&window_label, &grant);
    Ok(())
}

#[cfg(test)]
mod tests {
    include!("trigger_auth_tests.rs");
}
