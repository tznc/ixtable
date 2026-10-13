//! More automation flows: before-change triggers with the shell's hooks installed (port of
//! `tests/integration/before-change-triggers.test.tsx`), the step editor (port of
//! `tests/unit/automation-fail-step.test.tsx`), the jobs panel (port of
//! `tests/unit/jobs-panel.test.tsx`) and the mode itself (tabs, reveals, problems).

use super::tests::{choose, id, text};
use super::{AutomationMode, JobsPanel, Tab};
use crate::archive::DocumentConfig;
use crate::data::{DataValue, NamedValue};
use crate::records::{self, RecordWriteMeta};
use crate::ui::shell::{AutomationTab, ModeId, RevealTarget};
use crate::ui::testing::{shown, Harness};
use gpui_kit::{Entity, TestAppContext};
use serde_json::{json, Value};
use std::time::Duration;

fn named(column: &str, value: DataValue) -> NamedValue {
    NamedValue { column: column.into(), value }
}

fn setup(h: &mut Harness) {
    h.create_table(
        "orders",
        &[("id", "INTEGER", 1), ("total", "REAL", 0), ("due", "DATE", 0), ("status", "TEXT", 0)],
    );
    h.create_table("audit", &[("id", "INTEGER", 1), ("message", "TEXT", 0)]);
    for (order, total) in [(1, 10.0), (2, 20.0), (3, 30.0)] {
        h.insert_row(
            "orders",
            &[
                ("id", DataValue::Integer(order)),
                ("total", DataValue::Real(total)),
                ("due", DataValue::Date("2024-01-31".into())),
                ("status", text("open")),
            ],
        );
    }
}

// Stores definitions through the backend; the config store picks them up on its next write.
fn edit_config(h: &mut Harness, edit: impl FnOnce(&mut DocumentConfig)) {
    let mut config = h.config();
    edit(&mut config);
    crate::manager().unwrap().update_config(&h.label, config).expect("update config");
}

// Edits definitions the way the editors do, through the config store the views read.
fn edit_store(h: &mut Harness, edit: impl FnOnce(&mut DocumentConfig)) {
    let store = h.read_shell(|s, _| s.store.clone());
    h.cx
        .update(|cx| store.update(cx, |s, cx| s.update("Test edit", edit, cx)))
        .expect("update config");
    h.settle();
}

fn orders_without_conflict_checks(h: &mut Harness) {
    edit_config(h, |c| {
        for entity in c.entities.iter_mut().filter(|e| e.table == "orders") {
            entity.concurrency = "lastWriteWins".into();
        }
    });
}

fn action(id: &str, steps: Vec<Value>) -> Value {
    let steps: Vec<Value> = steps
        .into_iter()
        .enumerate()
        .map(|(i, mut s)| {
            s["id"] = json!(format!("{id}-{i}"));
            s
        })
        .collect();
    json!({"id": id, "name": id, "onError": "stop", "steps": steps})
}

fn trigger(id: &str, event: &str, action: &str) -> Value {
    json!({
        "id": id, "name": id, "table": "orders", "event": event, "actionId": action,
        "mode": "sync", "enabled": true, "maxAttempts": 1, "backoffMs": 0,
    })
}

fn configure(h: &mut Harness, actions: Value, triggers: Value, queries: Value) {
    orders_without_conflict_checks(h);
    edit_config(h, |c| {
        c.actions = serde_json::from_value(actions).unwrap();
        c.triggers = serde_json::from_value(triggers).unwrap();
        c.saved_queries = serde_json::from_value(queries).unwrap();
    });
}

fn column(h: &Harness, table: &str, name: &str) -> Vec<String> {
    let page = h.read_page(table);
    let index = page.columns.iter().position(|c| c.name == name).expect("column");
    page.rows.iter().map(|r| shown(&r[index])).collect()
}

fn audit(h: &Harness) -> Vec<String> {
    let mut messages = column(h, "audit", "message");
    messages.sort();
    messages
}

fn insert(h: &Harness, id: i64, total: f64, status: &str) -> Result<Vec<DataValue>, records::WriteError> {
    records::insert(
        &h.label,
        "orders",
        vec![
            named("id", DataValue::Integer(id)),
            named("total", DataValue::Real(total)),
            named("status", text(status)),
        ],
    )
}

