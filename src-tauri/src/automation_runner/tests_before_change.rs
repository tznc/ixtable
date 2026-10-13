//! Before-change triggers, deleted triggers and action queries. Ports of
//! `before-change-triggers.test.ts` against the real RecordStore.

use super::tests_support::{action, int, named, obj, step, text, trigger, with_actions, Db};
use super::tests_triggers::{env, jobs, messages, set_triggers, Capture};
use super::triggers::install_triggers;
use crate::automation::{ActionDef, Step, Trigger};
use crate::records::{self, RecordWrite, RecordWriteMeta, Registration};
use serde_json::{json, Value};
use std::sync::Arc;

fn stamp() -> ActionDef {
    action("stamp", vec![set("s1", "note", "'stamped ' & record.status")])
}

fn audit() -> ActionDef {
    action(
        "audit",
        vec![step(json!({"id": "a1", "kind": "createRecord", "table": "audit", "values": {
            "message": "'deleted ' & record.id & ' ' & record.status & ' old=' & old.status"
        }}))],
    )
}

fn set(id: &str, field: &str, value: &str) -> Step {
    step(json!({"id": id, "kind": "setField", "field": field, "value": value}))
}

fn fail(message: &str, when: &str) -> Step {
    step(json!({"kind": "fail", "message": message, "when": when}))
}

fn bc(overrides: Value) -> Trigger {
    let mut base = json!({
        "id": "t1", "name": "Check orders", "event": "beforeChange", "actionId": "stamp",
        "maxAttempts": 1, "backoffMs": 0,
    });
    for (k, v) in overrides.as_object().unwrap() {
        base[k] = v.clone();
    }
    trigger(base)
}

/// `orders` (id, status, note) with one open order, and an empty `audit`.
fn bc_db() -> Db {
    let db = Db::new();
    db.table("orders", &["id", "status", "note"]);
    db.table("audit", &["id", "message"]);
    db.seed("orders", json!({"id": 1, "status": "open"}));
    db.last_write_wins("orders");
    db
}

/// Sets the actions and triggers and installs the trigger hook.
fn install(db: &Db, mut actions: Vec<ActionDef>, triggers: Vec<Trigger>) -> Registration {
    actions.push(audit());
    with_actions(db, actions);
    set_triggers(db, triggers);
    install_triggers(env(db))
}

fn insert(db: &Db, status: &str) -> Result<Vec<crate::data::DataValue>, records::WriteError> {
    records::insert(&db.label, "orders", vec![named("status", text(status))])
}

fn update_status(db: &Db, status: &str) -> Result<u64, records::WriteError> {
    records::update(
        &db.label,
        "orders",
        vec![named("status", text(status))],
        vec![int(1)],
        RecordWriteMeta::default(),
    )
}

fn message(error: records::WriteError) -> String {
    error.error.message
}

#[test]
fn sets_a_field_on_insert_and_the_stored_row_has_it() {
    let db = bc_db();
    let _hook = install(&db, vec![stamp()], vec![bc(json!({}))]);
    let capture = Arc::new(Capture::default());
    let _capture = records::register_hook(&db.label, capture.clone());
    insert(&db, "new").expect("insert");
    let row = &db.rows("orders")[1];
    assert_eq!((&row["status"], &row["note"]), (&json!("new"), &json!("stamped new")));
    let sent = &capture.on("orders")[0];
    assert!(sent.values.iter().any(|v| v.column == "note" && v.value == text("stamped new")));
}

#[test]
fn sets_a_field_on_update_from_the_old_row_merged_with_the_new_values() {
    let db = bc_db();
    let stamp = action(
        "stamp",
        vec![set("s1", "note", "old.status & '->' & record.status & ' #' & record.id")],
    );
    let _hook = install(&db, vec![stamp], vec![bc(json!({}))]);
    update_status(&db, "closed").expect("update");
    let row = &db.rows("orders")[0];
    assert_eq!((&row["status"], &row["note"]), (&json!("closed"), &json!("open->closed #1")));
}

#[test]
fn gives_inserts_a_null_old_row() {
    let db = bc_db();
    let stamp = action("stamp", vec![set("s1", "note", "coalesce(old.status, 'none')")]);
    let _hook = install(&db, vec![stamp], vec![bc(json!({}))]);
    insert(&db, "new").expect("insert");
    assert_eq!(db.rows("orders")[1]["note"], json!("none"));
}

