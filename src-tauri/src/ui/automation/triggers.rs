//! The Triggers tab (port of `TriggersPanel.tsx`): the list of record triggers and the
//! editor of the selected one.
//!
//! Test ids: `new-trigger`, `trigger-item-N`, `no-triggers`; `trigger-name`,
//! `trigger-enabled`, `delete-trigger`, `trigger-table`, `trigger-event`,
//! `trigger-action`, `trigger-condition`, `trigger-run-as`, `trigger-mode`,
//! `trigger-max-attempts`, `trigger-backoff`, `trigger-idempotency`, `trigger-help`.

use super::env::Env;
use super::fields::{expression_problem, number_problem, parse_whole};
use crate::ui::controls::{check, choice_field, expr_field, text_field, Inputs};
use super::picker::action_picker;
use crate::automation::{Trigger, TriggerEvent, TriggerMode};
use crate::ui::{dialogs, display::{status_badge, Tone}, prelude::*, shell::Shell, task};

const EVENTS: [(&str, &str); 4] = [
    ("beforeChange", "Before a record is saved"),
    ("created", "Record created"),
    ("updated", "Record updated"),
    ("deleted", "Record deleted"),
];

fn event_key(event: TriggerEvent) -> &'static str {
    match event {
        TriggerEvent::BeforeChange => "beforeChange",
        TriggerEvent::Created => "created",
        TriggerEvent::Updated => "updated",
        TriggerEvent::Deleted => "deleted",
    }
}

fn event_from(key: &str) -> TriggerEvent {
    match key {
        "beforeChange" => TriggerEvent::BeforeChange,
        "updated" => TriggerEvent::Updated,
        "deleted" => TriggerEvent::Deleted,
        _ => TriggerEvent::Created,
    }
}

const BEFORE_HELP: &str = "Runs before each new or changed record is saved, including rows an action query writes. Its action can set fields with Set field steps, read queries, and reject the save with a Fail step; it cannot write records or open anything. In the condition and action, old is the row before an update and empty for a new record. Run as app skips the user's role checks; run as the signed-in user refuses the save up front when the user's role cannot run the trigger.";

const AFTER_HELP: &str = "Created, updated and deleted triggers see the record as record (a deleted record as it was). Sync triggers run after the record is saved, as part of the same operation; a failing action reports an error to whoever saved the record, but the save stays. Async triggers run in the background while the app is open, with retries. Run as app lets the trigger make its own writes even when the user's role cannot; run as the signed-in user refuses the save up front when the user's role cannot run the trigger.";

pub struct TriggersPanel {
    shell: Entity<Shell>,
    selected: Option<String>,
    inputs: Inputs<TriggersPanel, &'static str>,
    _subscription: Subscription,
}

impl TriggersPanel {
    pub fn new(shell: Entity<Shell>, focus: Option<String>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
        Self {
            shell,
            selected: focus,
            inputs: Inputs::new(Self::on_edit),
            _subscription: subscription,
        }
    }