#[gpui_kit::test]
fn stores_a_field_set_by_a_before_change_trigger_and_rejects_without_a_row(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    setup(&mut h);
    configure(
        &mut h,
        json!([action("a-stamp", vec![
            json!({"kind": "setField", "field": "status", "value": "upper(record.status)"}),
            json!({"kind": "fail", "message": "'Total must be positive'", "when": "record.total <= 0"}),
        ])]),
        json!([trigger("t-before", "beforeChange", "a-stamp")]),
        json!([]),
    );
    insert(&h, 4, 5.0, "new").expect("insert");
    assert_eq!(column(&h, "orders", "status")[3], "NEW");

    let error = insert(&h, 5, -1.0, "bad").unwrap_err();
    assert_eq!(error.error.message, "Not saved: Total must be positive");
    assert_eq!(h.read_page("orders").total, 4);
}

#[gpui_kit::test]
fn writes_an_audit_row_from_a_deleted_trigger_with_the_deleted_rows_values(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    setup(&mut h);
    configure(
        &mut h,
        json!([action("a-audit", vec![json!({
            "kind": "createRecord", "table": "audit",
            "values": {"message": "'deleted ' & record.id & ' ' & record.status & ' ' & old.total"},
        })])]),
        json!([trigger("t-del", "deleted", "a-audit")]),
        json!([]),
    );
    records::delete(&h.label, "orders", vec![DataValue::Integer(2)], RecordWriteMeta::default())
        .expect("delete");
    assert_eq!(h.read_page("orders").total, 2);
    assert_eq!(audit(&h), ["deleted 2 open 20"]);
}

fn run_query(h: &Harness, id: &str, params: Vec<NamedValue>) -> Result<crate::queries::action::ActionRun, records::WriteError> {
    records::run_action_query(&h.label, id, params, false, 0)
}

#[gpui_kit::test]
fn applies_before_change_fields_to_every_row_of_an_update_action_query_or_rejects_it(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    setup(&mut h);
    configure(
        &mut h,
        json!([action("a-date", vec![
            json!({"kind": "setField", "field": "due", "value": "today()"}),
            json!({"kind": "fail", "message": "'Orders over 25 stay open'", "when": "record.total > 25"}),
        ])]),
        json!([trigger("t-before", "beforeChange", "a-date")]),
        json!([{
            "id": "q-close", "name": "Close orders",
            "sql": "UPDATE orders SET status = 'closed' WHERE total < $max",
            "parameters": [{"name": "max", "logicalType": "number"}],
            "action": {"kind": "update", "table": "orders"},
        }]),
    );
    let before = h.read_page("orders").rows;

    // Order 3 is rejected, so the whole table stays as it was.
    let error = run_query(&h, "q-close", vec![named("max", DataValue::Integer(100))]).unwrap_err();
    assert_eq!(error.error.message, "Not saved: Orders over 25 stay open");
    assert_eq!(h.read_page("orders").rows, before);

    let run = run_query(&h, "q-close", vec![named("max", DataValue::Integer(25))]).expect("run");
    assert_eq!((run.dry_run, run.changed), (false, 2));
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    assert_eq!(column(&h, "orders", "due"), [today.as_str(), today.as_str(), "2024-01-31"]);
    assert_eq!(column(&h, "orders", "status"), ["closed", "closed", "open"]);
}

#[gpui_kit::test]
fn applies_a_before_change_field_to_rows_an_insert_action_query_creates(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    setup(&mut h);
    configure(
        &mut h,
        json!([action("a-set", vec![
            json!({"kind": "setField", "field": "status", "value": "record.status & '!'"}),
        ])]),
        json!([trigger("t-before", "beforeChange", "a-set")]),
        json!([{
            "id": "q-copy", "name": "Copy orders", "parameters": [],
            "sql": "INSERT INTO orders (id, total, status) SELECT id + 100, total, 'copy' FROM orders",
            "action": {"kind": "insert", "table": "orders"},
        }]),
    );
    let run = run_query(&h, "q-copy", vec![]).expect("run");
    assert_eq!(run.changed, 3);
    assert_eq!(&column(&h, "orders", "status")[3..], ["copy!", "copy!", "copy!"]);
    assert_eq!(h.read_page("orders").total, 6);
}

#[gpui_kit::test]
fn fires_deleted_triggers_once_per_row_of_a_delete_action_query(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    setup(&mut h);
    configure(
        &mut h,
        json!([action("a-audit", vec![json!({
            "kind": "createRecord", "table": "audit",
            "values": {"message": "'purged ' & record.id & ' total ' & old.total"},
        })])]),
        json!([trigger("t-del", "deleted", "a-audit")]),
        json!([{
            "id": "q-purge", "name": "Purge small orders", "parameters": [],
            "sql": "DELETE FROM orders WHERE total <= 20",
            "action": {"kind": "delete", "table": "orders"},
        }]),
    );
    run_query(&h, "q-purge", vec![]).expect("run");
    assert_eq!(h.read_page("orders").total, 1);
    assert_eq!(audit(&h), ["purged 1 total 10", "purged 2 total 20"]);
}

