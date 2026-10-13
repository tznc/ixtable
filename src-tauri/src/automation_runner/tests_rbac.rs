//! Role checks around triggers and custom actions. Port of
//! `tests/integration/runtime-rbac-triggers.test.tsx`: the role is a Runtime role preview
//! of the window's session, and Rust authorizes every write, so these run against the
//! backend with the trigger hook and custom-action router installed.

use super::custom::install_custom_actions;
use super::tests_support::{host, named, text, Db};
use super::triggers::install_triggers;
use crate::manager::AppError;
use crate::records::{self, RecordWriteMeta};
use crate::trigger_auth::TriggerWrite;
use super::context::{ActionContext, ContextBase, TriggerEnv};
use serde_json::{json, Map, Value};
use std::sync::Arc;

/// The environment without the in-app role check, so Rust alone decides (the TypeScript
/// test built its contexts without an authorizer too).
struct RustChecksOnly(Arc<dyn TriggerEnv>);

impl TriggerEnv for RustChecksOnly {
    fn label(&self) -> &str {
        self.0.label()
    }
    fn config(&self) -> Arc<crate::archive::DocumentConfig> {
        self.0.config()
    }
    fn app(&self) -> Map<String, Value> {
        self.0.app()
    }
    fn context(&self, base: ContextBase) -> ActionContext {
        let mut ctx = self.0.context(base);
        ctx.authorize = None;
        ctx
    }
}

fn code_of<T>(result: Result<T, AppError>) -> String {
    match result {
        Ok(_) => "OK".into(),
        Err(e) => e.code,
    }
}

fn preview(db: &Db, role: Option<&str>) {
    crate::authz::set_runtime_role_preview(db.label.clone(), role.map(str::to_string))
        .expect("role preview");
}

// Reads as the unrestricted user, then returns to the clerk.
fn rows(db: &Db, table: &str) -> Vec<Value> {
    preview(db, None);
    let rows = db.rows(table).into_iter().map(Value::Object).collect();
    preview(db, Some("clerk"));
    rows
}

fn insert_row(
    db: &Db,
    table: &str,
    column: &str,
    value: &str,
    trigger: Option<TriggerWrite>,
) -> Result<crate::recordstore::commands::TriggeredOutcome, AppError> {
    crate::recordstore::commands::insert_row(
        db.label.clone(),
        table.into(),
        vec![named(column, text(value))],
        trigger,
    )
}

fn forged(trigger_id: &str, step_id: &str, grant: &str) -> Option<TriggerWrite> {
    Some(TriggerWrite {
        trigger_id: trigger_id.into(),
        step_id: step_id.into(),
        grant: Some(grant.into()),
        ..Default::default()
    })
}

fn sync_trigger(id: &str, name: &str, event: &str, run_as: Option<&str>) -> Value {
    let mut trigger = json!({
        "id": id, "name": name, "table": "orders", "event": event, "actionId": "a-stock",
        "mode": "sync", "enabled": true, "maxAttempts": 3, "backoffMs": 1000,
    });
    if let Some(run_as) = run_as {
        trigger["runAs"] = json!(run_as);
    }
    trigger
}

/// `orders`, `inventory` (one row) and `audit_log`; a clerk who can only touch `orders`;
/// an action that updates inventory and logs; and an app-mode created trigger and a
/// user-mode updated trigger for it. The clerk's role is previewed.
fn rbac_db() -> Db {
    let db = Db::new();
    db.table("orders", &["id", "status"]);
    db.table("inventory", &["id", "qty"]);
    db.table("audit_log", &["id", "message"]);
    db.seed("inventory", json!({"qty": "10"}));
    db.set_config(|c| {
        c.actions = serde_json::from_value(json!([{
            "id": "a-stock", "name": "Reserve stock", "onError": "stop",
            "steps": [
                {"id": "s-stock", "kind": "updateRecord", "table": "inventory",
                 "match": {"id": "1"}, "values": {"qty": "'9'"}},
                {"id": "s-log", "kind": "createRecord", "table": "audit_log",
                 "values": {"message": "'ok'"}},
            ],
        }]))
        .unwrap();
        c.triggers = serde_json::from_value(json!([
            sync_trigger("t-stock", "Stock on order", "created", None),
            sync_trigger("t-user", "Stock on change", "updated", Some("user")),
        ]))
        .unwrap();
        c.roles = serde_json::from_value(json!([{
            "id": "clerk", "name": "Clerk",
            "permissions": {"navigation": [], "actions": [], "objects": [
                {"kind": "table", "id": "orders", "read": true, "create": true,
                 "update": true, "delete": false},
            ]},
        }]))
        .unwrap();
    });
    preview(&db, Some("clerk"));
    db
}

#[test]
fn app_mode_triggers_write_tables_the_role_cannot_and_forged_trigger_writes_are_refused() {
    let db = rbac_db();
    let _hook = install_triggers(db.env(host()));
    assert_eq!(code_of(insert_row(&db, "audit_log", "message", "x", None)), "FORBIDDEN");

    records::insert(&db.label, "orders", vec![named("status", text("new"))]).expect("insert");
    assert_eq!(rows(&db, "inventory"), [json!({"id": 1, "qty": "9"})]);
    assert_eq!(rows(&db, "audit_log"), [json!({"id": 1, "message": "ok"})]);

    let try_forged = |trigger: Option<TriggerWrite>| {
        code_of(insert_row(&db, "audit_log", "message", "forged", trigger))
    };
    assert_eq!(try_forged(forged("t-stock", "s-log", "made-up")), "FORBIDDEN");
    let issued = insert_row(&db, "orders", "status", "manual", None).expect("manual insert");
    let grant = issued.trigger_grant.expect("a grant for the sync app-mode trigger");
    assert_eq!(try_forged(forged("t-stock", "s-stock", &grant)), "FORBIDDEN", "not a step that writes audit_log");
    assert_eq!(try_forged(forged("t-user", "s-log", &grant)), "FORBIDDEN", "another trigger's step");
    assert_eq!(try_forged(forged("t-stock", "s-log", &grant)), "OK");
    crate::trigger_auth::release_trigger_grant(db.label.clone(), grant.clone()).expect("release");
    assert_eq!(try_forged(forged("t-stock", "s-log", &grant)), "FORBIDDEN", "released grants are void");
}

