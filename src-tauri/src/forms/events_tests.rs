//! Port of tests/unit/form-events.test.ts. The action runner is a test double that runs
//! the two step kinds the test action uses (`fail` and `message`); the real runner belongs
//! to Automation and plugs in through `actions::set_action_runner`.

use super::{
    EventState, FormEvent, after_update_problem, event_action, events_to_raise, run_form_event,
    veto_message,
};
use crate::archive::DocumentConfig;
use crate::design::{Form, FormEvents};
use crate::expr::{self, EvaluateOptions, Scope};
use crate::forms::actions::{
    ActionContext, ActionResult, ActionRunner, NoticeTone, RecordingHost, run_action,
    set_action_runner,
};
use serde_json::{Map, Value, json};
use std::sync::{Arc, Mutex};

/// Runs `fail` (aborts with the evaluated message when `when` holds) and `message` steps.
struct GuardRunner;

impl ActionRunner for GuardRunner {
    fn run_action(&self, action_id: &str, ctx: &ActionContext) -> ActionResult {
        if !ctx.host.authorize("action", action_id, "execute") {
            return ActionResult::failure("Not permitted");
        }
        let Some(action) = ctx.config.actions.iter().find(|a| a.id == action_id) else {
            return ActionResult::failure(format!("Action {action_id} not found"));
        };
        let mut scope = Scope::new();
        scope.insert("record".into(), Value::Object(ctx.record.clone().unwrap_or_default()));
        for step in &action.steps {
            let text = |key: &str| {
                step.fields
                    .get(key)
                    .and_then(Value::as_str)
                    .map(|src| expr::evaluate(src, &scope, EvaluateOptions::default()))
            };
            match step.kind.as_str() {
                "fail" => {
                    let holds = step.when.as_deref().is_none_or(|w| {
                        expr::evaluate_boolean(w, &scope, EvaluateOptions::default()).unwrap_or(false)
                    });
                    if holds {
                        let message = text("message").and_then(Result::ok).unwrap_or(Value::Null);
                        return ActionResult {
                            ok: false,
                            error: message.as_str().map(str::to_string),
                            aborted: true,
                            ..Default::default()
                        };
                    }
                }
                "message" => {
                    if let Some(Ok(Value::String(note))) = text("text") {
                        ctx.host.notify(&note, NoticeTone::Info);
                    }
                }
                _ => {}
            }
        }
        ActionResult::success()
    }
}

/// The action runner is process-wide; tests that install one take this lock.
fn runner_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn form(events: FormEvents) -> Form {
    Form {
        id: "f".into(),
        name: "Orders".into(),
        events,
        ..Default::default()
    }
}

fn bound(event: FormEvent, id: &str) -> Form {
    let mut events = FormEvents::default();
    match event {
        FormEvent::OnLoad => events.on_load = Some(id.into()),
        FormEvent::OnCurrent => events.on_current = Some(id.into()),
        FormEvent::BeforeUpdate => events.before_update = Some(id.into()),
        FormEvent::AfterUpdate => events.after_update = Some(id.into()),
    }
    form(events)
}

fn config() -> Arc<DocumentConfig> {
    let actions = serde_json::from_value(json!([{
        "id": "guard", "name": "Guard", "onError": "stop",
        "steps": [
            {"id": "s1", "kind": "fail", "when": "record.status = 'bad'", "message": "'No bad orders'"},
            {"id": "s2", "kind": "message", "text": "'ok ' & record.status"},
        ]
    }]))
    .unwrap();
    Arc::new(DocumentConfig {
        actions,
        ..Default::default()
    })
}

fn ctx(record: Value, host: Arc<RecordingHost>) -> ActionContext {
    ActionContext {
        config: config(),
        window_label: "test".into(),
        record: record.as_object().cloned(),
        form: None,
        app: Map::new(),
        params: None,
        old: None,
        snapshot: None,
        host,
    }
}

#[test]
fn raises_on_load_once_per_view_and_on_current_per_record() {
    let mut state = EventState::default();
    assert_eq!(
        events_to_raise(&mut state, "a"),
        [FormEvent::OnLoad, FormEvent::OnCurrent]
    );
    assert_eq!(events_to_raise(&mut state, "a"), []);
    assert_eq!(events_to_raise(&mut state, "b"), [FormEvent::OnCurrent]);
}