    pub fn triggers(&self, cx: &App) -> Vec<Trigger> {
        self.shell.read(cx).config(cx).triggers.clone()
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// Selects a trigger (a reveal from elsewhere).
    pub fn focus(&mut self, id: &str, cx: &mut Context<Self>) {
        self.selected = Some(id.to_string());
        cx.notify();
    }

    fn on_edit(this: &mut Self, key: &&'static str, text: String, cx: &mut Context<Self>) {
        match *key {
            "name" => this.edit(move |t| t.name = text, cx),
            "condition" => this.edit(
                move |t| t.condition = (!text.is_empty()).then_some(text),
                cx,
            ),
            "idempotency" => this.edit(
                move |t| t.idempotency_key = (!text.is_empty()).then_some(text),
                cx,
            ),
            "max-attempts" => {
                if let Some(n) = parse_whole(&text, 1) {
                    this.edit(move |t| t.max_attempts = n as u32, cx);
                }
            }
            "backoff" => {
                if let Some(n) = parse_whole(&text, 0) {
                    this.edit(move |t| t.backoff_ms = n as u64, cx);
                }
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

    fn edit(&mut self, mutate: impl FnOnce(&mut Trigger), cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let Some(current) = self.triggers(cx).into_iter().find(|t| t.id == id) else {
            return;
        };
        let mut next = current.clone();
        mutate(&mut next);
        if next == current {
            return;
        }
        self.update_config(
            "Edit trigger",
            move |c| {
                if let Some(t) = c.triggers.iter_mut().find(|t| t.id == id) {
                    *t = next;
                }
            },
            cx,
        );
    }

    fn add(&mut self, cx: &mut Context<Self>) {
        let env = Env::read(self.shell.read(cx), cx);
        let config = self.shell.read(cx).config(cx);
        let created = Trigger {
            id: uuid::Uuid::new_v4().to_string(),
            name: format!("Trigger {}", config.triggers.len() + 1),
            table: env.tables.first().map(|t| t.0.clone()).unwrap_or_default(),
            action_id: config.actions.first().map(|a| a.id.clone()).unwrap_or_default(),
            ..Default::default()
        };
        self.selected = Some(created.id.clone());
        self.update_config("Add trigger", move |c| c.triggers.push(created), cx);
    }

    fn remove(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let triggers = self.triggers(cx);
        let index = triggers.iter().position(|t| t.id == id);
        let name = index.map(|i| triggers[i].name.clone()).unwrap_or_default();
        let name = if name.is_empty() { "Untitled trigger".to_string() } else { name };
        let neighbor = index.and_then(|i| {
            triggers
                .get(i + 1)
                .or_else(|| i.checked_sub(1).and_then(|p| triggers.get(p)))
                .map(|t| t.id.clone())
        });
        let answer = dialogs::confirm(
            cx,
            &format!("Delete trigger {name}?"),
            "Its action will no longer run automatically. This can be undone with Undo.",
        );
        task::after(cx, answer, move |this, ok, cx| {
            if !ok {
                return;
            }
            this.selected = neighbor;
            this.update_config("Delete trigger", move |c| c.triggers.retain(|t| t.id != id), cx);
        });
    }

    fn render_list(&self, triggers: &[Trigger], cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected.clone();
        v_flex()
            .id("trigger-list")
            .w(px(220.))
            .flex_none()
            .gap_1()
            .p_2()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("new-trigger")
                    .outline()
                    .small()
                    .label("New trigger")
                    .on_click(cx.listener(|this, _, _, cx| this.add(cx))),
            )
            .children(triggers.iter().enumerate().map(|(i, t)| {
                let id = t.id.clone();
                let name = if t.name.is_empty() { "Untitled trigger" } else { t.name.as_str() };
                let item = format!("trigger-item-{}", i + 1);
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new(SharedString::from(item.clone()))
                            .ghost()
                            .small()
                            .flex_1()
                            .min_w_0()
                            .start_label(None, name.to_string())
                            .selected(selected.as_deref() == Some(id.as_str()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected = Some(id.clone());
                                cx.notify();
                            })),
                    )
                    .when(!t.enabled, |el| {
                        el.child(status_badge(format!("{item}-off"), "Off", Tone::Neutral, cx))
                    })
            }))
            .when(triggers.is_empty(), |el| {
                el.child(widgets::empty_state("no-triggers", "No triggers yet", "A trigger runs an action when a record changes.", cx))
            })
    }

    fn render_editor(
        &mut self,
        trigger: &Trigger,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let env = Env::read(self.shell.read(cx), cx);
        let actions = self.shell.read(cx).config(cx).actions.clone();
        let entity = cx.entity();
        let before = trigger.event == TriggerEvent::BeforeChange;
        let is_async = trigger.mode == TriggerMode::Async;
        let name = self.inputs.get("name", &trigger.name, "", window, cx);
        let condition_text = trigger.condition.clone().unwrap_or_default();
        let condition = self.inputs.get("condition", &condition_text, "always", window, cx);
        let idempotency_text = trigger.idempotency_key.clone().unwrap_or_default();
        let idempotency = self.inputs.get(
            "idempotency",
            &idempotency_text,
            "trigger id : table : key : event : hash of values",
            window,
            cx,
        );
        let attempts_text = trigger.max_attempts.to_string();
        let attempts = self.inputs.get("max-attempts", &attempts_text, "", window, cx);
        let backoff_text = trigger.backoff_ms.to_string();
        let backoff = self.inputs.get("backoff", &backoff_text, "", window, cx);
        let danger = cx.theme().danger;
        let number = |id: &str, label: &str, state: &Entity<_>, text: String, min: i64| {
            let problem = number_problem(&text, min);
            text_field(id.to_string(), label, state, cx).when_some(problem, |el, p| {
                el.child(widgets::styled_text(
                    format!("{id}-problem"),
                    p,
                    move |d| d.text_xs().text_color(danger),
                ))
            })
        };
        // Numbers show what is typed, not the stored value, so read the live text.
        let attempts_live = self.inputs.text(&"max-attempts", cx).unwrap_or(attempts_text);
        let backoff_live = self.inputs.text(&"backoff", cx).unwrap_or(backoff_text);
        let pick = |id: &'static str, label: &str, current: &str, options: &[(&str, &str)], apply: fn(&mut Trigger, &str)| {
            let entity = entity.clone();
            choice_field(
                id,
                label,
                current,
                options.iter().map(|(v, l)| (v.to_string(), l.to_string())).collect(),
                label,
                move |value, _, cx| {
                    let value = value.to_string();
                    entity.update(cx, |this, cx| this.edit(move |t| apply(t, &value), cx));
                },
                cx,
            )
        };
        let tables: Vec<(String, String)> = env.tables.clone();
        let table_field = {
            let entity = entity.clone();
            choice_field(
                "trigger-table",
                "Table",
                &trigger.table,
                tables,
                "Table",
                move |value, _, cx| {
                    let value = value.to_string();
                    entity.update(cx, |this, cx| this.edit(move |t| t.table = value, cx));
                },
                cx,
            )
        };
        let enabled = {
            let entity = entity.clone();
            check("trigger-enabled", "Enabled", trigger.enabled, move |on, _, cx| {
                entity.update(cx, |this, cx| this.edit(move |t| t.enabled = on, cx));
            })
        };
        let action_field = {
            let entity = entity.clone();
            action_picker(
                "trigger-action",
                "Action",
                Some(&trigger.action_id),
                None,
                &actions,
                move |id, _, cx| {
                    entity.update(cx, |this, cx| {
                        this.edit(move |t| t.action_id = id.unwrap_or_default(), cx)
                    });
                },
                cx,
            )
        };
        let run_as = pick(
            "trigger-run-as",
            "Run as",
            trigger.run_as.as_deref().unwrap_or("app"),
            &[("app", "App - the trigger's own steps"), ("user", "Signed-in user's role")],
            |t, v| t.run_as = Some(v.to_string()),
        );
        let event = pick(
            "trigger-event",
            "Event",
            event_key(trigger.event),
            &EVENTS,
            |t, v| {
                t.event = event_from(v);
                if t.event == TriggerEvent::BeforeChange {
                    t.mode = TriggerMode::Sync;
                }
            },
        );
        let mode = pick(
            "trigger-mode",
            "Run",
            if is_async { "async" } else { "sync" },
            &[
                ("sync", "Synchronously, inside the write"),
                ("async", "Asynchronously, on the job queue"),
            ],
            |t, v| t.mode = if v == "async" { TriggerMode::Async } else { TriggerMode::Sync },
        );
        let mut run_row = h_flex().gap_3().items_start().flex_wrap();
        if !before {
            run_row = run_row.child(mode);
            if is_async {
                run_row = run_row
                    .child(number("trigger-max-attempts", "Max attempts", &attempts, attempts_live, 1))
                    .child(number("trigger-backoff", "Retry backoff (ms)", &backoff, backoff_live, 0));
            }
        }
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_3()
                    .items_end()
                    .flex_wrap()
                    .child(text_field("trigger-name", "Trigger name", &name, cx))
                    .child(enabled)
                    .child(
                        Button::new("delete-trigger")
                            .danger()
                            .outline()
                            .small()
                            .label("Delete trigger")
                            .on_click(cx.listener(|this, _, _, cx| this.remove(cx))),
                    ),
            )
            .child(h_flex().gap_3().items_start().flex_wrap().child(table_field).child(event).child(action_field))
            .child(expr_field(
                "trigger-condition",
                "Condition",
                &condition,
                expression_problem(&condition_text, false),
                cx,
            ))
            .child(run_as)
            .child(run_row)
            .when(!before && is_async, |el| {
                el.child(expr_field(
                    "trigger-idempotency",
                    "Idempotency key",
                    &idempotency,
                    expression_problem(&idempotency_text, false),
                    cx,
                ))
            })
            .child(widgets::styled_text(
                "trigger-help",
                if before { BEFORE_HELP } else { AFTER_HELP },
                |d| d.text_sm(),
            ))
    }
}

impl Render for TriggersPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let triggers = self.triggers(cx);
        if !self.selected.as_ref().is_some_and(|s| triggers.iter().any(|t| t.id == *s)) {
            self.selected = triggers.first().map(|t| t.id.clone());
        }
        let trigger = self
            .selected
            .as_ref()
            .and_then(|id| triggers.iter().find(|t| t.id == *id))
            .cloned();
        let editor = match trigger {
            Some(t) => self.render_editor(&t, window, cx).into_any_element(),
            None => widgets::text("select-trigger", "Select or create a trigger.").into_any_element(),
        };
        h_flex()
            .size_full()
            .items_start()
            .child(self.render_list(&triggers, cx))
            .child(
                div()
                    .id("trigger-editor")
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
    use super::{event_from, event_key, EVENTS};

    #[test]
    fn events_round_trip_through_their_keys() {
        for (key, _) in EVENTS {
            assert_eq!(event_key(event_from(key)), key);
        }
    }
}