#[test]
fn rejects_with_the_fail_steps_message_and_writes_nothing() {
    let db = bc_db();
    let stamp = action("stamp", vec![fail("'Status is required'", "true")]);
    let _hook = install(&db, vec![stamp], vec![bc(json!({}))]);
    let capture = Arc::new(Capture::default());
    let _capture = records::register_hook(&db.label, capture.clone());
    let error = insert(&db, "x").unwrap_err();
    assert!(!error.committed);
    assert_eq!(error.error.code, "BEFORE_CHANGE_REJECTED");
    assert_eq!(error.error.message, "Not saved: Status is required");
    assert_eq!(db.rows("orders").len(), 1);
}

#[test]
fn rejects_an_update_with_a_failing_condition_gated_fail_step_and_leaves_the_row() {
    let db = bc_db();
    update_status(&db, "closed").expect("close without triggers");
    let stamp = action("stamp", vec![fail("'Closed orders are final'", "old.status = 'closed'")]);
    let _hook = install(&db, vec![stamp], vec![bc(json!({}))]);
    let error = update_status(&db, "open").unwrap_err();
    assert_eq!(error.error.message, "Not saved: Closed orders are final");
    assert_eq!(db.rows("orders")[0]["status"], json!("closed"));
}

#[test]
fn names_the_trigger_when_a_step_errors() {
    let db = bc_db();
    let stamp = action("stamp", vec![set("s1", "note", "1 +")]);
    let _hook = install(&db, vec![stamp], vec![bc(json!({}))]);
    let error = message(insert(&db, "x").unwrap_err());
    assert!(error.starts_with("Not saved: trigger \"Check orders\" failed: "), "{error}");
    assert_eq!(db.rows("orders").len(), 1);
}

#[test]
fn refuses_steps_that_write_with_a_message_naming_the_step() {
    let db = bc_db();
    let stamp = action(
        "stamp",
        vec![step(json!({"kind": "createRecord", "table": "audit", "values": {"message": "'x'"}}))],
    );
    let _hook = install(&db, vec![stamp], vec![bc(json!({}))]);
    let error = message(insert(&db, "x").unwrap_err());
    assert_eq!(
        error,
        "Not saved: A before-change trigger cannot run \"Create record\" steps; use an after trigger"
    );
    assert_eq!(db.rows("orders").len(), 1);
    assert!(db.rows("audit").is_empty());
}

#[test]
fn skips_a_trigger_whose_condition_is_false() {
    let db = bc_db();
    let _hook = install(&db, vec![stamp()], vec![bc(json!({"condition": "record.status = 'open'"}))]);
    insert(&db, "draft").expect("draft");
    insert(&db, "open").expect("open");
    assert_eq!(db.column("orders", "note"), [json!(null), json!(null), json!("stamped open")]);
}

#[test]
fn skips_disabled_triggers() {
    let db = bc_db();
    let _hook = install(&db, vec![stamp()], vec![bc(json!({"enabled": false}))]);
    insert(&db, "open").expect("insert");
    assert_eq!(db.rows("orders")[1]["note"], json!(null));
}

#[test]
fn chains_triggers_in_order_each_seeing_the_fields_earlier_ones_set() {
    let db = bc_db();
    let actions = vec![
        action("first", vec![set("s1", "note", "'one'")]),
        action("second", vec![set("s2", "note", "record.note & '+two'")]),
    ];
    let triggers = vec![
        bc(json!({"id": "t1", "actionId": "first"})),
        bc(json!({"id": "t2", "actionId": "second"})),
    ];
    let _hook = install(&db, actions, triggers);
    insert(&db, "x").expect("insert");
    assert_eq!(db.rows("orders")[1]["note"], json!("one+two"));
}

#[test]
fn sees_later_steps_updated_record_within_one_action() {
    let db = bc_db();
    let stamp = action("stamp", vec![set("s1", "note", "'a'"), set("s2", "status", "record.note & 'b'")]);
    let _hook = install(&db, vec![stamp], vec![bc(json!({}))]);
    insert(&db, "x").expect("insert");
    let row = &db.rows("orders")[1];
    assert_eq!((&row["note"], &row["status"]), (&json!("a"), &json!("ab")));
}

