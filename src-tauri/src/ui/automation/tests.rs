//! Automation mode flows (port of `tests/integration/automation.test.tsx`). Imports are
//! explicit: `gpui_kit::*` would shadow `#[test]`.
//!
//! The editors are driven through the window like a user would; the record writes go
//! through `records`, which runs the hooks the shell installed for the window.

use crate::data::DataValue;
use crate::ui::shell::ModeId;
use crate::ui::testing::{shown, Harness};
use gpui_kit::{SharedString, TestAppContext};
use std::time::Duration;

pub fn id(name: &str) -> SharedString {
    SharedString::from(name.to_string())
}

/// Opens the options menu of the choice `button` and picks the option labeled `option`.
pub fn choose(h: &mut Harness, button: &str, option: &str) {
    h.click(id(button));
    let mut seen = vec![];
    for index in 0..64usize {
        let label = h.update(|w, _| {
            use gpui_kit::test::TestWindowExt as _;
            w.within("popup-menu")
                .try_find(index)
                .map(|item| item.label().unwrap_or_default().to_string())
        });
        match label {
            Some(label) if label == option => {
                h.click_in("popup-menu", index);
                return;
            }
            Some(label) => seen.push(label),
            None => break,
        }
    }
    panic!("{button} has no option {option:?}; options are {seen:?}");
}

