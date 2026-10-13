//! Trigger hook and job worker tests. Ports of `automation-triggers.test.ts` and the
//! worker half of `automation-write-path.test.ts`, against the real RecordStore and job queue.

use super::context::{ActionHost, ContextBase, TriggerEnv};
use super::tests_support::{action, host, named, obj, step, text, trigger, with_actions, Db};
use super::triggers::{install_triggers, MAX_TRIGGER_DEPTH};
use super::worker::{has_queued_jobs, run_next_job};
use crate::archive::DocumentConfig;
use crate::automation::{ActionDef, Trigger};
use crate::data::DataValue;
use crate::jobs::{Job, JobFilter};
use crate::manager::AppError;
use crate::records::{self, RecordHook, RecordWrite, RecordWriteMeta};
use crate::trigger_auth::TriggerWrite;
use serde_json::{json, Map, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub fn audit_action() -> ActionDef {
    action(
        "audit",
        vec![step(json!({"id": "s1", "kind": "createRecord", "table": "audit", "values": {
            "message": "'order ' & record.id & ': ' & coalesce(old.status, 'new') & ' → ' & record.status"
        }}))],
    )
}

/// `orders` with one open order and an empty `audit`, plus the `audit` action.
pub fn trigger_db() -> Db {
    let db = Db::new();
    db.table("orders", &["id", "status"]);
    db.table("audit", &["id", "message"]);
    db.seed("orders", json!({"id": 1, "status": "open"}));
    db.last_write_wins("orders");
    with_actions(&db, vec![audit_action()]);
    db
}

pub fn set_triggers(db: &Db, triggers: Vec<Trigger>) {
    db.set_config(|c| c.triggers = triggers);
}

pub fn env(db: &Db) -> Arc<dyn TriggerEnv> {
    db.env(host() as Arc<dyn ActionHost>)
}

pub fn messages(db: &Db) -> Vec<Value> {
    db.column("audit", "message")
}

fn insert(db: &Db, status: &str) -> Result<Vec<DataValue>, records::WriteError> {
    records::insert(&db.label, "orders", vec![named("status", text(status))])
}

fn close(db: &Db, meta: RecordWriteMeta) -> Result<u64, records::WriteError> {
    records::update(
        &db.label,
        "orders",
        vec![named("status", text("closed"))],
        vec![DataValue::Integer(1)],
        meta,
    )
}

fn old_open() -> Option<Map<String, Value>> {
    Some(obj(json!({"id": 1, "status": "open"})))
}

pub fn jobs(db: &Db) -> Vec<Job> {
    crate::jobs::list_jobs(db.label.clone(), None).expect("jobs")
}

/// Records every write the hooks see before it is stored.
#[derive(Default)]
pub struct Capture(pub Mutex<Vec<RecordWrite>>);

impl Capture {
    pub fn on(&self, table: &str) -> Vec<RecordWrite> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|w| w.table == table)
            .cloned()
            .collect()
    }
}

impl RecordHook for Capture {
    fn before(&self, write: RecordWrite) -> Result<RecordWrite, AppError> {
        self.0.lock().unwrap().push(write.clone());
        Ok(write)
    }
}

#[test]
fn runs_sync_created_triggers_inside_the_write_and_honors_conditions() {
    let db = trigger_db();
    set_triggers(&db, vec![trigger(json!({"condition": "record.status = 'open'"}))]);
    let _hook = install_triggers(env(&db));
    insert(&db, "open").expect("open");
    insert(&db, "draft").expect("draft");
    assert_eq!(messages(&db), [json!("order 2: new → open")]);
}

#[test]
fn gives_updated_triggers_the_old_values_and_skips_disabled_ones() {
    let db = trigger_db();
    set_triggers(
        &db,
        vec![
            trigger(json!({"event": "updated"})),
            trigger(json!({"id": "t2", "event": "updated", "enabled": false})),
        ],
    );
    let _hook = install_triggers(env(&db));
    close(&db, RecordWriteMeta { old: old_open(), ..Default::default() }).expect("update");
    assert_eq!(messages(&db), [json!("order 1: open → closed")]);
}

#[test]
fn reads_old_values_before_an_update_when_the_caller_does_not_pass_them() {
    let db = trigger_db();
    set_triggers(&db, vec![trigger(json!({"event": "updated"}))]);
    let _hook = install_triggers(env(&db));
    close(&db, RecordWriteMeta::default()).expect("update");
    assert_eq!(messages(&db), [json!("order 1: open → closed")]);
}

#[test]
fn propagates_sync_trigger_failures_to_the_caller_after_the_write_commits() {
    let db = trigger_db();
    with_actions(
        &db,
        vec![action("audit", vec![step(json!({"kind": "message", "text": "1 +"}))])],
    );
    set_triggers(&db, vec![trigger(json!({"name": "Audit orders"}))]);
    let _hook = install_triggers(env(&db));
    let err = insert(&db, "open").unwrap_err();
    assert!(err.committed);
    assert!(err.error.message.contains("Trigger \"Audit orders\" failed"), "{}", err.error.message);
    assert_eq!(db.rows("orders").len(), 2);
}