#[test]
fn stops_the_chain_when_an_earlier_trigger_rejects() {
    let db = bc_db();
    let actions = vec![
        action("first", vec![step(json!({"kind": "fail", "message": "'no'"}))]),
        action("second", vec![set("s2", "note", "'two'")]),
    ];
    let triggers = vec![
        bc(json!({"id": "t1", "actionId": "first"})),
        bc(json!({"id": "t2", "actionId": "second"})),
    ];
    let _hook = install(&db, actions, triggers);
    assert_eq!(message(insert(&db, "x").unwrap_err()), "Not saved: no");
}

#[test]
fn rejects_a_whole_write_batch_and_writes_nothing() {
    let db = bc_db();
    let stamp = action(
        "stamp",
        vec![fail("'bad row'", "record.status = 'bad'"), set("s1", "note", "'ok'")],
    );
    let _hook = install(&db, vec![stamp], vec![bc(json!({}))]);
    let write = |status: &str| RecordWrite::insert("orders", vec![named("status", text(status))]);
    let error = records::write_batch(&db.label, vec![write("good"), write("bad")]).unwrap_err();
    assert_eq!(error.error.message, "Not saved: bad row");
    assert_eq!(db.rows("orders").len(), 1);
    records::write_batch(&db.label, vec![write("good"), write("fine")]).expect("batch");
    assert_eq!(db.column("orders", "note"), [json!(null), json!("ok"), json!("ok")]);
}

#[test]
fn deleted_triggers_run_after_the_delete_with_the_deleted_row_as_record_and_old() {
    let db = bc_db();
    let _hook = install(&db, vec![], vec![bc(json!({"event": "deleted", "actionId": "audit"}))]);
    records::delete(&db.label, "orders", vec![int(1)], RecordWriteMeta::default()).expect("delete");
    assert!(db.rows("orders").is_empty());
    assert_eq!(messages(&db), [json!("deleted 1 open old=open")]);
}

#[test]
fn deleted_triggers_use_meta_old_when_the_caller_passes_it() {
    let db = bc_db();
    let _hook = install(&db, vec![], vec![bc(json!({"event": "deleted", "actionId": "audit"}))]);
    let meta = RecordWriteMeta {
        old: Some(obj(json!({"id": 1, "status": "passed"}))),
        ..Default::default()
    };
    records::delete(&db.label, "orders", vec![int(1)], meta).expect("delete");
    assert_eq!(messages(&db), [json!("deleted 1 passed old=passed")]);
}

#[test]
fn deleted_triggers_honor_the_condition_against_the_deleted_row() {
    let db = bc_db();
    let trigger = bc(json!({"event": "deleted", "actionId": "audit", "condition": "old.status = 'x'"}));
    let _hook = install(&db, vec![], vec![trigger]);
    records::delete(&db.label, "orders", vec![int(1)], RecordWriteMeta::default()).expect("delete");
    assert!(db.rows("audit").is_empty());
}

#[test]
fn enqueues_a_job_for_async_deleted_triggers_with_the_row() {
    let db = bc_db();
    let trigger = bc(json!({"event": "deleted", "actionId": "audit", "mode": "async"}));
    let _hook = install(&db, vec![], vec![trigger]);
    records::delete(&db.label, "orders", vec![int(1)], RecordWriteMeta::default()).expect("delete");
    let queued = jobs(&db);
    assert_eq!(queued.len(), 1);
    assert_eq!((queued[0].trigger_id.as_str(), queued[0].action_id.as_str()), ("t1", "audit"));
    let payload = &queued[0].payload;
    assert_eq!(payload["table"], json!("orders"));
    assert_eq!(payload["event"], json!("deleted"));
    assert_eq!(payload["record"]["id"], json!(1));
    assert_eq!(payload["record"]["status"], json!("open"));
    assert_eq!(payload["old"]["status"], json!("open"));
    assert_eq!(payload["identity"], json!([1]));
    assert!(db.rows("audit").is_empty());
}

