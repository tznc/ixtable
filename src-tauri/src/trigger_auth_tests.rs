// Trigger execution identity tests (included from trigger_auth.rs).
use super::*;
use crate::manager::DocumentManager;
use crate::roles::{ObjectPermission, Permissions, Role};
use serde_json::json;

fn config() -> DocumentConfig {
    let mut c = DocumentConfig::default();
    c.actions = serde_json::from_value(json!([
        {"id": "a-stock", "name": "Stock", "onError": "stop", "steps": [
            {"id": "s-cond", "kind": "condition", "when": "true",
             "then": [{"id": "s-upd", "kind": "updateRecord", "table": "inventory",
                       "match": {"id": "1"}, "values": {"qty": "1"}}],
             "else": []},
            {"id": "s-call", "kind": "runAction", "actionId": "a-audit"}
        ]},
        {"id": "a-audit", "name": "Audit", "onError": "stop", "steps": [
            {"id": "s-log", "kind": "createRecord", "table": "audit_log", "values": {"m": "'x'"}}
        ]}
    ]))
    .unwrap();
    c.triggers = serde_json::from_value(json!([
        {"id": "t-app", "name": "App stock", "table": "orders", "event": "created",
         "actionId": "a-stock", "mode": "sync", "enabled": true},
        {"id": "t-user", "name": "User stock", "table": "orders", "event": "updated",
         "actionId": "a-stock", "mode": "sync", "enabled": true, "runAs": "user"},
        {"id": "t-async", "name": "Async audit", "table": "orders", "event": "created",
         "actionId": "a-audit", "mode": "async", "enabled": true}
    ]))
    .unwrap();
    c
}

fn no_job(_: &str, _: &str) -> Result<crate::jobs::Job, AppError> {
    Err(AppError::new("STALE_LEASE", "no job"))
}

fn step(grant: &str, trigger: &str, step: &str) -> TriggerWrite {
    TriggerWrite {
        trigger_id: trigger.into(),
        step_id: step.into(),
        grant: Some(grant.into()),
        ..Default::default()
    }
}

#[test]
fn grants_cover_only_declared_steps_of_app_mode_triggers_until_released_or_expired() {
    let c = config();
    let now = Instant::now();
    let token = issue("w1", &c, "orders", TriggerEvent::Created, now).unwrap();
    let ok = |auth: &TriggerWrite, table: &str, op: Op, at: Instant| {
        verify("w1", &c, auth, table, op, at, &no_job)
    };
    // Nested branch and called-action steps are declared.
    assert!(ok(
        &step(&token, "t-app", "s-upd"),
        "inventory",
        Op::Update,
        now
    )
    .is_ok());
    assert!(ok(
        &step(&token, "t-app", "s-log"),
        "audit_log",
        Op::Create,
        now
    )
    .is_ok());
    assert!(ok(&step(&token, "t-app", "s-upd"), "inventory", Op::Read, now).is_ok());
    // Wrong table, wrong op, wrong step.
    for (table, op, s) in [
        ("secrets", Op::Update, "s-upd"),
        ("inventory", Op::Delete, "s-upd"),
        ("inventory", Op::Update, "s-log"),
        ("inventory", Op::Update, "s-missing"),
    ] {
        let e = ok(&step(&token, "t-app", s), table, op, now).unwrap_err();
        assert_eq!(e.code, "FORBIDDEN", "{table} {op:?} {s}");
    }
    // Wrong trigger, a user-mode trigger, another window, a forged token.
    assert!(ok(
        &step(&token, "t-async", "s-log"),
        "audit_log",
        Op::Create,
        now
    )
    .is_err());
    assert!(ok(
        &step(&token, "t-user", "s-upd"),
        "inventory",
        Op::Update,
        now
    )
    .is_err());
    assert!(verify(
        "w2",
        &c,
        &step(&token, "t-app", "s-upd"),
        "inventory",
        Op::Update,
        now,
        &no_job
    )
    .is_err());
    assert!(ok(
        &step("forged", "t-app", "s-upd"),
        "inventory",
        Op::Update,
        now
    )
    .is_err());
    let bare = TriggerWrite {
        trigger_id: "t-app".into(),
        step_id: "s-upd".into(),
        ..Default::default()
    };
    assert!(ok(&bare, "inventory", Op::Update, now).is_err());
    // Expiry.
    let later = now + GRANT_TTL + Duration::from_secs(1);
    assert!(ok(
        &step(&token, "t-app", "s-upd"),
        "inventory",
        Op::Update,
        later
    )
    .is_err());
    // Single use: released by its own window only.
    release("w2", &token);
    assert!(ok(
        &step(&token, "t-app", "s-upd"),
        "inventory",
        Op::Update,
        now
    )
    .is_ok());
    release("w1", &token);
    assert!(ok(
        &step(&token, "t-app", "s-upd"),
        "inventory",
        Op::Update,
        now
    )
    .is_err());
}