// ---- Step editor, jobs panel, mode -------------------------------------------

fn mode(h: &mut Harness) -> Entity<AutomationMode> {
    h.switch_mode(ModeId::Automation);
    automation_mode(h)
}

fn automation_mode(h: &mut Harness) -> Entity<AutomationMode> {
    let view = h.read_shell(|s, _| s.mode_view()).expect("mode view");
    view.downcast().expect("AutomationMode")
}

#[gpui_kit::test]
fn adds_a_fail_step_and_edits_its_message_expression(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    mode(&mut h);
    h.click("new-action");
    choose(&mut h, "steps-kind", "Fail with message");
    h.click("steps-add");
    assert_eq!(h.label("step-1-title"), "Step 1: Fail with message");
    h.type_into("step-1-message", "'Over the limit'");
    let step = serde_json::to_value(&h.config().actions[0].steps[0]).unwrap();
    assert_eq!((step["kind"].as_str(), step["message"].as_str()), (Some("fail"), Some("'Over the limit'")));
    // Every step can be made conditional.
    assert!(h.exists("step-1-when"));
}

#[gpui_kit::test]
fn moves_and_removes_steps_and_edits_the_branches_of_a_condition(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    mode(&mut h);
    h.click("new-action");
    for kind in ["Show message", "If / else"] {
        choose(&mut h, "steps-kind", kind);
        h.click("steps-add");
    }
    assert_eq!(h.label("step-1-title"), "Step 1: Show message");
    assert_eq!(h.label("step-2-title"), "Step 2: If / else");
    h.click("step-2-up");
    assert_eq!(h.label("step-1-title"), "Step 1: If / else");

    choose(&mut h, "step-1-then-kind", "Fail with message");
    h.click("step-1-then-add");
    assert_eq!(h.label("step-1-then-1-title"), "Then step 1: Fail with message");
    h.fill("step-1-when", "record.total > 1");
    let steps = h.config().actions[0].steps.clone();
    let condition = serde_json::to_value(&steps[0]).unwrap();
    assert_eq!(condition["when"], "record.total > 1");
    assert_eq!(condition["then"][0]["kind"], "fail");
    assert_eq!(condition["else"], json!([]));

    h.click("step-1-then-1-remove");
    assert!(!h.exists("step-1-then-1-title"));
    h.click("step-2-remove");
    assert_eq!(h.config().actions[0].steps.len(), 1);
}

#[gpui_kit::test]
fn edits_navigation_and_form_steps_with_the_documents_objects(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    edit_store(&mut h, |c| {
        c.design.forms = serde_json::from_value(json!([{"id": "f1", "name": "Orders form"}])).unwrap();
    });
    mode(&mut h);
    h.click("new-action");
    choose(&mut h, "steps-kind", "Navigate");
    h.click("steps-add");
    choose(&mut h, "step-1-target", "Orders form");
    h.fill("step-1-mode", "edit");
    let target = serde_json::to_value(&h.config().actions[0].steps[0]).unwrap()["target"].clone();
    assert_eq!(target, json!({"kind": "form", "id": "f1", "mode": "edit"}));
    choose(&mut h, "step-1-target-kind", "report");
    let target = serde_json::to_value(&h.config().actions[0].steps[0]).unwrap()["target"].clone();
    assert_eq!(target, json!({"kind": "report", "id": ""}));
}

#[gpui_kit::test]
fn edits_trigger_fields_and_hides_async_options_for_sync_triggers(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.create_table("orders", &[("id", "INTEGER", 1)]);
    mode(&mut h);
    h.click("new-action");
    h.click("tab-triggers");
    h.click("new-trigger");
    assert_eq!(h.config().triggers[0].table, "orders");
    assert_eq!(h.config().triggers[0].action_id, h.config().actions[0].id);
    assert!(!h.exists("trigger-max-attempts"));
    choose(&mut h, "trigger-mode", "Asynchronously, on the job queue");
    h.fill("trigger-max-attempts", "0");
    assert_eq!(h.label("trigger-max-attempts-problem"), "Enter a whole number of at least 1");
    assert_eq!(h.config().triggers[0].max_attempts, 3, "an invalid number is not stored");
    h.fill("trigger-max-attempts", "5");
    assert_eq!(h.config().triggers[0].max_attempts, 5);
    h.fill("trigger-condition", "record.id >");
    assert!(h.exists("trigger-condition-problem"));
    h.fill("trigger-condition", "record.id > 1");

    choose(&mut h, "trigger-event", "Before a record is saved");
    assert!(!h.exists("trigger-mode"), "before-change triggers are always sync");
    assert!(h.label("trigger-help").starts_with("Runs before each new or changed record"));
    h.click("delete-trigger");
    assert!(h.config().triggers.is_empty());
    assert_eq!(h.label("select-trigger"), "Select or create a trigger.");
}

