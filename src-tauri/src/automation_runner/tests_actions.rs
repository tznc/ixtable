//! Action runner tests: steps, failure modes, composition and permissions. Ports of
//! `automation-runner.test.ts`, `automation-fail-step.test.ts` and the headless-context cases.

use super::context::{ActionHost, TriggerEnv};
use super::host::HeadlessHost;
use super::runner::{run_action, run_action_by_id};
use super::tests_support::{
    action, action_with, error_has, events, host, obj, orders_db, step, with_actions,
};
use super::worker::headless_context;
use crate::manager::AppError;
use crate::records::{RecordHook, RecordOperation, RecordWrite, WriteResult};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[test]
fn updates_the_current_record_creates_rows_sets_state_and_messages() {
    let db = orders_db();
    let a = action(
        "a",
        vec![
            step(json!({"kind": "updateRecord", "table": "orders", "match": "current",
                "values": {"status": "'shipped'"}})),
            step(json!({"kind": "createRecord", "table": "audit",
                "values": {"message": "'order ' & record.id & ' is ' & record.status"},
                "storeAs": "entry"})),
            step(json!({"kind": "setState", "scope": "app", "key": "last", "value": "results.entry.id"})),
            step(json!({"kind": "message", "text": "'Logged ' & app.last & ' by ' & app.user", "tone": "info"})),
        ],
    );
    with_actions(&db, vec![a.clone()]);
    let h = host();
    let mut ctx = db.ctx(h.clone());
    ctx.record = Some(obj(json!({"id": 1, "status": "open"})));
    ctx.app = obj(json!({"user": "ana"}));
    let result = run_action(&a, ctx);
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(db.column("orders", "status")[0], json!("shipped"));
    assert_eq!(db.rows("audit").len(), 1);
    assert_eq!(db.rows("audit")[0]["message"], json!("order 1 is shipped"));
    assert_eq!(
        events(&h),
        vec![json!(["state", "app", "last", 1]), json!(["notify", "Logged 1 by ana", "info"])]
    );
    assert_eq!(h.events().last(), Some(&json!(["refresh"])));
    let kinds: Vec<_> = result.steps.iter().map(|s| (s.kind.as_str(), s.ok)).collect();
    assert_eq!(
        kinds,
        [("updateRecord", true), ("createRecord", true), ("setState", true), ("message", true)]
    );
}

#[test]
fn matches_rows_by_expression_deletes_them_and_fails_when_nothing_matches() {
    let db = orders_db();
    let a = action(
        "a",
        vec![step(json!({"kind": "deleteRecord", "table": "orders", "match": {"status": "params.status"}}))],
    );
    with_actions(&db, vec![a.clone()]);
    let make = || {
        let mut ctx = db.ctx(host());
        ctx.params = Some(obj(json!({"status": "open"})));
        ctx
    };
    assert!(run_action(&a, make()).ok);
    assert!(db.rows("orders").is_empty());
    let again = run_action(&a, make());
    assert!(!again.ok);
    assert!(error_has(&again.error, "No orders rows match"), "{:?}", again.error);
}

#[test]
fn skips_steps_whose_when_is_false_and_branches_on_conditions() {
    let db = orders_db();
    let a = action(
        "a",
        vec![
            step(json!({"kind": "message", "text": "'skipped'", "when": "record.total > 100"})),
            step(json!({"kind": "condition", "when": "record.total > 5",
                "then": [step(json!({"kind": "message", "text": "'big'"}))],
                "else": [step(json!({"kind": "message", "text": "'small'"}))]})),
        ],
    );
    with_actions(&db, vec![a.clone()]);
    let h = host();
    let mut ctx = db.ctx(h.clone());
    ctx.record = Some(obj(json!({"id": 1, "total": 10})));
    let result = run_action(&a, ctx);
    assert_eq!(events(&h), vec![json!(["notify", "big", "info"])]);
    assert!(result.steps[0].skipped && result.steps[0].ok);
    let paths: Vec<_> = result.steps.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(paths, ["0", "1", "1.then.0"]);
}

