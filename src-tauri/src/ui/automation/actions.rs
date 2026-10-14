//! The Actions tab (port of `ActionsPanel.tsx`): the list of actions, the editor of the
//! selected one (name, description, on-error mode, steps) and its test run.
//!
//! Test ids: `new-action`, `action-item-N`, `no-actions`; `action-name`,
//! `action-description`, `action-on-error`, `delete-action`; the steps are the
//! `StepEditor`'s. Test run: `test-sample`, `test-problem`, `test-run`, `test-result`,
//! `test-event-N` and the step log `test-log` (see `step_log`).

use crate::ui::controls::{choice_field, text_field, Inputs};
use super::step_editor::StepEditor;
use super::steps::{new_action, step_number};
use crate::automation::{ActionDef, OnError};
use crate::automation_runner::{
    context::{ActionContext, ActionHost, NavigationTarget, Tone},
    run_action,
    runner::step_label,
    ActionResult, StepLog,
};
use crate::ui::{
    dialogs::{DialogProvider, Dialogs},
    prelude::*,
    shell::Shell,
    task,
};
use serde_json::{Map, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

const ON_ERROR: [(&str, &str); 3] = [
    ("stop", "Stop at the first failure"),
    ("continue", "Log failures and continue"),
    ("rollback", "Roll back all record changes"),
];

fn on_error_key(mode: OnError) -> &'static str {
    match mode {
        OnError::Stop => "stop",
        OnError::Continue => "continue",
        OnError::Rollback => "rollback",
    }
}

fn on_error_from(key: &str) -> OnError {
    match key {
        "continue" => OnError::Continue,
        "rollback" => OnError::Rollback,
        _ => OnError::Stop,
    }
}