#[gpui_kit::test]
fn reveals_an_action_or_trigger_from_another_mode(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    edit_store(&mut h, |c| {
        c.actions = serde_json::from_value(json!([
            {"id": "a1", "name": "First", "steps": []},
            {"id": "a2", "name": "Second", "steps": []},
        ]))
        .unwrap();
        c.triggers = serde_json::from_value(json!([trigger("t1", "created", "a2")])).unwrap();
    });
    h.switch_mode(ModeId::Design);
    let shell = h.shell();
    h.update(|window, cx| {
        shell.update(cx, |s, cx| {
            s.request_reveal(
                RevealTarget::Automation { tab: AutomationTab::Actions, object_id: "a2".into() },
                window,
                cx,
            )
        })
    });
    h.settle();
    assert_eq!(h.mode(), ModeId::Automation);
    let view = automation_mode(&mut h);
    let (tab, selected) = h.cx.read(|cx| {
        let m = view.read(cx);
        (m.tab, m.actions.read(cx).selected().map(str::to_string))
    });
    assert_eq!((tab, selected.as_deref()), (Tab::Actions, Some("a2")));
    assert_eq!(h.label("action-item-2"), "Second");

    // A trigger reveal switches the tab, also while the mode is already open.
    h.update(|window, cx| {
        shell.update(cx, |s, cx| {
            s.request_reveal(
                RevealTarget::Automation { tab: AutomationTab::Triggers, object_id: "t1".into() },
                window,
                cx,
            )
        })
    });
    h.settle();
    let (tab, selected) = h.cx.read(|cx| {
        let m = view.read(cx);
        (m.tab, m.triggers.read(cx).selected().map(str::to_string))
    });
    assert_eq!((tab, selected.as_deref()), (Tab::Triggers, Some("t1")));
}

#[gpui_kit::test]
fn lists_automation_problems_after_the_definitions_settle(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    mode(&mut h);
    h.click("new-action");
    choose(&mut h, "steps-kind", "Create record");
    h.click("steps-add");
    assert!(!h.exists("issues-toggle"), "validation waits for the definitions to settle");
    h.advance(Duration::from_millis(400));
    assert_eq!(h.label("issues-toggle"), "2 automation problems");
    h.click("issues-toggle");
    let problems: Vec<String> = (1..=2).map(|i| h.label(id(&format!("issue-{i}")))).collect();
    assert!(problems.iter().any(|p| p.contains("table is empty")), "{problems:?}");
}

// ---- Jobs panel ----------------------------------------------------------------

fn job(id: &str, status: &str) -> crate::jobs::Job {
    serde_json::from_value(json!({
        "id": id, "documentId": "d", "triggerId": "t1", "actionId": "a1", "payload": {},
        "idempotencyKey": id, "status": status, "attempts": 1, "maxAttempts": 3, "backoffMs": 0,
        "nextRunAt": "2026-10-05T00:00:00.000Z", "leaseUntil": null,
        "createdAt": "2026-10-05T00:00:00.000Z", "updatedAt": "2026-10-05T00:00:00.000Z",
        "lastError": null,
    }))
    .unwrap()
}

#[gpui_kit::test]
fn shows_the_list_for_the_current_filter_when_an_older_request_answers_last(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    let view = mode(&mut h);
    h.click("tab-jobs");
    let panel: Entity<JobsPanel> = h.cx.read(|cx| view.read(cx).jobs.clone());
    // Two requests are in flight: the first for all statuses, then one for `cancelled`.
    let (first, second) = h.cx.update(|cx| {
        panel.update(cx, |p, _| {
            let first = p.begin_load();
            p.status = "cancelled".into();
            (first, p.begin_load())
        })
    });
    // The newer answer arrives first; the older one must not replace it.
    h.cx.update(|cx| {
        panel.update(cx, |p, cx| {
            p.finish_load(second, Ok(vec![job("j-cancelled", "cancelled")]), cx);
            p.finish_load(first, Ok(vec![job("j-queued", "queued")]), cx);
        })
    });
    h.settle();
    assert_eq!(h.label("job-1-status"), "cancelled");
    assert!(!h.exists("job-2-status"));
    let jobs = h.cx.read(|cx| panel.read(cx).jobs.clone());
    assert_eq!(jobs.len(), 1);
}
