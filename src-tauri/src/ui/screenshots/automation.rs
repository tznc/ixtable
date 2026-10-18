//! Automation mode captures (`automation-*`): the action list, the step editor with several
//! step kinds and a test run, the trigger editor, and the job queue with run history.

use crate::data::{DataValue, NamedValue};
use crate::ui::data::testkit::choose;
use crate::ui::screenshot::context;
use crate::ui::shell::ModeId;
use crate::ui::testing::{Harness, UiContext, shown};
use gpui_kit::SharedString;
use std::time::Duration;

const WINDOW: (f32, f32) = (1280.0, 1100.0);
const TALL: (f32, f32) = (1280.0, 1900.0);

fn id(name: &str) -> SharedString {
    SharedString::from(name.to_string())
}

fn wait_until<C: UiContext>(h: &mut Harness<'_, C>, what: &str, mut done: impl FnMut(&mut Harness<'_, C>) -> bool) {
    for _ in 0..400 {
        h.settle();
        if done(h) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
        h.advance(Duration::from_millis(500));
    }
    panic!("timed out waiting for {what}");
}

fn setup<C: UiContext>(h: &mut Harness<'_, C>) {
    h.create_table("orders", &[("id", "INTEGER", 1), ("status", "TEXT", 0), ("total", "REAL", 0)]);
    h.create_table("audit", &[("id", "INTEGER", 1), ("message", "TEXT", 0)]);
    h.insert_row(
        "orders",
        &[("id", DataValue::Integer(1)), ("status", DataValue::Text("open".into())), ("total", DataValue::Real(40.0))],
    );
    h.database_changed();
}

fn insert_order<C: UiContext>(h: &mut Harness<'_, C>, order: i64) {
    crate::records::insert(
        &h.label,
        "orders",
        vec![
            NamedValue { column: "id".into(), value: DataValue::Integer(order) },
            NamedValue { column: "status".into(), value: DataValue::Text("open".into()) },
        ],
    )
    .expect("insert order");
    h.records_changed();
}

/// A new action with one record step on `table`.
fn record_action<C: UiContext>(h: &mut Harness<'_, C>, name: &str, kind: &str, table: &str, values: &[(&str, &str)]) {
    h.click("new-action");
    h.fill("action-name", name);
    choose(h, "steps-kind", kind);
    h.click("steps-add");
    choose(h, "step-1-table", table);
    for (n, (column, expr)) in values.iter().enumerate() {
        let n = n + 1;
        h.click("step-1-values-add");
        h.fill(id(&format!("step-1-values-{n}-key")), column);
        h.fill(id(&format!("step-1-values-{n}-expr")), expr);
    }
}

fn job_with<C: UiContext>(h: &mut Harness<'_, C>, status: &str) -> Option<usize> {
    (1..=6usize).find(|n| h.try_label(id(&format!("job-{n}-status"))).is_some_and(|s| s.eq_ignore_ascii_case(status)))
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn automation_actions_and_steps() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, TALL);
    h.click("new-document");
    setup(&mut h);
    h.switch_mode(ModeId::Automation);
    assert_eq!(h.label("mode-title"), "Automation");

    record_action(&mut h, "Close order", "Update record", "orders", &[("status", "'closed'")]);
    h.fill("step-1-when", "record.status = 'open'");
    for kind in ["Show message", "If / else"] {
        choose(&mut h, "steps-kind", kind);
        h.click("steps-add");
    }
    h.fill("step-2-text", "'Closing order ' & record.id");
    choose(&mut h, "step-3-then-kind", "Fail with message");
    h.click("step-3-then-add");
    h.fill("step-3-when", "record.total > 100");
    h.type_into("step-3-then-1-message", "'Orders over 100 need approval'");
    assert_eq!(h.label("step-3-title"), "Step 3: If / else");
    assert_eq!(h.label("step-3-then-1-title"), "Then step 1: Fail with message");
    h.capture(
        "automation-01-step-editor",
        &[
            "The Actions tab lists the new action and edits Close order as three steps: Update record, Show message and an If / else.",
            "The Update record step has a 'Only when' expression and a status value row; the If / else step holds a nested Fail with message step.",
        ],
    );

    h.fill("test-sample", r#"{"id": 1, "status": "open", "total": 40}"#);
    h.click("test-run");
    assert_eq!(h.label("test-result"), "Test run succeeded");
    h.capture(
        "automation-02-test-run",
        &[
            "The Test run section shows 'Test run succeeded' with the log of each step beneath the sample record.",
            "The Update record step reports its result and the condition step reports it ran.",
        ],
    );

    record_action(&mut h, "Audit new order", "Create record", "audit", &[("message", "'created ' & record.id")]);
    assert_eq!(h.config().actions.len(), 2);
    h.click("action-item-1");
    assert!(h.exists("action-item-2"));
    h.capture(
        "automation-03-actions-list",
        &["The action list shows both actions, Close order and Audit new order, with the selected one editable beside it."],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn automation_triggers_and_jobs() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, WINDOW);
    h.click("new-document");
    setup(&mut h);
    h.switch_mode(ModeId::Automation);
    record_action(&mut h, "Audit later", "Create record", "audit", &[("message", "'queued ' & record.id")]);

    h.click("tab-triggers");
    h.click("new-trigger");
    h.fill("trigger-name", "Audit new orders");
    choose(&mut h, "trigger-table", "orders");
    choose(&mut h, "trigger-event", "Record created");
    choose(&mut h, "trigger-action", "Audit later");
    choose(&mut h, "trigger-mode", "Asynchronously, on the job queue");
    h.fill("trigger-max-attempts", "1");
    let saved = h.config().triggers.into_iter().next().expect("trigger saved");
    assert_eq!((saved.table.as_str(), saved.max_attempts), ("orders", 1));
    h.capture(
        "automation-04-trigger-editor",
        &[
            "The Triggers tab edits Audit new orders: table orders, event Record created, action Audit later.",
            "Asynchronous mode shows the retry options (max attempts and backoff) that synchronous triggers hide.",
        ],
    );

    insert_order(&mut h, 2);
    wait_until(&mut h, "the audit row", |h| h.read_page("audit").total == 1);
    h.click("tab-actions");
    h.fill("step-1-values-1-expr", "1 + 'x'");
    insert_order(&mut h, 3);
    h.click("tab-jobs");
    wait_until(&mut h, "a succeeded and a failed job", |h| {
        job_with(h, "succeeded").is_some() && job_with(h, "failed").is_some()
    });
    let failed = job_with(&mut h, "failed").unwrap();
    h.click(id(&format!("job-{failed}-history")));
    wait_until(&mut h, "the attempt history", |h| {
        h.try_label(id(&format!("job-{failed}-attempt-1-result"))).is_some_and(|l| l == "Failed")
    });
    assert!(shown(&DataValue::Text(h.label(id(&format!("job-{failed}-error"))))).contains("add number and text"));
    h.capture(
        "automation-05-jobs-history",
        &[
            "The Jobs tab lists one succeeded job and one failed job with its error 'add number and text'.",
            "The failed job's attempt history is expanded with attempt 1 marked Failed, and offers Retry.",
        ],
    );
}
