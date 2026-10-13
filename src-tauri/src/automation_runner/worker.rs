//! Background job worker: claims due jobs for the open document, runs each job's action
//! with a headless context, and reports the outcome to the queue. It runs only while the
//! app is open (PRD §17.3); jobs left running by a crash are requeued by `jobs.rs` when
//! their lease expires. (Formerly `src/automation/worker.ts`.)

use super::context::{session_authorizer, ActionContext, Authorizer, TriggerEnv};
use super::host::HeadlessHost;
use super::runner::run_action;
use crate::archive::DocumentConfig;
use crate::authz::Op;
use crate::jobs::{Job, JobFilter};
use crate::manager::AppError;
use crate::trigger_auth::TriggerWrite;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

/// Wakes a worker that is waiting for its next poll.
#[derive(Default)]
pub struct Wake {
    pending: Mutex<bool>,
    signal: Condvar,
}

impl Wake {
    pub fn wake(&self) {
        *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.signal.notify_all();
    }
    fn wait(&self, timeout: Duration) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if !*pending {
            pending = self
                .signal
                .wait_timeout(pending, timeout)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        *pending = false;
    }
}

fn wakers() -> std::sync::MutexGuard<'static, HashMap<String, Vec<Arc<Wake>>>> {
    static WAKERS: OnceLock<Mutex<HashMap<String, Vec<Arc<Wake>>>>> = OnceLock::new();
    WAKERS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Wakes the worker(s) of a window right after jobs were enqueued, cancelled or retried.
pub fn notify_jobs_changed(label: &str) {
    if let Some(list) = wakers().get(label) {
        for wake in list {
            wake.wake();
        }
    }
}

/// Whether `role_id` (a job's originating role) allows the operation.
fn role_allows(config: &DocumentConfig, role_id: &str, kind: &str, id: &str, op: &str) -> bool {
    let op = match op {
        "read" | "view" => Op::Read,
        "create" => Op::Create,
        "update" => Op::Update,
        "delete" => Op::Delete,
        "execute" => Op::Execute,
        _ => return false,
    };
    config
        .roles
        .iter()
        .find(|r| r.id == role_id)
        .is_some_and(|role| crate::authz::allows(config, role, kind, id, op, &|_| Vec::new()))
}

/// A context without a user: navigation, confirmation and form state fail clearly. A
/// user-mode job never exceeds the active role nor the role it was created under; an
/// app-mode job (`job` given, its trigger running as the app) presents its lease to Rust
/// instead, which allows only the writes its trigger declares.
pub fn headless_context(
    env: &dyn TriggerEnv,
    payload: &Value,
    host: Arc<HeadlessHost>,
    job: Option<&Job>,
) -> ActionContext {
    let config = env.config();
    let label = env.label();
    let trigger = job.and_then(|j| config.triggers.iter().find(|t| t.id == j.trigger_id));
    let as_app = trigger.is_some_and(|t| !t.runs_as_user());
    let object = |key: &str| match payload.get(key) {
        Some(Value::Object(m)) => Some(m.clone()),
        _ => None,
    };
    let mut ctx = ActionContext::new(label, config.clone(), host);
    ctx.record = object("record");
    ctx.old = object("old");
    ctx.app = env.app();
    ctx.trigger_depth = payload
        .get("triggerDepth")
        .and_then(Value::as_u64)
        .map_or(1, |d| d as u32);
    if let (true, Some(trigger), Some(job)) = (as_app, trigger, job) {
        ctx.trigger_auth = Some(TriggerWrite {
            trigger_id: trigger.id.clone(),
            job_id: Some(job.id.clone()),
            lease_token: Some(job.lease_token.clone().unwrap_or_default()),
            ..Default::default()
        });
    } else {
        let role = payload
            .get("roleId")
            .and_then(Value::as_str)
            .map(String::from);
        let session = session_authorizer(label);
        let authorizer: Authorizer = Arc::new(move |kind, id, op| {
            session(kind, id, op)
                && role
                    .as_deref()
                    .is_none_or(|r| role_allows(&config, r, kind, id, op))
        });
        ctx.authorize = Some(authorizer);
    }
    ctx
}