#[test]
fn runs_saved_queries_and_stores_rows_for_later_steps() {
    let db = orders_db();
    let a = action(
        "a",
        vec![
            step(json!({"kind": "runQuery", "queryId": "q1", "params": {}, "storeAs": "open"})),
            step(json!({"kind": "message", "text": "'Total ' & sum(results.open.total)"})),
        ],
    );
    with_actions(&db, vec![a.clone()]);
    let h = host();
    let result = run_action(&a, db.ctx(h.clone()));
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(events(&h), vec![json!(["notify", "Total 30", "info"])]);
    assert_eq!(result.results["open"][0]["id"], json!(1));
}

#[test]
fn navigates_to_existing_targets_and_rejects_missing_ones() {
    let db = orders_db();
    let a = action(
        "a",
        vec![
            step(json!({"kind": "openForm", "formId": "f1", "mode": "edit", "recordId": "record.id"})),
            step(json!({"kind": "openReport", "reportId": "r1", "params": {"id": "record.id"}})),
            step(json!({"kind": "navigate", "target": {"kind": "dashboard", "id": "nope"}})),
        ],
    );
    with_actions(&db, vec![a.clone()]);
    let h = host();
    let mut ctx = db.ctx(h.clone());
    ctx.record = Some(obj(json!({"id": 7})));
    let result = run_action(&a, ctx);
    assert_eq!(
        events(&h),
        vec![
            json!(["navigate", {"kind": "form", "id": "f1", "mode": "edit", "recordId": 7}]),
            json!(["navigate", {"kind": "report", "id": "r1", "params": {"id": 7}}]),
        ]
    );
    assert!(!result.ok);
    assert!(error_has(&result.error, "dashboard nope does not exist"));
}

#[test]
fn passes_evaluated_parameters_to_open_dashboard() {
    let db = orders_db();
    db.set_config(|c| {
        c.dashboards = vec![serde_json::from_value(json!({"id": "d1", "name": "Sales"})).unwrap()];
    });
    let a = action(
        "a",
        vec![step(json!({"kind": "openDashboard", "dashboardId": "d1", "params": {"region": "record.region"}}))],
    );
    with_actions(&db, vec![a.clone()]);
    let h = host();
    let mut ctx = db.ctx(h.clone());
    ctx.record = Some(obj(json!({"region": "East"})));
    assert!(run_action(&a, ctx).ok);
    assert_eq!(
        events(&h),
        vec![json!(["navigate", {"kind": "dashboard", "id": "d1", "params": {"region": "East"}}])]
    );
}

#[test]
fn reports_empty_and_invalid_expressions_as_step_failures() {
    let db = orders_db();
    let empty = action("empty", vec![step(json!({"kind": "message", "text": " "}))]);
    let bad = action("bad", vec![step(json!({"kind": "message", "text": "1 +"}))]);
    with_actions(&db, vec![empty.clone(), bad.clone()]);
    assert!(error_has(&run_action(&empty, db.ctx(host())).error, "Message has no expression"));
    assert!(!run_action(&bad, db.ctx(host())).ok);
}

fn failing() -> Value {
    json!({"kind": "createRecord", "table": "audit", "values": {"message": "'bad'"}})
}

fn create(message: &str) -> Value {
    json!({"kind": "createRecord", "table": "audit", "values": {"message": format!("'{message}'")}})
}

#[test]
fn stop_ends_at_the_first_failure_and_keeps_earlier_writes() {
    let db = orders_db();
    let a = action("a", vec![step(create("first")), step(failing()), step(create("never"))]);
    with_actions(&db, vec![a.clone()]);
    let result = run_action(&a, db.ctx(host()));
    assert!(!result.ok);
    assert!(error_has(&result.error, "Step 2 (createRecord) failed:"), "{:?}", result.error);
    assert!(error_has(&result.error, "check constraint"), "{:?}", result.error);
    assert_eq!(db.column("audit", "message"), [json!("first")]);
    assert_eq!(result.steps.len(), 2);
}