#[test]
fn user_mode_triggers_refuse_the_initiating_save_up_front() {
    let db = rbac_db();
    let _hook = install_triggers(db.env(host()));
    records::insert(&db.label, "orders", vec![named("status", text("new"))]).expect("insert");
    let before = rows(&db, "orders");
    let meta = RecordWriteMeta {
        expected: Some(vec![named("status", text("new"))]),
        ..Default::default()
    };
    let error = records::update(
        &db.label,
        "orders",
        vec![named("status", text("changed"))],
        vec![super::tests_support::int(1)],
        meta,
    )
    .unwrap_err()
    .error;
    assert_eq!(error.code, "FORBIDDEN");
    assert!(error.message.contains("Stock on change"), "{}", error.message);
    assert!(error.message.contains("inventory"), "{}", error.message);
    assert_eq!(rows(&db, "orders"), before);
}

// Routes the table's writes to the action, as a customAction entity does.
fn route_to(db: &Db, table: &str, action: Value) {
    let id = action["id"].as_str().unwrap().to_string();
    db.set_config(|c| {
        c.actions.push(serde_json::from_value(action).unwrap());
        for entity in c.entities.iter_mut().filter(|e| e.table == table) {
            entity.concurrency = "customAction".into();
            entity.action_id = Some(id.clone());
        }
    });
}

#[test]
fn app_mode_trigger_writes_routed_to_a_custom_action_run_under_the_triggers_grant() {
    let db = rbac_db();
    route_to(
        &db,
        "inventory",
        json!({
            "id": "a-guard", "name": "Guard stock", "onError": "stop",
            "steps": [
                {"id": "g-upd", "kind": "updateRecord", "table": "inventory", "match": "current",
                 "values": {"qty": "record.qty"}},
                {"id": "g-log", "kind": "createRecord", "table": "audit_log",
                 "values": {"message": "'guarded'"}},
            ],
        }),
    );
    let _hook = install_triggers(db.env(host()));
    let _router = install_custom_actions(db.env(host()));
    records::insert(&db.label, "orders", vec![named("status", text("new"))]).expect("insert");
    assert_eq!(rows(&db, "inventory"), [json!({"id": 1, "qty": "9"})]);
    assert_eq!(
        rows(&db, "audit_log"),
        [json!({"id": 1, "message": "guarded"}), json!({"id": 2, "message": "ok"})]
    );
}

#[test]
fn rust_authorizes_the_writes_of_a_custom_action_a_save_is_routed_to() {
    let db = rbac_db();
    let _hook = install_triggers(db.env(host()));
    records::insert(&db.label, "orders", vec![named("status", text("new"))]).expect("insert");
    db.set_config(|c| c.triggers.clear());
    route_to(
        &db,
        "orders",
        json!({
            "id": "a-order", "name": "Guard order", "onError": "rollback",
            "steps": [
                {"id": "o-upd", "kind": "updateRecord", "table": "orders", "match": "current",
                 "values": {"status": "record.status"}},
                {"id": "o-log", "kind": "createRecord", "table": "audit_log",
                 "values": {"message": "'changed'"}},
            ],
        }),
    );
    let before = (rows(&db, "orders"), rows(&db, "audit_log"));
    let _router = install_custom_actions(Arc::new(RustChecksOnly(db.env(host()))));
    let meta = RecordWriteMeta {
        expected: Some(vec![named("status", text("new"))]),
        ..Default::default()
    };
    let error = records::update(
        &db.label,
        "orders",
        vec![named("status", text("changed"))],
        vec![super::tests_support::int(1)],
        meta,
    )
    .unwrap_err()
    .error;
    assert!(error.message.contains("cannot create table \"audit_log\""), "{}", error.message);
    assert_eq!((rows(&db, "orders"), rows(&db, "audit_log")), before);
}

#[test]
fn gates_checkpoints_installation_reset_and_schema_listing_by_role() {
    let db = rbac_db();
    let label = || db.label.clone();
    assert_eq!(code_of(crate::checkpoints::create_checkpoint(label(), Some("x".into()))), "FORBIDDEN");
    assert_eq!(
        code_of(crate::checkpoints::restore_checkpoint_as_copy(label(), "c".into(), "/tmp/x.ixt".into())),
        "FORBIDDEN"
    );
    assert_eq!(
        code_of(crate::installation_commands::reset_runtime_installation_data(label(), true)),
        "FORBIDDEN"
    );
    let names: Vec<String> = crate::list_database_objects(label())
        .unwrap()
        .into_iter()
        .map(|o| o.name)
        .collect();
    assert_eq!(names, ["orders"]);
    assert_eq!(code_of(crate::inspect_table(label(), "inventory".into(), None)), "FORBIDDEN");
    assert_eq!(code_of(crate::inspect_table(label(), "orders".into(), None)), "OK");

    preview(&db, None);
    assert_eq!(code_of(crate::checkpoints::create_checkpoint(label(), Some("x".into()))), "OK");
    let mut all: Vec<String> = crate::list_database_objects(label())
        .unwrap()
        .into_iter()
        .map(|o| o.name)
        .collect();
    all.sort();
    assert_eq!(all, ["audit_log", "inventory", "orders"]);
}
