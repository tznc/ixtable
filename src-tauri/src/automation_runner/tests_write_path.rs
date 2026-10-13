//! Optimistic writes, matching many rows, `customAction` entities, trigger failures after a
//! commit and the interactive host. Ports of `automation-expected.test.ts` and
//! `automation-write-path.test.ts`.

use super::context::{ActionHost, NavigationTarget, Tone};
use super::custom::install_custom_actions;
use super::host::{app_state, install_handlers, SessionHost, UiHandlers};
use super::runner::{run_action, run_action_by_id};
use super::tests_support::{
    action, action_with, error_has, host, int, named, obj, orders_db, step, text, with_actions, Db,
};
use super::tests_triggers::{env, set_triggers, Capture};
use super::triggers::install_triggers;
use crate::data::DataValue;
use crate::manager::AppError;
use crate::records::{self, RecordHook, RecordOperation, RecordWrite, RecordWriteMeta};
use serde_json::{json, Map, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

fn update(id: &str, values: Value) -> Value {
    json!({"id": id, "kind": "updateRecord", "table": "orders", "match": "current", "values": values})
}

fn remove(id: &str) -> Value {
    json!({"id": id, "kind": "deleteRecord", "table": "orders", "match": "current"})
}

/// `orders` (id, status, note) with one row.
fn notes_db() -> Db {
    let db = Db::new();
    db.table("orders", &["id", "status", "note"]);
    db.seed("orders", json!({"id": 1, "status": "open", "note": "n"}));
    db
}

fn run_on(db: &Db, a: &crate::automation::ActionDef, record: Value) -> super::runner::ActionResult {
    with_actions(db, vec![a.clone()]);
    let mut ctx = db.ctx(host());
    ctx.record = Some(obj(record));
    run_action(a, ctx)
}

#[test]
fn rows_written_twice_carry_post_write_values_forward_in_a_rollback_transaction() {
    let db = notes_db();
    let a = action_with(
        "a",
        "rollback",
        vec![
            step(update("s1", json!({"status": "'approved'"}))),
            step(update("s2", json!({"note": "'done'"}))),
        ],
    );
    let result = run_on(&db, &a, json!({"id": 1}));
    assert!(result.ok, "{:?}", result.error);
    let row = &db.rows("orders")[0];
    assert_eq!((&row["status"], &row["note"]), (&json!("approved"), &json!("done")));
}

#[test]
fn deletes_a_row_updated_earlier_in_the_same_transaction() {
    let db = notes_db();
    let a = action_with(
        "a",
        "rollback",
        vec![step(update("s1", json!({"status": "'approved'"}))), step(remove("s2"))],
    );
    assert!(run_on(&db, &a, json!({"id": 1})).ok);
    assert!(db.rows("orders").is_empty());
}

#[test]
fn fails_clearly_on_a_row_deleted_earlier_in_the_run() {
    let db = notes_db();
    let a = action_with(
        "a",
        "rollback",
        vec![step(remove("s1")), step(update("s2", json!({"note": "'x'"})))],
    );
    let result = run_on(&db, &a, json!({"id": 1}));
    assert!(!result.ok);
    assert!(error_has(&result.error, "already deleted by this action"), "{:?}", result.error);
    assert_eq!(db.rows("orders").len(), 1);
}

#[test]
fn updates_the_same_row_twice_with_immediate_writes() {
    let db = notes_db();
    let a = action(
        "a",
        vec![
            step(update("s1", json!({"status": "'approved'"}))),
            step(update("s2", json!({"note": "'done'"}))),
        ],
    );
    assert!(run_on(&db, &a, json!({"id": 1})).ok);
    let row = &db.rows("orders")[0];
    assert_eq!((&row["status"], &row["note"]), (&json!("approved"), &json!("done")));
}

fn custom_entity(db: &Db) {
    db.set_config(|c| {
        c.entities = serde_json::from_value(json!([
            {"id": "e1", "table": "orders", "concurrency": "customAction", "actionId": "a"}
        ]))
        .unwrap();
    });
}

/// A direct write that skips the custom-action router (another user's change).
fn change_elsewhere(db: &Db, column: &str, value: &str) {
    let loaded = vec![named("id", int(1)), named("status", text("open")), named("note", text("n"))];
    let meta = RecordWriteMeta { direct: true, expected: Some(loaded), ..Default::default() };
    records::update(&db.label, "orders", vec![named(column, text(value))], vec![int(1)], meta)
        .expect("change elsewhere");
}

fn custom_update_db() -> (Db, records::Registration) {
    let db = notes_db();
    let a = action("a", vec![step(update("c1", json!({"status": "params.changes.status"})))]);
    with_actions(&db, vec![a]);
    custom_entity(&db);
    let router = install_custom_actions(env(&db));
    (db, router)
}

fn has_conflict(error: &records::WriteError) -> bool {
    let message = error.error.message.to_lowercase();
    message.contains("changed") || message.contains("conflict")
}

#[test]
fn custom_actions_conflict_when_the_row_changed_after_the_form_loaded_it() {
    let (db, _router) = custom_update_db();
    change_elsewhere(&db, "note", "changed elsewhere");
    let loaded = obj(json!({"id": 1, "status": "open", "note": "n"}));
    let expected = vec![named("id", int(1)), named("status", text("open")), named("note", text("n"))];
    let meta = RecordWriteMeta { old: Some(loaded), expected: Some(expected), ..Default::default() };
    let error = records::update(&db.label, "orders", vec![named("status", text("x"))], vec![int(1)], meta)
        .unwrap_err();
    assert!(has_conflict(&error), "{}", error.error.message);
    let row = &db.rows("orders")[0];
    assert_eq!((&row["status"], &row["note"]), (&json!("open"), &json!("changed elsewhere")));
}

#[test]
fn custom_actions_conflict_from_the_stale_old_snapshot_when_no_expected_is_given() {
    let (db, _router) = custom_update_db();
    change_elsewhere(&db, "note", "changed elsewhere");
    let meta = RecordWriteMeta {
        old: Some(obj(json!({"id": 1, "status": "open", "note": "n"}))),
        ..Default::default()
    };
    let error = records::update(&db.label, "orders", vec![named("status", text("x"))], vec![int(1)], meta)
        .unwrap_err();
    assert!(has_conflict(&error), "{}", error.error.message);
}

#[test]
fn custom_actions_write_when_the_grid_passes_no_snapshot() {
    let (db, _router) = custom_update_db();
    records::update(
        &db.label,
        "orders",
        vec![named("status", text("x"))],
        vec![int(1)],
        RecordWriteMeta::default(),
    )
    .expect("update");
    assert_eq!(db.rows("orders")[0]["status"], json!("x"));
}

/// `orders` without a primary key.
fn keyless_db() -> Db {
    let db = Db::new();
    let column = |name: &str, ty: &str| {
        json!({"name": name, "declaredType": ty, "nullable": true, "primaryKeyPosition": 0})
    };
    let spec = json!({"name": "orders",
        "columns": [column("id", "INTEGER"), column("status", "TEXT"), column("note", "TEXT")]});
    crate::create_database_table(db.label.clone(), serde_json::from_value(spec).unwrap()).expect("table");
    db.seed("orders", json!({"id": 1, "status": "open", "note": "n"}));
    db
}

fn keyless_record(note: &str) -> Value {
    json!({"rowid": 1, "id": 1, "status": "open", "note": note})
}

#[test]
fn keyless_tables_send_the_loaded_snapshot_not_unsaved_edits_as_expected() {
    let db = keyless_db();
    let a = action("a", vec![step(update("s1", json!({"status": "'approved'"})))]);
    with_actions(&db, vec![a.clone()]);
    let mut ctx = db.ctx(host());
    ctx.record = Some(obj(keyless_record("unsaved edit")));
    ctx.snapshot = Some(obj(keyless_record("n")));
    let result = run_action(&a, ctx);
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(db.rows("orders")[0]["status"], json!("approved"));
}

#[test]
fn keyless_tables_update_the_same_row_twice() {
    let db = keyless_db();
    let a = action(
        "a",
        vec![
            step(update("s1", json!({"status": "'approved'"}))),
            step(update("s2", json!({"note": "'done'"}))),
        ],
    );
    with_actions(&db, vec![a.clone()]);
    let mut ctx = db.ctx(host());
    ctx.record = Some(obj(keyless_record("n")));
    ctx.snapshot = Some(obj(keyless_record("n")));
    let result = run_action(&a, ctx);
    assert!(result.ok, "{:?}", result.error);
    let row = &db.rows("orders")[0];
    assert_eq!((&row["status"], &row["note"]), (&json!("approved"), &json!("done")));
}

fn pairs(values: &[crate::data::NamedValue]) -> Vec<(String, DataValue)> {
    values.iter().map(|v| (v.column.clone(), v.value.clone())).collect()
}

#[test]
fn optimistic_writes_send_the_matched_rows_original_values_as_expected() {
    let db = orders_db();
    let a = action(
        "a",
        vec![
            step(update("s1", json!({"status": "'x'"}))),
            step(json!({"id": "s2", "kind": "deleteRecord", "table": "orders", "match": {"id": "2"}})),
        ],
    );
    let capture = Arc::new(Capture::default());
    let _capture = records::register_hook(&db.label, capture.clone());
    let result = run_on(&db, &a, json!({"id": 1}));
    assert!(result.ok, "{:?}", result.error);
    let writes = capture.on("orders");
    let expected = |op| {
        let write = writes.iter().find(|w| w.operation == op).expect("write");
        pairs(write.meta.expected.as_ref().expect("expected"))
    };
    assert_eq!(
        expected(RecordOperation::Update),
        [("id".into(), int(1)), ("status".into(), text("open")), ("total".into(), int(10))]
    );
    assert_eq!(
        expected(RecordOperation::Delete),
        [("id".into(), int(2)), ("status".into(), text("open")), ("total".into(), int(20))]
    );
}

/// Changes the order once, right before the first update is stored.
struct ChangesOrderFirst {
    label: String,
    armed: AtomicBool,
}

impl RecordHook for ChangesOrderFirst {
    fn before(&self, write: RecordWrite) -> Result<RecordWrite, AppError> {
        if write.operation == RecordOperation::Update && self.armed.swap(false, Ordering::SeqCst) {
            let current = vec![named("id", int(1)), named("status", text("open")), named("total", int(10))];
            let meta = RecordWriteMeta { direct: true, expected: Some(current), ..Default::default() };
            records::update(&self.label, "orders", vec![named("status", text("shipped"))], vec![int(1)], meta)
                .expect("concurrent change");
        }
        Ok(write)
    }
}

#[test]
fn conflicts_when_the_row_changed_after_the_step_read_it() {
    let db = orders_db();
    let a = action("a", vec![step(update("s1", json!({"status": "'x'"})))]);
    let _hook = records::register_hook(
        &db.label,
        Arc::new(ChangesOrderFirst { label: db.label.clone(), armed: AtomicBool::new(true) }),
    );
    let result = run_on(&db, &a, json!({"id": 1}));
    assert!(!result.ok);
    let error = result.error.unwrap().to_lowercase();
    assert!(error.contains("changed") || error.contains("conflict"), "{error}");
    assert_eq!(db.rows("orders")[0]["status"], json!("shipped"));
}

#[test]
fn pages_through_every_match_instead_of_stopping_at_one_page() {
    let db = Db::new();
    db.table("items", &["id", "flag"]);
    let rows: Vec<RecordWrite> = (1..=1200)
        .map(|i| RecordWrite::insert("items", vec![named("id", int(i)), named("flag", text("a"))]))
        .collect();
    records::write_batch(&db.label, rows).expect("seed");
    let a = action(
        "a",
        vec![step(json!({"kind": "updateRecord", "table": "items", "match": {"flag": "'a'"},
            "values": {"flag": "'b'"}}))],
    );
    with_actions(&db, vec![a.clone()]);
    assert!(run_action(&a, db.ctx(host())).ok);
    let flags = db.column("items", "flag");
    assert_eq!(flags.len(), 1000, "reads are paged; the writes below check the rest");
    assert!(flags.iter().all(|f| f == "b"));
    let page = crate::read_table_page(db.label.clone(), "items".into(), 1000, 1000, vec![], vec![], None)
        .expect("second page");
    assert_eq!(page.rows.len(), 200);
    assert!(page.rows.iter().all(|r| r[1] == text("b")));
}

fn guard() -> crate::automation::ActionDef {
    action(
        "guard",
        vec![
            step(json!({"id": "g1", "kind": "createRecord", "table": "audit", "values": {
                "message": "params.operation & ' ' & old.status & ' → ' & coalesce(params.changes.status, '-') & ' / ' & record.status"
            }})),
            step(json!({"id": "g2", "kind": "condition", "when": "params.operation = 'update'",
                "then": [{"id": "g3", "kind": "updateRecord", "table": "orders", "match": "current",
                    "values": {"status": "upper(record.status)"}}],
                "else": [{"id": "g4", "kind": "deleteRecord", "table": "orders", "match": "current"}]})),
        ],
    )
}

fn guarded_db() -> (Db, records::Registration) {
    let db = orders_db();
    with_actions(&db, vec![guard()]);
    db.set_config(|c| {
        c.entities = serde_json::from_value(json!([
            {"id": "e1", "table": "orders", "concurrency": "customAction", "actionId": "guard"}
        ]))
        .unwrap();
    });
    let router = install_custom_actions(env(&db));
    (db, router)
}

#[test]
fn custom_entities_route_updates_and_deletes_to_the_action_which_writes_directly() {
    let (db, _router) = guarded_db();
    let meta = RecordWriteMeta {
        old: Some(obj(json!({"id": 1, "status": "open", "total": 10}))),
        expected: Some(vec![named("status", text("open"))]),
        ..Default::default()
    };
    records::update(&db.label, "orders", vec![named("status", text("paid"))], vec![int(1)], meta)
        .expect("update");
    assert_eq!(db.rows("orders")[0]["status"], json!("PAID"));
    records::delete(&db.label, "orders", vec![int(2)], RecordWriteMeta::default()).expect("delete");
    assert_eq!(db.column("orders", "id"), [json!(1)]);
    assert_eq!(
        db.column("audit", "message"),
        [json!("update open → paid / paid"), json!("delete open → - / open")]
    );
    records::insert(&db.label, "orders", vec![named("status", text("new"))]).expect("insert");
    assert_eq!(db.rows("orders").len(), 2);
}

#[test]
fn custom_entities_route_action_steps_too_joining_a_rollback_transaction() {
    let (db, _router) = guarded_db();
    let caller = action_with(
        "caller",
        "rollback",
        vec![step(json!({"id": "c1", "kind": "updateRecord", "table": "orders", "match": {"id": "1"},
            "values": {"status": "'late'"}}))],
    );
    with_actions(&db, vec![guard(), caller]);
    let result = run_action_by_id("caller", db.ctx(host()));
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(db.rows("orders")[0]["status"], json!("LATE"));
    assert_eq!(db.rows("audit").len(), 1);
}

#[test]
fn custom_entities_need_a_role_allowed_to_run_the_routed_action() {
    let (db, _router) = guarded_db();
    let caller = action(
        "caller",
        vec![step(json!({"id": "c1", "kind": "updateRecord", "table": "orders", "match": {"id": "1"}, "values": {}}))],
    );
    with_actions(&db, vec![guard(), caller.clone()]);
    let mut ctx = db.ctx(host());
    ctx.authorize = Some(Arc::new(|kind, id, _| kind != "action" || id != "guard"));
    let result = run_action(&caller, ctx);
    assert!(!result.ok);
    assert!(error_has(&result.error, "Not permitted"), "{:?}", result.error);
    assert!(db.rows("audit").is_empty());
}

#[test]
fn custom_entities_reject_the_write_when_the_action_fails() {
    let (db, _router) = guarded_db();
    with_actions(
        &db,
        vec![action("guard", vec![step(json!({"kind": "fail", "message": "'Use the approval form'"}))])],
    );
    let error = records::update(
        &db.label,
        "orders",
        vec![named("status", text("x"))],
        vec![int(1)],
        RecordWriteMeta::default(),
    )
    .unwrap_err();
    assert!(error.error.message.contains("Use the approval form"), "{}", error.error.message);
    assert_eq!(db.rows("orders")[0]["status"], json!("open"));
}

fn boom_trigger(db: &Db, name: &str) {
    with_actions(
        db,
        vec![action("boom", vec![step(json!({"kind": "fail", "message": "'audit is full'"}))])],
    );
    set_triggers(
        db,
        vec![super::tests_support::trigger(json!({
            "name": name, "event": "updated", "actionId": "boom", "maxAttempts": 1, "backoffMs": 0
        }))],
    );
}

#[test]
fn sync_trigger_failures_after_a_commit_say_the_changes_were_saved_and_still_run_effects() {
    let db = orders_db();
    let a = action_with(
        "a",
        "rollback",
        vec![
            step(json!({"id": "s1", "kind": "updateRecord", "table": "orders", "match": {"id": "1"},
                "values": {"status": "'x'"}})),
            step(json!({"id": "s2", "kind": "message", "text": "'Updated'"})),
        ],
    );
    let _hook = {
        boom_trigger(&db, "Audit orders");
        install_triggers(env(&db))
    };
    let h = host();
    with_actions(
        &db,
        vec![
            action("boom", vec![step(json!({"kind": "fail", "message": "'audit is full'"}))]),
            a.clone(),
        ],
    );
    let result = run_action(&a, db.ctx(h.clone()));
    assert!(!result.ok);
    let error = result.error.unwrap();
    assert_eq!(
        error,
        "The record changes were saved, but trigger \"Audit orders\" failed: audit is full"
    );
    assert!(!error.contains("no record changes were saved"));
    assert_eq!(db.rows("orders")[0]["status"], json!("x"));
    assert_eq!(h.events(), [json!(["notify", "Updated", "info"]), json!(["refresh"])]);
}

#[test]
fn direct_writes_report_the_saved_record_and_refresh() {
    let db = orders_db();
    boom_trigger(&db, "T");
    let a = action(
        "a",
        vec![step(json!({"id": "s1", "kind": "updateRecord", "table": "orders", "match": {"id": "1"},
            "values": {"status": "'x'"}}))],
    );
    let _hook = install_triggers(env(&db));
    let h = host();
    with_actions(
        &db,
        vec![
            action("boom", vec![step(json!({"kind": "fail", "message": "'nope'"}))]),
            a.clone(),
        ],
    );
    let result = run_action(&a, db.ctx(h.clone()));
    assert!(error_has(&result.error, "The record was saved, but trigger \"T\" failed: nope"));
    assert_eq!(h.events(), [json!(["refresh"])]);
}

/// Handlers that follow navigation and report an app state, like the Runtime.
struct Listener {
    seen: Mutex<Vec<String>>,
}

impl UiHandlers for Listener {
    fn navigate(&self, target: &NavigationTarget) -> bool {
        self.seen.lock().unwrap().push(format!("{} {}", target.kind, target.id));
        true
    }
    fn app_state(&self) -> Map<String, Value> {
        obj(json!({"region": "EU"}))
    }
}

#[test]
fn session_hosts_hand_navigation_to_listening_handlers_and_report_it_when_nobody_listens() {
    let label = format!("host-{}", uuid::Uuid::new_v4());
    let notices = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = notices.clone();
    let session = SessionHost::new(
        &label,
        Arc::new(|_| true),
        Arc::new(move |message: &str, _: Tone| sink.lock().unwrap().push(message.into())),
        Arc::new(|| {}),
    );
    let target = NavigationTarget::new("form", "f1");
    session.navigate(&target).unwrap();
    assert!(notices.lock().unwrap()[0].contains("Open it in the Runtime"));
    let listener = Arc::new(Listener { seen: Mutex::new(Vec::new()) });
    let registration = install_handlers(&label, listener.clone());
    session.navigate(&target).unwrap();
    assert_eq!(*listener.seen.lock().unwrap(), ["form f1"]);
    assert_eq!(notices.lock().unwrap().len(), 1);
    assert_eq!(app_state(&label), obj(json!({"region": "EU"})));
    drop(registration);
    assert!(app_state(&label).is_empty());
}