#[test]
fn continue_logs_the_failure_and_runs_the_remaining_steps() {
    let db = orders_db();
    let a = action_with("a", "continue", vec![step(failing()), step(create("after"))]);
    with_actions(&db, vec![a.clone()]);
    let result = run_action(&a, db.ctx(host()));
    assert!(result.ok);
    let oks: Vec<_> = result.steps.iter().map(|s| s.ok).collect();
    assert_eq!(oks, [false, true]);
    assert_eq!(db.column("audit", "message"), [json!("after")]);
}

#[test]
fn rollback_saves_none_of_the_writes_and_holds_back_effects() {
    let db = orders_db();
    let a = action_with(
        "a",
        "rollback",
        vec![
            step(create("temp")),
            step(json!({"kind": "updateRecord", "table": "orders", "match": {"id": "1"}, "values": {"total": "99"}})),
            step(json!({"kind": "deleteRecord", "table": "orders", "match": {"id": "2"}})),
            step(json!({"kind": "message", "text": "'saved'"})),
            step(json!({"kind": "message", "text": "1 +"})),
        ],
    );
    with_actions(&db, vec![a.clone()]);
    let h = host();
    let result = run_action(&a, db.ctx(h.clone()));
    assert!(!result.ok);
    let error = result.error.clone().unwrap();
    assert!(error.starts_with("Step 5 (message) failed: "), "{error}");
    assert!(error.ends_with("; no record changes were saved"), "{error}");
    assert!(db.rows("audit").is_empty());
    assert_eq!(db.rows("orders").len(), 2);
    assert_eq!(db.column("orders", "total"), [json!(10), json!(20)]);
    assert!(h.events().is_empty());
}

#[test]
fn fail_aborts_with_the_evaluated_message_and_commits_nothing_under_rollback() {
    let db = orders_db();
    let a = action_with(
        "a",
        "rollback",
        vec![
            step(create("temp")),
            step(json!({"kind": "message", "text": "'saved'"})),
            step(json!({"kind": "condition", "when": "record.total > 5",
                "then": [step(json!({"kind": "fail", "message": "'Total ' & record.total & ' is over the limit'"}))],
                "else": []})),
            step(json!({"kind": "message", "text": "'not reached'"})),
        ],
    );
    with_actions(&db, vec![a.clone()]);
    let h = host();
    let mut ctx = db.ctx(h.clone());
    ctx.record = Some(obj(json!({"id": 1, "total": 10})));
    let result = run_action(&a, ctx);
    assert!(!result.ok && result.aborted);
    assert_eq!(
        result.error.as_deref(),
        Some("Total 10 is over the limit; no record changes were saved")
    );
    assert!(db.rows("audit").is_empty());
    assert!(h.events().is_empty());
    let steps: Vec<_> = result.steps.iter().map(|s| (s.path.as_str(), s.kind.as_str(), s.ok)).collect();
    assert_eq!(
        steps,
        [
            ("0", "createRecord", true),
            ("1", "message", true),
            ("2", "condition", false),
            ("2.then.0", "fail", false)
        ]
    );
}

#[test]
fn fail_stops_even_under_continue_is_skipped_when_false_and_propagates_from_nested_actions() {
    let db = orders_db();
    let child = action("child", vec![step(json!({"kind": "fail", "message": "'Credit limit reached'"}))]);
    let skipped = action_with(
        "skipped",
        "continue",
        vec![
            step(json!({"kind": "fail", "message": "'never'", "when": "false"})),
            step(json!({"kind": "message", "text": "'done'"})),
        ],
    );
    let parent = action_with(
        "parent",
        "continue",
        vec![
            step(json!({"kind": "runAction", "actionId": "child"})),
            step(json!({"kind": "message", "text": "'not reached'"})),
        ],
    );
    with_actions(&db, vec![child, skipped.clone(), parent.clone()]);
    let first = host();
    assert!(run_action(&skipped, db.ctx(first.clone())).ok);
    assert_eq!(events(&first), vec![json!(["notify", "done", "info"])]);
    let second = host();
    let result = run_action(&parent, db.ctx(second.clone()));
    assert!(!result.ok && result.aborted);
    assert_eq!(result.error.as_deref(), Some("Credit limit reached"));
    assert!(events(&second).is_empty());
}