/// Polls (with real sleeps: the job worker is a thread) until `done` holds.
pub fn wait_until(h: &mut Harness, what: &str, mut done: impl FnMut(&mut Harness) -> bool) {
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

pub fn text(value: &str) -> DataValue {
    DataValue::Text(value.into())
}

pub fn setup_tables(h: &mut Harness) {
    h.create_table("orders", &[("id", "INTEGER", 1), ("status", "TEXT", 0)]);
    h.create_table("audit", &[("id", "INTEGER", 1), ("message", "TEXT", 0)]);
}

pub fn insert_order(h: &mut Harness, order: i64, status: &str) {
    crate::records::insert(
        &h.label,
        "orders",
        vec![
            crate::data::NamedValue { column: "id".into(), value: DataValue::Integer(order) },
            crate::data::NamedValue { column: "status".into(), value: text(status) },
        ],
    )
    .expect("insert order");
    h.records_changed();
}

/// The messages in `audit`, sorted.
pub fn audit_messages(h: &mut Harness) -> Vec<String> {
    let mut messages: Vec<String> = h
        .read_page("audit")
        .rows
        .iter()
        .map(|row| shown(&row[1]))
        .collect();
    messages.sort();
    messages
}

/// Defines an action with one record step: the New action button, a name, the step and
/// its table and value rows.
pub fn define_action(h: &mut Harness, name: &str, kind: &str, table: &str, values: &[(&str, &str)]) {
    h.click("new-action");
    h.fill("action-name", name);
    choose(h, "steps-kind", kind);
    h.click("steps-add");
    assert_eq!(h.label("step-1-title"), format!("Step 1: {kind}"));
    choose(h, "step-1-table", table);
    for (n, (column, expr)) in values.iter().enumerate() {
        let n = n + 1;
        h.click("step-1-values-add");
        h.fill(id(&format!("step-1-values-{n}-key")), column);
        h.fill(id(&format!("step-1-values-{n}-expr")), expr);
    }
    let saved = h.config().actions.iter().find(|a| a.name == name).map(|a| a.steps.len());
    assert_eq!(saved, Some(1), "{name} was not saved with its step");
}

pub fn define_trigger(h: &mut Harness, name: &str, action: &str, mode: &str) {
    h.click("tab-triggers");
    h.click("new-trigger");
    h.fill("trigger-name", name);
    choose(h, "trigger-table", "orders");
    choose(h, "trigger-event", "Record created");
    choose(h, "trigger-action", action);
    choose(
        h,
        "trigger-mode",
        if mode == "async" {
            "Asynchronously, on the job queue"
        } else {
            "Synchronously, inside the write"
        },
    );
    let saved = h.config().triggers.into_iter().find(|t| t.name == name).expect("trigger saved");
    assert_eq!(saved.table, "orders");
    assert_eq!(format!("{:?}", saved.mode).to_lowercase(), mode);
}

fn open_automation(h: &mut Harness) {
    h.switch_mode(ModeId::Automation);
    assert_eq!(h.label("mode-title"), "Automation");
}

#[gpui_kit::test]
fn defines_a_status_transition_action_in_the_editor_and_test_runs_it(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    setup_tables(&mut h);
    h.insert_row("orders", &[("id", DataValue::Integer(1)), ("status", text("open"))]);
    open_automation(&mut h);
    define_action(&mut h, "Close order", "Update record", "orders", &[("status", "'closed'")]);
    assert_eq!(h.label("step-1-match-kind"), "The current record");
    h.fill("step-1-when", "record.status = 'open'");

    h.fill("step-1-values-1-expr", "'closed' +");
    assert_eq!(h.label("step-1-values-1-expr-problem"), "Unexpected end of expression");
    h.fill("step-1-values-1-expr", "'closed'");
    assert!(!h.exists("step-1-values-1-expr-problem"));

    h.fill("test-sample", r#"{"id": 1, "status": "open"}"#);
    h.click("test-run");
    assert_eq!(h.label("test-result"), "Test run succeeded");
    assert_eq!(shown(&h.read_page("orders").rows[0][1]), "closed");

    h.fill("test-sample", r#"{"id": 1, "status": "closed"}"#);
    h.click("test-run");
    assert_eq!(h.label("test-log-1-result"), "Skipped");

    let saved = &h.config().actions[0];
    assert_eq!(saved.name, "Close order");
    assert_eq!(format!("{:?}", saved.on_error), "Stop");
    let step = serde_json::to_value(&saved.steps[0]).unwrap();
    assert_eq!(step["kind"], "updateRecord");
    assert_eq!(step["table"], "orders");
    assert_eq!(step["match"], "current");
    assert_eq!(step["values"], serde_json::json!({"status": "'closed'"}));
    assert_eq!(step["when"], "record.status = 'open'");
}

#[gpui_kit::test]
fn runs_a_sync_created_trigger_that_writes_an_audit_row_inside_the_insert(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    setup_tables(&mut h);
    open_automation(&mut h);
    define_action(
        &mut h,
        "Audit",
        "Create record",
        "audit",
        &[("message", "'created ' & record.id & ' ' & record.status")],
    );
    define_trigger(&mut h, "Audit new orders", "Audit", "sync");
    insert_order(&mut h, 7, "open");
    assert_eq!(audit_messages(&mut h), ["created 7 open"]);
}

fn job_rows(h: &mut Harness) -> Vec<(usize, String, String)> {
    (1..=6usize)
        .filter_map(|n| {
            let status = h.try_label(id(&format!("job-{n}-status")))?;
            let error = h.label(id(&format!("job-{n}-error")));
            Some((n, status, error))
        })
        .collect()
}

fn job_with(h: &mut Harness, status: &str, error: &str) -> Option<usize> {
    job_rows(h)
        .into_iter()
        .find(|(_, s, e)| s == status && e.contains(error))
        .map(|(n, ..)| n)
}

#[gpui_kit::test]
fn runs_async_triggers_on_the_durable_queue_with_retry_after_failure_and_cancel(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    setup_tables(&mut h);
    open_automation(&mut h);
    define_action(
        &mut h,
        "Audit later",
        "Create record",
        "audit",
        &[("message", "'queued ' & record.id")],
    );
    define_trigger(&mut h, "Async audit", "Audit later", "async");

    insert_order(&mut h, 1, "open");
    wait_until(&mut h, "the first audit row", |h| audit_messages(h) == ["queued 1"]);
    h.click("tab-jobs");
    wait_until(&mut h, "a succeeded job", |h| job_with(h, "succeeded", "").is_some());

    // Break the action and allow a single attempt: the job fails.
    h.click("tab-actions");
    h.fill("step-1-values-1-expr", "1 + 'x'");
    h.click("tab-triggers");
    h.fill("trigger-max-attempts", "1");
    assert_eq!(h.config().triggers[0].max_attempts, 1);
    insert_order(&mut h, 2, "open");
    h.click("tab-jobs");
    wait_until(&mut h, "a failed job", |h| job_with(h, "failed", "add number and text").is_some());
    let failed = job_with(&mut h, "failed", "").unwrap();
    h.click(id(&format!("job-{failed}-history")));
    wait_until(&mut h, "the attempt history", |h| {
        h.try_label(id(&format!("job-{failed}-attempt-1")))
            .is_some_and(|l| l.starts_with("Attempt 1: failed"))
    });

    // Fix the action and retry the job.
    h.click("tab-actions");
    h.fill("step-1-values-1-expr", "'retried ' & record.id");
    h.click("tab-jobs");
    h.click(id(&format!("job-{failed}-retry")));
    wait_until(&mut h, "the retried audit row", |h| {
        audit_messages(h) == ["queued 1", "retried 2"]
    });
    wait_until(&mut h, "no failed job", |h| job_with(h, "failed", "").is_none());

    // Fail again with a long backoff: the job waits in the queue and is cancelled.
    h.click("tab-actions");
    h.fill("step-1-values-1-expr", "1 + 'x'");
    h.click("tab-triggers");
    h.fill("trigger-max-attempts", "3");
    h.fill("trigger-backoff", "600000");
    assert_eq!(h.config().triggers[0].backoff_ms, 600_000);
    insert_order(&mut h, 3, "open");
    h.click("tab-jobs");
    choose(&mut h, "jobs-status", "queued");
    wait_until(&mut h, "a queued job with its error", |h| {
        job_with(h, "queued", "add number and text").is_some()
    });
    let queued = job_with(&mut h, "queued", "add").unwrap();
    h.click(id(&format!("job-{queued}-cancel")));
    wait_until(&mut h, "the job to leave the queued list", |h| {
        job_with(h, "queued", "").is_none()
    });
    assert!(!h.exists("jobs-error"));
    choose(&mut h, "jobs-status", "cancelled");
    wait_until(&mut h, "the cancelled job", |h| job_with(h, "cancelled", "").is_some());
    assert_eq!(audit_messages(&mut h), ["queued 1", "retried 2"]);
}

#[gpui_kit::test]
fn deduplicates_jobs_by_idempotency_key(cx: &mut TestAppContext) {
    let h = Harness::new_document(cx);
    let job = || crate::jobs::EnqueueJob {
        trigger_id: "t1".into(),
        action_id: "a1".into(),
        payload: serde_json::json!({}),
        idempotency_key: "same-key".into(),
        ..Default::default()
    };
    let first = crate::jobs::enqueue_job(h.label.clone(), job()).unwrap();
    let second = crate::jobs::enqueue_job(h.label.clone(), job()).unwrap();
    assert_eq!(first.id, second.id);
    let listed = crate::jobs::list_jobs(h.label.clone(), None).unwrap();
    assert_eq!(listed.len(), 1);
}