/// The sample record of a test run: a JSON object.
pub fn parse_sample(text: &str) -> Result<Map<String, Value>, String> {
    let text = if text.trim().is_empty() { "{}" } else { text };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err("The sample record must be a JSON object".to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// The host of a test run: navigation and state changes are logged instead of followed,
/// confirmations ask the user.
struct TestRunHost {
    log: Mutex<Vec<String>>,
    refreshed: AtomicBool,
    dialogs: Option<Arc<dyn DialogProvider>>,
}

impl TestRunHost {
    fn push(&self, line: String) {
        self.log.lock().unwrap_or_else(|e| e.into_inner()).push(line);
    }
}

impl ActionHost for TestRunHost {
    fn navigate(&self, target: &NavigationTarget) -> Result<(), String> {
        let mode = target.mode.as_deref().map(|m| format!(" ({m})")).unwrap_or_default();
        self.push(format!("Navigate to {} {}{mode}", target.kind, target.id));
        Ok(())
    }
    fn set_state(&self, scope: &str, key: &str, value: &Value) -> Result<(), String> {
        self.push(format!("Set {scope}.{key} = {value}"));
        Ok(())
    }
    fn confirm(&self, message: &str) -> Result<bool, String> {
        Ok(self
            .dialogs
            .as_ref()
            .is_some_and(|d| d.confirm("Confirm", message)))
    }
    fn notify(&self, message: &str, tone: Tone) {
        let kind = if tone == Tone::Error { "Error" } else { "Message" };
        self.push(format!("{kind}: {message}"));
    }
    fn refresh(&self) {
        self.refreshed.store(true, Ordering::SeqCst);
    }
}

/// What a finished test run shows.
pub struct TestOutcome {
    pub result: ActionResult,
    pub events: Vec<String>,
}

pub struct ActionsPanel {
    shell: Entity<Shell>,
    selected: Option<String>,
    editor: Option<Entity<StepEditor>>,
    inputs: Inputs<ActionsPanel, &'static str>,
    sample: String,
    problem: Option<String>,
    running: bool,
    outcome: Option<TestOutcome>,
    _subscription: Subscription,
}

impl ActionsPanel {
    pub fn new(shell: Entity<Shell>, focus: Option<String>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
        Self {
            shell,
            selected: focus,
            editor: None,
            inputs: Inputs::new(Self::on_edit),
            sample: "{}".to_string(),
            problem: None,
            running: false,
            outcome: None,
            _subscription: subscription,
        }
    }

    pub fn actions(&self, cx: &App) -> Vec<ActionDef> {
        self.shell.read(cx).config(cx).actions.clone()
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// The finished test run, if any.
    pub fn outcome(&self) -> Option<&TestOutcome> {
        self.outcome.as_ref()
    }

    /// Selects an action (a reveal from elsewhere).
    pub fn focus(&mut self, id: &str, cx: &mut Context<Self>) {
        self.select(id, cx);
    }

    fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.selected.as_deref() != Some(id) {
            self.selected = Some(id.to_string());
            self.outcome = None;
            self.problem = None;
        }
        cx.notify();
    }

    fn on_edit(this: &mut Self, key: &&'static str, text: String, cx: &mut Context<Self>) {
        match *key {
            "name" => this.edit("Edit action", move |a| a.name = text, cx),
            "description" => this.edit("Edit action", move |a| a.description = text, cx),
            "sample" => {
                this.sample = text;
                cx.notify();
            }
            _ => {}
        }
    }

    fn update_config(
        &mut self,
        label: &str,
        mutate: impl FnOnce(&mut crate::archive::DocumentConfig),
        cx: &mut Context<Self>,
    ) {
        Shell::edit_config(&self.shell, label, mutate, cx);
        cx.notify();
    }

    fn edit(&mut self, label: &str, mutate: impl FnOnce(&mut ActionDef), cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let current = self.actions(cx).into_iter().find(|a| a.id == id);
        let Some(current) = current else {
            return;
        };
        let mut next = current.clone();
        mutate(&mut next);
        if next == current {
            return;
        }
        self.update_config(
            label,
            move |c| {
                if let Some(a) = c.actions.iter_mut().find(|a| a.id == id) {
                    *a = next;
                }
            },
            cx,
        );
    }

    fn add(&mut self, cx: &mut Context<Self>) {
        let created = new_action(self.actions(cx).len() + 1);
        self.select(&created.id.clone(), cx);
        self.update_config("Add action", move |c| c.actions.push(created), cx);
    }

    fn remove(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let actions = self.actions(cx);
        let index = actions.iter().position(|a| a.id == id);
        let neighbor = index.and_then(|i| {
            actions
                .get(i + 1)
                .or_else(|| i.checked_sub(1).and_then(|p| actions.get(p)))
                .map(|a| a.id.clone())
        });
        self.selected = neighbor;
        self.outcome = None;
        self.update_config("Delete action", move |c| c.actions.retain(|a| a.id != id), cx);
    }

    /// Runs the selected action against the document's data with the sample record.
    fn run_test(&mut self, cx: &mut Context<Self>) {
        let Some(action) = self
            .selected
            .as_ref()
            .and_then(|id| self.actions(cx).into_iter().find(|a| a.id == *id))
        else {
            return;
        };
        let record = match parse_sample(&self.sample) {
            Ok(r) => r,
            Err(problem) => {
                self.problem = Some(problem);
                cx.notify();
                return;
            }
        };
        self.problem = None;
        self.running = true;
        let host = Arc::new(TestRunHost {
            log: Mutex::new(vec![]),
            refreshed: AtomicBool::new(false),
            dialogs: cx.try_global::<Dialogs>().map(|d| d.0.clone()),
        });
        let shell = self.shell.read(cx);
        let (label, config) = (
            shell.label().to_string(),
            Arc::new(shell.config(cx).clone()),
        );
        let observer = host.clone();
        task::spawn_call(
            cx,
            move || {
                let mut ctx = ActionContext::new(&label, config, host);
                ctx.record = Some(record);
                run_action(&action, ctx)
            },
            move |this, result, cx| {
                this.running = false;
                let events = observer.log.lock().unwrap_or_else(|e| e.into_inner()).clone();
                this.outcome = Some(TestOutcome { result, events });
                if observer.refreshed.load(Ordering::SeqCst) {
                    this.shell.update(cx, |s, cx| s.records_changed(cx));
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    fn render_list(&self, actions: &[ActionDef], cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected.clone();
        v_flex()
            .id("action-list")
            .w(px(220.))
            .flex_none()
            .gap_1()
            .p_2()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("new-action")
                    .outline()
                    .small()
                    .label("New action")
                    .on_click(cx.listener(|this, _, _, cx| this.add(cx))),
            )
            .children(actions.iter().enumerate().map(|(i, a)| {
                let id = a.id.clone();
                let label = if a.name.is_empty() { "Untitled action" } else { a.name.as_str() };
                Button::new(SharedString::from(format!("action-item-{}", i + 1)))
                    .ghost()
                    .small()
                    .w_full()
                    .start_label(None, label.to_string())
                    .selected(selected.as_deref() == Some(id.as_str()))
                    .on_click(cx.listener(move |this, _, _, cx| this.select(&id, cx)))
            }))
            .when(actions.is_empty(), |el| {
                el.child(widgets::text("no-actions", "No actions yet."))
            })
    }

    fn render_editor(
        &mut self,
        action: &ActionDef,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if self.editor.as_ref().is_none_or(|e| e.read(cx).action_id != action.id) {
            let shell = self.shell.clone();
            let id = action.id.clone();
            self.editor = Some(cx.new(|_| StepEditor::new(shell, &id)));
        }
        let name = self.inputs.get("name", &action.name, "", window, cx);
        let description = self.inputs.get("description", &action.description, "", window, cx);
        let entity = cx.entity();
        let on_error = choice_field(
            "action-on-error",
            "On error",
            on_error_key(action.on_error),
            ON_ERROR.iter().map(|(v, l)| (v.to_string(), l.to_string())).collect(),
            "On error",
            move |value, _, cx| {
                let mode = on_error_from(value);
                entity.update(cx, |this, cx| {
                    this.edit("Edit action", move |a| a.on_error = mode, cx)
                });
            },
            cx,
        );
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_3()
                    .items_end()
                    .flex_wrap()
                    .child(text_field("action-name", "Action name", &name, cx))
                    .child(text_field("action-description", "Description", &description, cx))
                    .child(on_error)
                    .child(
                        Button::new("delete-action")
                            .danger()
                            .outline()
                            .small()
                            .label("Delete action")
                            .on_click(cx.listener(|this, _, _, cx| this.remove(cx))),
                    ),
            )
            .children(self.editor.clone())
            .child(self.render_test_run(window, cx))
    }

    fn render_test_run(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sample = self.sample.clone();
        let state = self.inputs.get("sample", &sample, "{}", window, cx);
        let theme = cx.theme();
        let (ok, fail) = (theme.success, theme.danger);
        let outcome = self.outcome.as_ref().map(|o| {
            let (text, color) = if o.result.ok {
                ("Test run succeeded".to_string(), ok)
            } else if o.result.cancelled {
                ("Test run cancelled".to_string(), fail)
            } else {
                (
                    format!("Test run failed: {}", o.result.error.clone().unwrap_or_default()),
                    fail,
                )
            };
            v_flex()
                .gap_1()
                .child(widgets::styled_text("test-result", text, move |d| d.text_color(color)))
                .children(
                    o.events
                        .iter()
                        .enumerate()
                        .map(|(i, e)| widgets::text(format!("test-event-{}", i + 1), e.clone())),
                )
                .child(step_log("test-log", &o.result.steps, cx))
        });
        v_flex()
            .id("test-run-section")
            .gap_2()
            .p_3()
            .border_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .child(widgets::section_heading("test-run-heading", "Test run"))
            .child(div().text_sm().child("Runs the action against the document's data. Record changes are real."))
            .child(text_field("test-sample", "Sample record (JSON)", &state, cx))
            .when_some(self.problem.clone(), |el, p| {
                el.child(widgets::styled_text("test-problem", p, move |d| d.text_xs().text_color(fail)))
            })
            .child(
                div().child(
                    Button::new("test-run")
                        .outline()
                        .small()
                        .label("Test run")
                        .disabled(self.running)
                        .on_click(cx.listener(|this, _, _, cx| this.run_test(cx))),
                ),
            )
            .children(outcome)
    }
}

/// The log of a run: a header row and one row per step. Rows are `<id>-N` with cells
/// `<id>-N-step`, `-kind`, `-result` and `-time`.
pub fn step_log(id: &str, steps: &[StepLog], cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (ok, fail, muted) = (theme.success, theme.danger, theme.muted_foreground);
    let cell = |w: f32| div().w(px(w)).text_sm();
    v_flex()
        .id(SharedString::from(id.to_string()))
        .gap_0p5()
        .when(!steps.is_empty(), |el| {
            el.child(
                h_flex()
                    .gap_2()
                    .text_color(muted)
                    .child(cell(60.).child("Step"))
                    .child(cell(180.).child("Kind"))
                    .child(cell(320.).child("Result"))
                    .child(cell(70.).child("Time")),
            )
        })
        .children(steps.iter().enumerate().map(|(i, s)| {
            let row = format!("{id}-{}", i + 1);
            let number = if s.path.is_empty() { (s.step_index + 1).to_string() } else { step_number(&s.path) };
            let (result, color) = match (&s.error, s.skipped, s.ok) {
                (_, true, _) => ("Skipped".to_string(), muted),
                (_, _, true) => ("OK".to_string(), ok),
                (e, _, _) => (format!("Failed: {}", e.clone().unwrap_or_default()), fail),
            };
            h_flex()
                .gap_2()
                .child(cell(60.).child(widgets::text(format!("{row}-step"), number)))
                .child(cell(180.).child(widgets::text(format!("{row}-kind"), step_label(&s.kind).to_string())))
                .child(cell(320.).child(widgets::styled_text(
                    format!("{row}-result"),
                    result,
                    move |d| d.text_color(color),
                )))
                .child(cell(70.).child(widgets::text(format!("{row}-time"), format!("{} ms", s.duration_ms))))
        }))
}

impl Render for ActionsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let actions = self.actions(cx);
        // The selection follows the list: a missing one falls back to the first action.
        if !self.selected.as_ref().is_some_and(|s| actions.iter().any(|a| a.id == *s)) {
            self.selected = actions.first().map(|a| a.id.clone());
        }
        let action = self
            .selected
            .as_ref()
            .and_then(|id| actions.iter().find(|a| a.id == *id))
            .cloned();
        let editor = match action {
            Some(action) => self.render_editor(&action, window, cx).into_any_element(),
            None => widgets::text("select-action", "Select or create an action.").into_any_element(),
        };
        h_flex()
            .size_full()
            .items_start()
            .child(self.render_list(&actions, cx))
            .child(
                div()
                    .id("action-editor")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .p_4()
                    .child(editor),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{on_error_from, on_error_key, parse_sample};
    use crate::automation::OnError;

    #[test]
    fn sample_records_must_be_json_objects() {
        assert!(parse_sample("").unwrap().is_empty());
        assert_eq!(parse_sample(r#"{"a": 1}"#).unwrap()["a"], 1);
        assert_eq!(
            parse_sample("[1]").unwrap_err(),
            "The sample record must be a JSON object"
        );
        assert!(parse_sample("{oops").is_err());
    }

    #[test]
    fn on_error_modes_round_trip_through_their_keys() {
        for mode in [OnError::Stop, OnError::Continue, OnError::Rollback] {
            assert_eq!(on_error_from(on_error_key(mode)), mode);
        }
        assert_eq!(on_error_from("unknown"), OnError::Stop);
    }
}