/// Claims and runs one job. `Ok(None)` when none is due.
pub fn run_next_job(
    env: &dyn TriggerEnv,
    on_job: Option<&(dyn Fn(&Job, bool) + Send + Sync)>,
) -> Result<Option<Job>, AppError> {
    let label = env.label();
    let Some(job) = crate::jobs::claim_next_job(label.into(), None)? else {
        return Ok(None);
    };
    let host = Arc::new(HeadlessHost::default());
    let lease = job.lease_token.clone().unwrap_or_default();
    let finished = (|| -> Result<bool, AppError> {
        let config = env.config();
        let action = config
            .actions
            .iter()
            .find(|a| a.id == job.action_id)
            .cloned()
            .ok_or_else(|| {
                AppError::new("NOT_FOUND", format!("Action {} does not exist", job.action_id))
            })?;
        let ctx = headless_context(env, &job.payload, host.clone(), Some(&job));
        let result = run_action(&action, ctx);
        let log = json!({ "steps": result.steps, "messages": host.messages() });
        if result.ok {
            crate::jobs::complete_job(label.into(), job.id.clone(), lease.clone(), Some(log))?;
            Ok(true)
        } else {
            crate::jobs::fail_job(
                label.into(),
                job.id.clone(),
                lease.clone(),
                result.error.unwrap_or_else(|| "Action failed".into()),
                Some(log),
            )?;
            Ok(false)
        }
    })();
    let ok = match finished {
        Ok(ok) => ok,
        Err(e) => {
            // A job this worker no longer leases (cancelled, expired, re-claimed) refuses its
            // result with STALE_LEASE; anything else is a failed attempt.
            if e.code != "STALE_LEASE" {
                let _ = crate::jobs::fail_job(
                    label.into(),
                    job.id.clone(),
                    lease,
                    e.message,
                    Some(json!({ "messages": host.messages() })),
                );
            }
            false
        }
    };
    if let Some(on_job) = on_job {
        on_job(&job, ok);
    }
    Ok(Some(job))
}

/// True when the window's queue has a queued job (due now or waiting for a retry).
pub fn has_queued_jobs(label: &str) -> bool {
    crate::jobs::list_jobs(
        label.into(),
        Some(JobFilter {
            status: Some("queued".into()),
            limit: Some(1),
        }),
    )
    .is_ok_and(|jobs| !jobs.is_empty())
}

type JobCallback = Arc<dyn Fn(&Job, bool) + Send + Sync>;

/// A running worker thread. Dropping the handle stops it once its current job is done.
pub struct WorkerHandle {
    stop: Arc<AtomicBool>,
    wake: Arc<Wake>,
    label: String,
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.wake.wake();
        if let Some(list) = wakers().get_mut(&self.label) {
            list.retain(|w| !Arc::ptr_eq(w, &self.wake));
        }
    }
}

/// How the worker polls: often while an async trigger is enabled or jobs wait (retries
/// whose backoff has elapsed need a poll), rarely otherwise. It also wakes at once on
/// `notify_jobs_changed`.
#[derive(Clone, Copy)]
pub struct Polling {
    pub busy: Duration,
    pub idle: Duration,
}

impl Default for Polling {
    fn default() -> Self {
        Self {
            busy: Duration::from_secs(1),
            idle: Duration::from_secs(10),
        }
    }
}

/// Starts the worker thread for the environment's window.
pub fn start_worker(
    env: Arc<dyn TriggerEnv>,
    on_job: Option<JobCallback>,
    polling: Polling,
) -> WorkerHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let wake = Arc::new(Wake::default());
    let label = env.label().to_string();
    wakers().entry(label.clone()).or_default().push(wake.clone());
    let handle = WorkerHandle {
        stop: stop.clone(),
        wake: wake.clone(),
        label,
    };
    std::thread::Builder::new()
        .name("ixtable-job-worker".into())
        .spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                // No document, or the queue is unavailable: try again on the next poll.
                while !stop.load(Ordering::SeqCst)
                    && matches!(run_next_job(env.as_ref(), on_job.as_deref()), Ok(Some(_)))
                {}
                let async_enabled = env
                    .config()
                    .triggers
                    .iter()
                    .any(|t| t.enabled && t.mode == crate::automation::TriggerMode::Async);
                let busy = async_enabled || has_queued_jobs(env.label());
                wake.wait(if busy { polling.busy } else { polling.idle });
            }
        })
        .expect("start job worker");
    handle
}