#[test]
fn stops_trigger_recursion_at_the_depth_limit() {
    let db = trigger_db();
    with_actions(
        &db,
        vec![action(
            "loop",
            vec![step(json!({"kind": "createRecord", "table": "orders", "values": {"status": "'again'"}}))],
        )],
    );
    set_triggers(&db, vec![trigger(json!({"actionId": "loop"}))]);
    let _hook = install_triggers(env(&db));
    let err = insert(&db, "seed").unwrap_err();
    let expected = format!("recursion limit ({MAX_TRIGGER_DEPTH})");
    assert!(err.error.message.contains(&expected), "{}", err.error.message);
    assert_eq!(db.rows("orders").len() as u32, 1 + 1 + MAX_TRIGGER_DEPTH);
}

#[test]
fn enqueues_async_triggers_once_per_write_and_the_worker_runs_them() {
    let db = trigger_db();
    set_triggers(&db, vec![trigger(json!({"mode": "async", "event": "updated"}))]);
    let env = env(&db);
    let _hook = install_triggers(env.clone());
    let write = || {
        close(
            &db,
            RecordWriteMeta { old: old_open(), write_id: "w-1".into(), ..Default::default() },
        )
        .expect("update")
    };
    write();
    write();
    let queued = jobs(&db);
    assert_eq!(queued.len(), 1);
    let job = &queued[0];
    assert_eq!((job.trigger_id.as_str(), job.action_id.as_str()), ("t1", "audit"));
    assert_eq!((job.max_attempts, job.backoff_ms), (3, 10));
    assert_eq!(job.payload["table"], json!("orders"));
    assert_eq!(job.payload["event"], json!("updated"));
    assert_eq!(job.payload["record"], json!({"id": 1, "status": "closed"}));
    // t1:orders:1:updated:<8 hex digits>:w-1
    let parts: Vec<&str> = job.idempotency_key.split(':').collect();
    assert_eq!(parts.len(), 6, "{}", job.idempotency_key);
    assert_eq!(&parts[..4], ["t1", "orders", "1", "updated"]);
    assert!(parts[4].len() == 8 && parts[4].chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(parts[5], "w-1");
    assert!(messages(&db).is_empty());

    let ran = run_next_job(env.as_ref(), None).expect("run").expect("a job was due");
    assert_eq!(ran.id, job.id);
    assert_eq!(messages(&db), [json!("order 1: open → closed")]);
    assert_eq!(jobs(&db)[0].status, "succeeded");
    let attempts = crate::jobs::job_attempts(db.label.clone(), job.id.clone()).expect("attempts");
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].ok, Some(true));
    assert!(attempts[0].log["steps"].is_array());
    assert!(run_next_job(env.as_ref(), None).expect("run").is_none());
}

#[test]
fn uses_a_custom_idempotency_key_expression_and_reports_worker_failures() {
    let db = trigger_db();
    set_triggers(
        &db,
        vec![trigger(json!({"mode": "async", "idempotencyKey": "'order-' & record.id & '-' & trigger.id"}))],
    );
    let env = env(&db);
    let _hook = install_triggers(env.clone());
    insert(&db, "open").expect("insert");
    assert_eq!(jobs(&db)[0].idempotency_key, "order-2-t1");
    with_actions(&db, vec![]);
    run_next_job(env.as_ref(), None).expect("run");
    let failed = &jobs(&db)[0];
    assert_eq!(failed.last_error.as_deref(), Some("Action audit does not exist"));
    assert_eq!(failed.attempts, 1);
}

#[test]
fn enqueues_a_later_identical_write_as_a_new_event() {
    let db = trigger_db();
    set_triggers(&db, vec![trigger(json!({"mode": "async", "event": "updated"}))]);
    let _hook = install_triggers(env(&db));
    for _ in 0..2 {
        close(&db, RecordWriteMeta { old: old_open(), ..Default::default() }).expect("update");
    }
    let queued = jobs(&db);
    assert_eq!(queued.len(), 2);
    assert_ne!(queued[0].idempotency_key, queued[1].idempotency_key);
}

/// Cancels the running job the first time the worker reads the definitions after claiming it.
struct CancelsOnConfig {
    inner: Arc<dyn TriggerEnv>,
    armed: AtomicBool,
}

impl TriggerEnv for CancelsOnConfig {
    fn label(&self) -> &str {
        self.inner.label()
    }
    fn config(&self) -> Arc<DocumentConfig> {
        if self.armed.swap(false, Ordering::SeqCst) {
            let running = JobFilter { status: Some("running".into()), limit: None };
            for job in crate::jobs::list_jobs(self.label().into(), Some(running)).unwrap() {
                crate::jobs::cancel_job(self.label().into(), job.id).unwrap();
            }
        }
        self.inner.config()
    }
    fn context(&self, base: ContextBase) -> super::context::ActionContext {
        self.inner.context(base)
    }
}