/// Records the operations a write passes through the hooks.
#[derive(Default)]
struct Seen(Mutex<Vec<String>>);

impl RecordHook for Seen {
    fn before(&self, write: RecordWrite) -> Result<RecordWrite, AppError> {
        self.0.lock().unwrap().push(format!("before {}", op(write.operation)));
        Ok(write)
    }
    fn after(&self, write: &RecordWrite, result: &WriteResult, _: Option<&str>) -> Result<(), AppError> {
        let result = match result {
            WriteResult::Inserted(_) => "inserted".to_string(),
            WriteResult::Changed(n) => format!("changed {n}"),
        };
        self.0.lock().unwrap().push(format!("after {} {result}", op(write.operation)));
        Ok(())
    }
}

fn op(operation: RecordOperation) -> &'static str {
    match operation {
        RecordOperation::Insert => "insert",
        RecordOperation::Update => "update",
        RecordOperation::Delete => "delete",
    }
}

#[test]
fn rollback_commits_all_writes_in_one_batch_then_runs_effects_and_after_hooks() {
    let db = orders_db();
    let seen = Arc::new(Seen::default());
    let _reg = crate::records::register_hook(&db.label, seen.clone());
    let a = action_with(
        "a",
        "rollback",
        vec![
            step(create("ok")),
            step(json!({"kind": "updateRecord", "table": "orders", "match": "current", "values": {"total": "99"}})),
            step(json!({"kind": "message", "text": "'saved'"})),
        ],
    );
    with_actions(&db, vec![a.clone()]);
    let h = host();
    let mut ctx = db.ctx(h.clone());
    ctx.record = Some(obj(json!({"id": 1})));
    let result = run_action(&a, ctx);
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(db.column("audit", "message"), [json!("ok")]);
    assert_eq!(db.column("orders", "total")[0], json!(99));
    assert_eq!(events(&h), vec![json!(["notify", "saved", "info"])]);
    assert_eq!(
        *seen.0.lock().unwrap(),
        ["before insert", "before update", "after insert inserted", "after update changed 1"]
    );
}

#[test]
fn rollback_with_a_failing_commit_saves_nothing() {
    let db = orders_db();
    let a = action_with(
        "a",
        "rollback",
        vec![
            step(json!({"kind": "updateRecord", "table": "orders", "match": {"id": "1"}, "values": {"total": "5"}})),
            step(failing()),
            step(json!({"kind": "message", "text": "'saved'"})),
        ],
    );
    with_actions(&db, vec![a.clone()]);
    let h = host();
    let result = run_action(&a, db.ctx(h.clone()));
    assert!(!result.ok);
    let error = result.error.clone().unwrap();
    assert!(error.starts_with("Transaction failed: "), "{error}");
    assert!(error_has(&result.error, "check constraint") && error.ends_with("no record changes were saved"));
    assert_eq!(db.column("orders", "total")[0], json!(10));
    assert!(db.rows("audit").is_empty());
    assert!(h.events().is_empty());
}

#[test]
fn a_declined_confirm_cancels_and_rolls_back() {
    let db = orders_db();
    let a = action_with(
        "a",
        "rollback",
        vec![
            step(create("pending")),
            step(json!({"kind": "confirm", "message": "'Close order ' & record.id & '?'"})),
            step(json!({"kind": "message", "text": "'done'"})),
        ],
    );
    with_actions(&db, vec![a.clone()]);
    let h = Arc::new(super::host::RecordingHost::answering(false));
    let mut ctx = db.ctx(h.clone());
    ctx.record = Some(obj(json!({"id": 3})));
    let result = run_action(&a, ctx);
    assert!(!result.ok && result.cancelled);
    assert_eq!(events(&h), vec![json!(["confirm", "Close order 3?"])]);
    assert!(db.rows("audit").is_empty());
}