/// Two open orders and the action queries `pay` (update) and `purge` (delete).
fn query_db() -> Db {
    let db = bc_db();
    db.seed("orders", json!({"id": 2, "status": "open"}));
    db.set_config(|c| {
        c.saved_queries = serde_json::from_value(json!([
            {"id": "pay", "name": "Pay", "sql": "UPDATE orders SET status = 'paid'",
             "action": {"kind": "update", "table": "orders"}},
            {"id": "purge", "name": "Purge", "sql": "DELETE FROM orders",
             "action": {"kind": "delete", "table": "orders"}},
        ]))
        .unwrap();
    });
    db
}

fn run_query(db: &Db, id: &str, dry_run: bool) -> Result<crate::queries::action::ActionRun, records::WriteError> {
    records::run_action_query(&db.label, id, Vec::new(), dry_run, 0)
}

#[test]
fn action_queries_run_before_change_hooks_on_pending_rows_and_apply_the_overrides() {
    let db = query_db();
    let _hook = install(&db, vec![stamp()], vec![bc(json!({}))]);
    let run = run_query(&db, "pay", false).expect("run");
    assert!(!run.dry_run);
    assert_eq!(db.column("orders", "status"), [json!("paid"), json!("paid")]);
    assert_eq!(db.column("orders", "note"), [json!("stamped paid"), json!("stamped paid")]);
}

#[test]
fn action_queries_give_each_pending_rows_old_values_to_the_trigger() {
    let db = query_db();
    let stamp = action("stamp", vec![set("s1", "note", "old.status & '>' & record.status")]);
    let _hook = install(&db, vec![stamp], vec![bc(json!({}))]);
    run_query(&db, "pay", false).expect("run");
    assert_eq!(db.column("orders", "note"), [json!("open>paid"), json!("open>paid")]);
}

#[test]
fn action_queries_leave_rows_the_triggers_skip_alone() {
    let db = query_db();
    let _hook = install(&db, vec![stamp()], vec![bc(json!({"condition": "record.id = 2"}))]);
    run_query(&db, "pay", false).expect("run");
    assert_eq!(db.column("orders", "status"), [json!("paid"), json!("paid")]);
    assert_eq!(db.column("orders", "note"), [json!(null), json!("stamped paid")]);
}

#[test]
fn action_queries_write_nothing_when_a_trigger_rejects() {
    let db = query_db();
    let stamp = action("stamp", vec![fail("'No paying'", "record.id = 2")]);
    let _hook = install(&db, vec![stamp], vec![bc(json!({}))]);
    let error = run_query(&db, "pay", false).unwrap_err();
    assert_eq!(error.error.message, "Not saved: No paying");
    assert_eq!(db.column("orders", "status"), [json!("open"), json!("open")]);
}

#[test]
fn action_queries_do_not_run_before_change_hooks_on_a_dry_run() {
    let db = query_db();
    let stamp = action("stamp", vec![fail("'must not run'", "true")]);
    let _hook = install(&db, vec![stamp], vec![bc(json!({}))]);
    let run = run_query(&db, "pay", true).expect("dry run");
    assert!(run.dry_run);
    assert_eq!(db.column("orders", "status"), [json!("open"), json!("open")]);
}

#[test]
fn action_queries_fire_deleted_triggers_once_per_deleted_row() {
    let db = query_db();
    let trigger = bc(json!({"event": "deleted", "actionId": "audit"}));
    let _hook = install(&db, vec![], vec![trigger]);
    let run = run_query(&db, "purge", false).expect("run");
    assert_eq!(run.removed + run.changed, 2);
    assert!(db.rows("orders").is_empty());
    let mut sent = messages(&db);
    sent.sort_by_key(|m| m.to_string());
    assert_eq!(sent, [json!("deleted 1 open old=open"), json!("deleted 2 open old=open")]);
}

#[test]
fn action_queries_enqueue_one_async_deleted_job_per_deleted_row() {
    let db = query_db();
    let trigger = bc(json!({"event": "deleted", "actionId": "audit", "mode": "async"}));
    let _hook = install(&db, vec![], vec![trigger]);
    run_query(&db, "purge", false).expect("run");
    let mut identities: Vec<String> = jobs(&db).iter().map(|j| j.payload["identity"].to_string()).collect();
    identities.sort();
    assert_eq!(identities, ["[1]", "[2]"]);
    assert!(jobs(&db).iter().all(|j| j.payload["event"] == json!("deleted")));
}