#[test]
fn ignores_a_stale_lease_instead_of_recording_a_failure() {
    let db = trigger_db();
    set_triggers(&db, vec![trigger(json!({"mode": "async"}))]);
    let inner = env(&db);
    let _hook = install_triggers(inner.clone());
    insert(&db, "open").expect("insert");
    let env = CancelsOnConfig { inner, armed: AtomicBool::new(true) };
    run_next_job(&env, None).expect("run");
    let job = &jobs(&db)[0];
    assert_eq!(job.status, "cancelled");
    assert_eq!(job.last_error, None);
}

fn audit_writes(capture: &Capture) -> Vec<TriggerWrite> {
    capture
        .on("audit")
        .into_iter()
        .map(|w| w.meta.trigger.expect("a trigger step write"))
        .collect()
}

#[test]
fn app_mode_step_writes_present_the_writes_grant() {
    let db = trigger_db();
    set_triggers(&db, vec![trigger(json!({}))]);
    let capture = Arc::new(Capture::default());
    let _hook = install_triggers(env(&db));
    let _capture = records::register_hook(&db.label, capture.clone());
    insert(&db, "open").expect("insert");
    let auth = audit_writes(&capture);
    assert_eq!(auth.len(), 1);
    assert_eq!((auth[0].trigger_id.as_str(), auth[0].step_id.as_str()), ("t1", "s1"));
    assert!(auth[0].grant.as_deref().is_some_and(|g| !g.is_empty()), "{:?}", auth[0]);
    assert_eq!(messages(&db).len(), 1, "Rust accepted the grant");
}

#[test]
fn user_mode_triggers_run_without_trigger_auth() {
    let db = trigger_db();
    set_triggers(&db, vec![trigger(json!({"runAs": "user"}))]);
    let capture = Arc::new(Capture::default());
    let _hook = install_triggers(env(&db));
    let _capture = records::register_hook(&db.label, capture.clone());
    insert(&db, "open").expect("insert");
    let writes = capture.on("audit");
    assert_eq!(writes.len(), 1);
    assert!(writes[0].meta.trigger.is_none());
    assert_eq!(messages(&db).len(), 1);
}

#[test]
fn app_mode_jobs_present_their_lease_on_step_writes() {
    let db = trigger_db();
    set_triggers(&db, vec![trigger(json!({"mode": "async"}))]);
    let env = env(&db);
    let capture = Arc::new(Capture::default());
    let _hook = install_triggers(env.clone());
    let _capture = records::register_hook(&db.label, capture.clone());
    insert(&db, "open").expect("insert");
    let job = jobs(&db).remove(0);
    run_next_job(env.as_ref(), None).expect("run");
    let auth = audit_writes(&capture);
    assert_eq!(auth.len(), 1);
    assert_eq!((auth[0].trigger_id.as_str(), auth[0].step_id.as_str()), ("t1", "s1"));
    assert_eq!(auth[0].job_id.as_deref(), Some(job.id.as_str()));
    assert!(auth[0].lease_token.as_deref().is_some_and(|t| !t.is_empty()));
    assert_eq!(messages(&db).len(), 1, "Rust accepted the lease");
}

#[test]
fn every_write_gets_its_own_write_id_unless_the_caller_retries_one() {
    let db = trigger_db();
    let capture = Arc::new(Capture::default());
    let _capture = records::register_hook(&db.label, capture.clone());
    let identity = vec![DataValue::Integer(1)];
    let values = || vec![named("status", text("x"))];
    for meta in [
        RecordWriteMeta::default(),
        RecordWriteMeta::default(),
        RecordWriteMeta { write_id: "retry-1".into(), ..Default::default() },
    ] {
        records::update(&db.label, "orders", values(), identity.clone(), meta).expect("update");
    }
    let ids: Vec<String> = capture.on("orders").iter().map(|w| w.meta.write_id.clone()).collect();
    assert_eq!(ids.len(), 3);
    assert!(ids.iter().all(|id| !id.is_empty()));
    assert_ne!(ids[0], ids[1]);
    assert_eq!(ids[2], "retry-1");
}

#[test]
fn sees_queued_jobs_even_when_no_async_trigger_is_enabled() {
    let db = trigger_db();
    assert!(!has_queued_jobs(&db.label));
    crate::jobs::enqueue_job(
        db.label.clone(),
        crate::jobs::EnqueueJob {
            trigger_id: "gone".into(),
            action_id: "audit".into(),
            payload: json!({}),
            idempotency_key: "k1".into(),
            max_attempts: Some(1),
            backoff_ms: Some(0),
        },
    )
    .expect("enqueue");
    assert!(has_queued_jobs(&db.label));
}