#[test]
fn nested_actions_run_inside_the_callers_transaction() {
    let db = orders_db();
    let child = action("child", vec![step(create("child"))]);
    let parent = action_with(
        "parent",
        "rollback",
        vec![step(json!({"kind": "runAction", "actionId": "child"})), step(json!({"kind": "message", "text": "1 +"}))],
    );
    with_actions(&db, vec![child, parent]);
    let result = run_action_by_id("parent", db.ctx(host()));
    assert!(!result.ok);
    let paths: Vec<_> = result.steps.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(paths, ["0", "0.action.0", "1"]);
    assert!(db.rows("audit").is_empty());
}

#[test]
fn stops_recursive_run_action_cycles() {
    let db = orders_db();
    let mut a = action("a", vec![step(json!({"kind": "runAction", "actionId": "b"}))]);
    a.name = "A".into();
    let mut b = action("b", vec![step(json!({"kind": "runAction", "actionId": "a"}))]);
    b.name = "B".into();
    with_actions(&db, vec![a, b]);
    let result = run_action_by_id("a", db.ctx(host()));
    assert!(!result.ok);
    assert!(error_has(&result.error, "Recursive action call: A → B → A"), "{:?}", result.error);
}

#[test]
fn denies_unauthorized_actions_and_writes() {
    let db = orders_db();
    let a = action("a", vec![step(json!({"kind": "deleteRecord", "table": "orders", "match": {"id": "1"}}))]);
    with_actions(&db, vec![a.clone()]);
    let mut deny_all = db.ctx(host());
    deny_all.authorize = Some(Arc::new(|_, _, _| false));
    let result = run_action(&a, deny_all);
    assert_eq!(result.error.as_deref(), Some("Not permitted"));
    let mut deny_delete = db.ctx(host());
    deny_delete.authorize = Some(Arc::new(|_, _, op| op != "delete"));
    let result = run_action(&a, deny_delete);
    assert!(error_has(&result.error, "Not permitted"));
    assert_eq!(db.rows("orders").len(), 2);
}

#[test]
fn returns_an_error_for_unknown_actions() {
    let db = orders_db();
    let result = run_action_by_id("missing", db.ctx(host()));
    assert!(!result.ok);
    assert_eq!(result.error.as_deref(), Some("Action missing does not exist"));
}

#[test]
fn headless_contexts_fail_navigation_and_confirmation_clearly() {
    let db = orders_db();
    let nav = action("nav", vec![step(json!({"kind": "openForm", "formId": "f1"}))]);
    let ask = action("ask", vec![step(json!({"kind": "confirm", "message": "'ok?'"}))]);
    let say = action("say", vec![step(json!({"kind": "message", "text": "'hi ' & record.id"}))]);
    with_actions(&db, vec![nav.clone(), ask.clone(), say.clone()]);
    let env = db.env(host() as Arc<dyn ActionHost>);
    let headless = Arc::new(HeadlessHost::default());
    let payload = json!({"record": {"id": 1}});
    let make = || headless_context(env.as_ref() as &dyn TriggerEnv, &payload, headless.clone(), None);
    assert!(error_has(&run_action(&nav, make()).error, "Navigation is not available in background jobs"));
    assert!(error_has(&run_action(&ask, make()).error, "Confirmation is not available"));
    assert!(run_action(&say, make()).ok);
    let messages = headless.messages();
    assert_eq!(messages.len(), 1);
    assert_eq!((messages[0].text.as_str(), messages[0].tone.as_str()), ("hi 1", "info"));
}