#[test]
fn each_use_extends_a_grant_so_it_can_serve_many_rows_in_turn() {
    let c = config();
    let now = Instant::now();
    let token = issue("w1", &c, "orders", TriggerEvent::Created, now).unwrap();
    let at = |t: Instant| {
        verify(
            "w1",
            &c,
            &step(&token, "t-app", "s-upd"),
            "inventory",
            Op::Update,
            t,
            &no_job,
        )
    };
    let first = now + GRANT_TTL - Duration::from_secs(1);
    assert!(at(first).is_ok());
    // Past the original expiry, but within a TTL of the last use.
    assert!(at(first + GRANT_TTL - Duration::from_secs(1)).is_ok());
    assert!(at(first + GRANT_TTL * 3).is_err());
    release("w1", &token);
}

#[test]
fn grants_are_issued_only_for_enabled_sync_app_mode_triggers() {
    let mut c = config();
    let now = Instant::now();
    assert!(issue("w", &c, "orders", TriggerEvent::Updated, now).is_none());
    assert!(issue("w", &c, "inventory", TriggerEvent::Created, now).is_none());
    c.triggers[0].enabled = false;
    assert!(issue("w", &c, "orders", TriggerEvent::Created, now).is_none());
    // A grant issued before the trigger was disabled no longer works.
    let c2 = config();
    let token = issue("w", &c2, "orders", TriggerEvent::Created, now).unwrap();
    let auth = step(&token, "t-app", "s-upd");
    assert!(verify("w", &c, &auth, "inventory", Op::Update, now, &no_job).is_err());
}

#[test]
fn async_jobs_authorize_with_their_live_lease() {
    let c = config();
    let now = Instant::now();
    let auth = TriggerWrite {
        trigger_id: "t-async".into(),
        step_id: "s-log".into(),
        job_id: Some("j1".into()),
        lease_token: Some("1:abc".into()),
        ..Default::default()
    };
    let job = |trigger: &'static str| {
        move |id: &str, lease: &str| -> Result<crate::jobs::Job, AppError> {
            if id == "j1" && lease == "1:abc" {
                Ok(crate::jobs::Job {
                    id: id.into(),
                    document_id: "d".into(),
                    trigger_id: trigger.into(),
                    action_id: "a-audit".into(),
                    payload: Value::Null,
                    idempotency_key: "k".into(),
                    status: "running".into(),
                    attempts: 1,
                    max_attempts: 3,
                    backoff_ms: 1,
                    next_run_at: String::new(),
                    lease_until: None,
                    created_at: String::new(),
                    updated_at: String::new(),
                    last_error: None,
                    lease_token: Some(lease.into()),
                })
            } else {
                no_job(id, lease)
            }
        }
    };
    assert!(verify(
        "w",
        &c,
        &auth,
        "audit_log",
        Op::Create,
        now,
        &job("t-async")
    )
    .is_ok());
    assert!(verify("w", &c, &auth, "audit_log", Op::Create, now, &job("t-app")).is_err());
    assert!(verify("w", &c, &auth, "audit_log", Op::Create, now, &no_job).is_err());
    let stale = TriggerWrite {
        lease_token: Some("1:old".into()),
        ..auth.clone()
    };
    assert!(verify(
        "w",
        &c,
        &stale,
        "audit_log",
        Op::Create,
        now,
        &job("t-async")
    )
    .is_err());
}