#[test]
fn runs_nothing_for_an_unbound_event() {
    let _lock = runner_lock();
    set_action_runner(Some(Arc::new(GuardRunner)));
    assert_eq!(event_action(&form(FormEvents::default()), FormEvent::OnLoad), None);
    assert_eq!(event_action(&bound(FormEvent::OnLoad, ""), FormEvent::OnLoad), None);
    let host = Arc::new(RecordingHost::new(true));
    assert!(
        run_form_event(&form(FormEvents::default()), FormEvent::BeforeUpdate, &ctx(json!({}), host))
            .is_none()
    );
    set_action_runner(None);
}

#[test]
fn vetoes_a_save_when_the_before_update_action_fails() {
    let _lock = runner_lock();
    set_action_runner(Some(Arc::new(GuardRunner)));
    let bound = bound(FormEvent::BeforeUpdate, "guard");
    let host = Arc::new(RecordingHost::new(true));
    let bad = run_form_event(&bound, FormEvent::BeforeUpdate, &ctx(json!({"status": "bad"}), host));
    assert_eq!(veto_message(bad.as_ref()).as_deref(), Some("No bad orders"));
    let host = Arc::new(RecordingHost::new(true));
    let ok = run_form_event(
        &bound,
        FormEvent::BeforeUpdate,
        &ctx(json!({"status": "paid"}), host.clone()),
    );
    assert_eq!(veto_message(ok.as_ref()), None);
    assert_eq!(host.notes(), ["ok paid"]);
    set_action_runner(None);
}

#[test]
fn vetoes_when_the_role_cannot_run_the_action_and_on_a_declined_confirm() {
    let _lock = runner_lock();
    set_action_runner(Some(Arc::new(GuardRunner)));
    let bound = bound(FormEvent::BeforeUpdate, "guard");
    let host = Arc::new(RecordingHost::new(true));
    host.denied.store(true, std::sync::atomic::Ordering::SeqCst);
    let denied = run_form_event(&bound, FormEvent::BeforeUpdate, &ctx(json!({"status": "paid"}), host));
    assert_eq!(veto_message(denied.as_ref()).as_deref(), Some("Not permitted"));
    let cancelled = ActionResult {
        cancelled: true,
        ..Default::default()
    };
    assert_eq!(veto_message(Some(&cancelled)).as_deref(), Some("Save cancelled."));
    assert_eq!(veto_message(None), None);
    set_action_runner(None);
}

#[test]
fn without_a_runner_an_action_fails_instead_of_panicking() {
    let _lock = runner_lock();
    set_action_runner(None);
    let host = Arc::new(RecordingHost::new(true));
    let result = run_action("guard", &ctx(json!({}), host));
    assert!(!result.ok);
    assert!(result.error.unwrap().contains("not available"));
}

#[test]
fn reports_a_failed_after_update_action_without_undoing_the_save() {
    let failed = ActionResult::failure("boom");
    assert_eq!(
        after_update_problem(Some(&failed)).as_deref(),
        Some("Saved, but the after update action failed: boom")
    );
    assert_eq!(after_update_problem(Some(&ActionResult::success())), None);
    let cancelled = ActionResult {
        cancelled: true,
        ..Default::default()
    };
    assert_eq!(after_update_problem(Some(&cancelled)), None);
}

#[test]
fn keeps_bound_events_through_a_design_upgrade_and_drops_empty_ones() {
    let design: crate::design::DesignSchema = serde_json::from_value(json!({
        "version": 3,
        "forms": [
            {"id": "a", "name": "A", "events": {"onLoad": "x", "onCurrent": null}},
            {"id": "b", "name": "B"},
        ]
    }))
    .unwrap();
    let events = &design.forms[0].events;
    assert_eq!(events.on_load.as_deref(), Some("x"));
    assert_eq!(events.on_current, None);
    assert!(design.forms[1].events.is_empty());
    // An empty event serializes away, so an unbound form stores no `events` key.
    assert!(serde_json::to_value(&design.forms[1]).unwrap().get("events").is_none());
}