#[test]
fn updates_of_custom_action_entities_declare_the_routed_action() {
    let mut c = config();
    c.actions.push(
        serde_json::from_value(json!(
            {"id": "a-guard", "name": "Guarded stock", "onError": "stop", "steps": [
                {"id": "s-guard", "kind": "createRecord", "table": "stock_audit", "values": {"m": "'x'"}}
            ]}
        ))
        .unwrap(),
    );
    let now = Instant::now();
    let token = issue("w-custom", &c, "orders", TriggerEvent::Created, now).unwrap();
    let guard = step(&token, "t-app", "s-guard");
    let check = |c: &DocumentConfig| {
        verify(
            "w-custom",
            c,
            &guard,
            "stock_audit",
            Op::Create,
            now,
            &no_job,
        )
    };
    assert_eq!(check(&c).unwrap_err().code, "FORBIDDEN");
    c.entities.push(crate::recordstore::EntitySettings {
        id: "e-inventory".into(),
        table: "inventory".into(),
        concurrency: "customAction".into(),
        action_id: Some("a-guard".into()),
    });
    assert!(check(&c).is_ok());
    let needed = needed_grants(&c, &c.triggers[1]);
    assert!(needed.contains(&("action".into(), "a-guard".into(), Op::Execute)));
    assert!(needed.contains(&("table".into(), "stock_audit".into(), Op::Create)));
    release("w-custom", &token);
}

fn grant(kind: &str, id: &str, write: bool) -> ObjectPermission {
    ObjectPermission {
        kind: kind.into(),
        id: id.into(),
        read: true,
        create: write,
        update: write,
        delete: false,
    }
}

#[test]
fn user_mode_triggers_refuse_the_initiating_write_up_front() {
    let base = std::env::temp_dir().join(format!("ixtable-tauth-{}", uuid::Uuid::new_v4()));
    let m = DocumentManager::new(base.join("data"), base.join("cache")).unwrap();
    m.new_session("w").unwrap();
    let clerk = |objects: Vec<ObjectPermission>, actions: &[&str]| Role {
        id: "clerk".into(),
        name: "Clerk".into(),
        permissions: Permissions {
            navigation: vec![],
            objects,
            actions: actions.iter().map(|a| a.to_string()).collect(),
        },
    };
    let set = |role: Option<Role>| {
        m.with_session("w", |s| {
            s.doc.config = config();
            s.access = role.map_or(crate::authz::Access::Unset, crate::authz::Access::Role);
            Ok(())
        })
        .unwrap()
    };
    let check = |event| m.with_session("w", |s| check_user_triggers(s, "orders", event));
    set(None);
    assert!(check(TriggerEvent::Updated).is_ok());
    set(Some(clerk(
        vec![grant("table", "orders", true)],
        &["a-stock"],
    )));
    // App-mode triggers (created) never block; the user-mode one needs grants.
    assert!(check(TriggerEvent::Created).is_ok());
    let e = check(TriggerEvent::Updated).unwrap_err();
    assert_eq!(e.code, "FORBIDDEN");
    assert!(e.message.contains("User stock") && e.message.contains("inventory"));
    set(Some(clerk(
        vec![
            grant("table", "orders", true),
            grant("table", "inventory", true),
        ],
        &["a-stock"],
    )));
    let e = check(TriggerEvent::Updated).unwrap_err();
    assert!(e.message.contains("a-audit"), "{}", e.message);
    set(Some(clerk(
        vec![
            grant("table", "orders", true),
            grant("table", "inventory", true),
            grant("table", "audit_log", true),
        ],
        &["a-stock", "a-audit"],
    )));
    assert!(check(TriggerEvent::Updated).is_ok());
    let _ = m.close("w", true);
    let _ = std::fs::remove_dir_all(base);
}
