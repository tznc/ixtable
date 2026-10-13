//! The step editor of an action (port of `StepEditor.tsx`): an ordered list of steps with
//! add, move and remove, and the fields of each kind. Condition steps nest two lists.
//!
//! The editor has no copy of the steps. It renders the action in the document config and
//! writes every edit back through the config store, so undo and redo just work.
//!
//! Test ids (positions are 1-based, nested lists extend the prefix):
//! - `step-N` is the step, `step-N-title` its heading ("Step 1: Update record"),
//!   `step-N-up`, `-down`, `-remove` its tools.
//! - Field controls are `step-N-<field>` (`table`, `store-as`, `when`, `value`, ...). A
//!   value map `values` has `step-N-values-add` and, per row M, `step-N-values-M-key`,
//!   `-expr` and `-remove`.
//! - Lists: `steps-kind` and `steps-add` for the action's steps; `step-N-then-add`,
//!   `step-N-else-add` (and `-kind`) for the branches, whose steps are `step-N-then-M`.

use super::env::{Env, FORM_MODES};
use super::fields::{
    check, choice, choice_field, expr_field, expression_problem, plain, text_field, Inputs,
};
use super::steps::{
    add_step, find_mut, from_values, map_add, map_expr, map_remove, map_rename, map_rows,
    move_step, nested_text, remove_step, set_field, set_nested, text_of, to_values, Branch,
    ListPath, STEP_KINDS,
};
use crate::automation_runner::runner::step_label;
use crate::ui::{prelude::*, shell::Shell};
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct StepEditor {
    shell: Entity<Shell>,
    pub action_id: String,
    inputs: Inputs<StepEditor, String>,
    // The kind chosen for the next step added to each list.
    kinds: HashMap<ListPath, String>,
}

/// The field a text edit of an input with `field` sets, and whether an empty text clears it.
fn clears_when_empty(kind: &str, field: &str) -> bool {
    match field {
        "recordId" | "mode" | "target.mode" | "target.recordId" => true,
        "when" => kind != "condition",
        "storeAs" => kind != "runQuery",
        "value" => kind == "closeForm",
        _ => false,
    }
}

/// A step's value for an editor field (`target.id` reads inside the target).
fn value_of(step: &Value, field: &str) -> String {
    match field.split_once('.') {
        Some((object, key)) => nested_text(step, object, key),
        None => text_of(step, field),
    }
}

impl StepEditor {
    pub fn new(shell: Entity<Shell>, action_id: &str) -> Self {
        Self {
            shell,
            action_id: action_id.to_string(),
            inputs: Inputs::new(Self::on_edit),
            kinds: HashMap::new(),
        }
    }

    /// The action's steps right now.
    pub fn steps(&self, cx: &App) -> Vec<Value> {
        self.shell
            .read(cx)
            .config(cx)
            .actions
            .iter()
            .find(|a| a.id == self.action_id)
            .map(|a| to_values(&a.steps))
            .unwrap_or_default()
    }

    /// Applies `edit` to the action's steps and stores the result when it changed them.
    fn edit(
        &mut self,
        edit: impl FnOnce(&mut Vec<Value>) -> bool,
        cx: &mut Context<Self>,
    ) {
        let mut values = self.steps(cx);
        if !edit(&mut values) {
            return;
        }
        let steps = from_values(&values);
        let id = self.action_id.clone();
        let store = self.shell.read(cx).store.clone();
        let result = store.update(cx, |s, cx| {
            s.update(
                "Edit action steps",
                |c| {
                    if let Some(a) = c.actions.iter_mut().find(|a| a.id == id) {
                        a.steps = steps;
                    }
                },
                cx,
            )
        });
        if let Err(e) = result {
            self.shell.update(cx, |s, cx| s.set_error(Some(e), cx));
        }
        cx.notify();
    }

    /// An input edited: `step/field` or `step/map/row/k|v`.
    fn on_edit(this: &mut Self, key: &String, text: String, cx: &mut Context<Self>) {
        let parts: Vec<&str> = key.split('/').collect();
        match parts.as_slice() {
            [id, field] => this.set_text(id, field, text, cx),
            [id, map, row, side] => {
                let (id, map) = (id.to_string(), map.to_string());
                let row: usize = row.parse().unwrap_or(usize::MAX);
                if *side == "k" {
                    this.edit(move |root| map_rename(root, &id, &map, row, &text), cx);
                } else {
                    this.edit(move |root| map_expr(root, &id, &map, row, &text), cx);
                }
            }
            _ => {}
        }
    }

    fn set_text(&mut self, id: &str, field: &str, text: String, cx: &mut Context<Self>) {
        let (id, field) = (id.to_string(), field.to_string());
        self.edit(
            move |root| {
                let kind = find_mut(root, &id)
                    .and_then(|s| s.get("kind").and_then(Value::as_str).map(str::to_string))
                    .unwrap_or_default();
                let value = if text.is_empty() && clears_when_empty(&kind, &field) {
                    None
                } else {
                    Some(Value::String(text))
                };
                match field.split_once('.') {
                    Some((object, key)) => set_nested(root, &id, object, key, value),
                    None => set_field(root, &id, &field, value),
                }
            },
            cx,
        );
    }

    fn set_value(&mut self, id: &str, field: &str, value: Option<Value>, cx: &mut Context<Self>) {
        let (id, field) = (id.to_string(), field.to_string());
        self.edit(move |root| set_field(root, &id, &field, value), cx);
    }

    fn picker(
        me: &Entity<Self>,
        id: &str,
        field: &str,
    ) -> impl Fn(&str, &mut Window, &mut App) + 'static {
        let (me, id, field) = (me.clone(), id.to_string(), field.to_string());
        move |value, _, cx| {
            let value = value.to_string();
            me.update(cx, |this, cx| this.set_text(&id, &field, value, cx));
        }
    }

    // ---- Controls ------------------------------------------------------------

    fn text(
        &mut self,
        item: &str,
        step: &Value,
        field: &str,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let id = text_of(step, "id");
        let state = self.inputs.get(
            format!("{id}/{field}"),
            &value_of(step, field),
            "",
            window,
            cx,
        );
        let suffix = kebab(field.trim_start_matches("target."));
        text_field(format!("{item}-{suffix}"), label, &state, cx)
    }

    #[allow(clippy::too_many_arguments)]
    fn expr(
        &mut self,
        item: &str,
        step: &Value,
        field: &str,
        label: &str,
        required: bool,
        placeholder: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let id = text_of(step, "id");
        let value = value_of(step, field);
        let state = self
            .inputs
            .get(format!("{id}/{field}"), &value, placeholder, window, cx);
        let suffix = kebab(field.trim_start_matches("target."));
        expr_field(
            &format!("{item}-{suffix}"),
            label,
            &state,
            expression_problem(&value, required),
            cx,
        )
    }

    fn when(&mut self, item: &str, step: &Value, window: &mut Window, cx: &mut Context<Self>) -> Div {
        self.expr(item, step, "when", "Only when", false, "always", window, cx)
    }

    #[allow(clippy::too_many_arguments)]
    fn pick(
        &self,
        me: &Entity<Self>,
        item: &str,
        step: &Value,
        field: &str,
        label: &str,
        options: Vec<(String, String)>,
        placeholder: &str,
        cx: &App,
    ) -> Div {
        let id = text_of(step, "id");
        let suffix = kebab(field.trim_start_matches("target."));
        choice_field(
            format!("{item}-{suffix}"),
            label,
            &value_of(step, field),
            options,
            placeholder,
            Self::picker(me, &id, field),
            cx,
        )
    }

    fn value_map(
        &mut self,
        item: &str,
        step: &Value,
        field: &str,
        (label, key_label): (&str, &str),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let id = text_of(step, "id");
        let me = cx.entity();
        let rows = map_rows(step, field);
        let mut body = v_flex().gap_2().p_2().border_1().border_color(cx.theme().border).rounded_md();
        body = body.child(div().text_sm().font_weight(FontWeight::MEDIUM).child(label.to_string()));
        for (index, (column, expr)) in rows.iter().enumerate() {
            let n = index + 1;
            let row_id = format!("{item}-{field}-{n}");
            let key_state = self.inputs.get(format!("{id}/{field}/{index}/k"), column, "", window, cx);
            let expr_state = self.inputs.get(format!("{id}/{field}/{index}/v"), expr, "", window, cx);
            let shown = if column.is_empty() { format!("{key_label} {n}") } else { column.clone() };
            let remove = {
                let (me, id, field) = (me.clone(), id.clone(), field.to_string());
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    let (id, field) = (id.clone(), field.clone());
                    me.update(cx, |this, cx| this.edit(move |root| map_remove(root, &id, &field, index), cx));
                }
            };
            body = body.child(
                h_flex()
                    .gap_2()
                    .items_end()
                    .child(text_field(format!("{row_id}-key"), &format!("{key_label} {n}"), &key_state, cx))
                    .child(expr_field(
                        &format!("{row_id}-expr"),
                        &format!("{shown} expression"),
                        &expr_state,
                        expression_problem(expr, true),
                        cx,
                    ))
                    .child(
                        Button::new(SharedString::from(format!("{row_id}-remove")))
                            .outline()
                            .small()
                            .label("Remove")
                            .on_click(remove),
                    ),
            );
        }
        let add = {
            let (me, id, field, key_label) =
                (me.clone(), id.clone(), field.to_string(), key_label.to_string());
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let (id, field, key_label) = (id.clone(), field.clone(), key_label.clone());
                me.update(cx, |this, cx| {
                    this.edit(move |root| map_add(root, &id, &field, &key_label), cx)
                });
            }
        };
        body.child(
            div().child(
                Button::new(SharedString::from(format!("{item}-{field}-add")))
                    .outline()
                    .small()
                    .label(format!("Add {}", key_label.to_lowercase()))
                    .on_click(add),
            ),
        )
    }

    // ---- Lists ---------------------------------------------------------------

    /// The list at `path` with its add controls. `prefix` names its items (`step`,
    /// `step-2-then`), `noun` the step ("Step", "Then step").
    #[allow(clippy::too_many_arguments)]
    fn list(
        &mut self,
        path: &ListPath,
        prefix: &str,
        noun: &str,
        controls: &str,
        steps: &[Value],
        env: &Env,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let me = cx.entity();
        let count = steps.len();
        let mut list = v_flex().gap_3();
        if steps.is_empty() {
            list = list.child(widgets::text(format!("{controls}-empty"), "No steps yet."));
        }
        for (index, step) in steps.iter().enumerate() {
            let item = format!("{prefix}-{}", index + 1);
            let id = text_of(step, "id");
            let title = format!("{noun} {}: {}", index + 1, step_label(&text_of(step, "kind")));
            let tool = |suffix: &str, label: &str, enabled: bool, act: Box<dyn Fn(&mut Self, &mut Context<Self>)>| {
                let me = me.clone();
                Button::new(SharedString::from(format!("{item}-{suffix}")))
                    .outline()
                    .xsmall()
                    .label(label.to_string())
                    .disabled(!enabled)
                    .on_click(move |_, _, cx| {
                        me.update(cx, |this, cx| act(this, cx));
                    })
            };
            let (p1, i1) = (path.clone(), id.clone());
            let (p2, i2) = (path.clone(), id.clone());
            let (p3, i3) = (path.clone(), id.clone());
            let tools = h_flex()
                .gap_1()
                .child(tool("up", "Up", index > 0, Box::new(move |this, cx| {
                    let (p, i) = (p1.clone(), i1.clone());
                    this.edit(move |root| move_step(root, &p, &i, -1), cx)
                })))
                .child(tool("down", "Down", index + 1 < count, Box::new(move |this, cx| {
                    let (p, i) = (p2.clone(), i2.clone());
                    this.edit(move |root| move_step(root, &p, &i, 1), cx)
                })))
                .child(tool("remove", "Remove", true, Box::new(move |this, cx| {
                    let (p, i) = (p3.clone(), i3.clone());
                    this.edit(move |root| remove_step(root, &p, &i), cx)
                })));
            let fields = self.fields(&item, path, step, env, window, cx);
            list = list.child(
                v_flex()
                    .id(SharedString::from(item.clone()))
                    .gap_2()
                    .p_3()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_md()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(widgets::styled_text(
                                format!("{item}-title"),
                                title,
                                |d| d.font_weight(FontWeight::SEMIBOLD),
                            ))
                            .child(tools),
                    )
                    .children(fields),
            );
        }
        let kind = self
            .kinds
            .get(path)
            .cloned()
            .unwrap_or_else(|| "updateRecord".to_string());
        let options = STEP_KINDS
            .iter()
            .map(|k| (k.to_string(), step_label(k).to_string()))
            .collect();
        let (entity, kind_path) = (me.clone(), path.clone());
        let add = {
            let (entity, path, kind) = (me.clone(), path.clone(), kind.clone());
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let (path, kind) = (path.clone(), kind.clone());
                entity.update(cx, |this, cx| {
                    this.edit(move |root| add_step(root, &path, &kind).is_some(), cx)
                });
            }
        };
        list.child(
            h_flex()
                .gap_2()
                .items_end()
                .child(widgets::field(
                    &format!("New {} kind", noun.to_lowercase()),
                    choice(
                        SharedString::from(format!("{controls}-kind")),
                        &kind,
                        options,
                        "Kind",
                        move |value, _, cx| {
                            let (value, path) = (value.to_string(), kind_path.clone());
                            entity.update(cx, |this, cx| {
                                this.kinds.insert(path, value);
                                cx.notify();
                            });
                        },
                    ),
                    cx,
                ))
                .child(
                    Button::new(SharedString::from(format!("{controls}-add")))
                        .outline()
                        .small()
                        .label(format!("Add {}", noun.to_lowercase()))
                        .on_click(add),
                ),
        )
    }

    // ---- Fields of each kind -------------------------------------------------

    fn fields(
        &mut self,
        item: &str,
        path: &ListPath,
        step: &Value,
        env: &Env,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let me = cx.entity();
        let id = text_of(step, "id");
        let kind = text_of(step, "kind");
        let row = |children: Vec<Div>| {
            h_flex()
                .gap_3()
                .flex_wrap()
                .items_start()
                .children(children)
                .into_any_element()
        };
        let table = |this: &Self, cx: &App| {
            this.pick(&me, item, step, "table", "Table", env.tables.clone(), "Table", cx)
        };
        let mut out: Vec<AnyElement> = vec![];
        match kind.as_str() {
            "createRecord" => {
                let t = table(self, cx);
                let store = self.text(item, step, "storeAs", "Store result as", window, cx);
                let when = self.when(item, step, window, cx);
                out.push(row(vec![t, store, when]));
                out.push(self.value_map(item, step, "values", ("Values", "Column"), window, cx).into_any_element());
            }
            "updateRecord" | "deleteRecord" => {
                let t = table(self, cx);
                let when = self.when(item, step, window, cx);
                out.push(row(vec![t, when]));
                out.push(self.match_fields(&me, item, step, window, cx));
                if kind == "updateRecord" {
                    out.push(self.value_map(item, step, "values", ("Values", "Column"), window, cx).into_any_element());
                }
            }
            "runQuery" => {
                let q = self.pick(&me, item, step, "queryId", "Query", env.queries.clone(), "No query", cx);
                let store = self.text(item, step, "storeAs", "Store rows as", window, cx);
                let when = self.when(item, step, window, cx);
                out.push(row(vec![q, store, when]));
                out.push(self.value_map(item, step, "params", ("Parameters", "Parameter"), window, cx).into_any_element());
            }
            "navigate" => {
                let target_kind = nested_text(step, "target", "kind");
                let targets = match target_kind.as_str() {
                    "form" => env.forms.clone(),
                    "report" => env.reports.clone(),
                    "dashboard" => env.dashboards.clone(),
                    _ => env.tables.clone(),
                };
                let kinds = plain(&["form", "report", "dashboard", "table"]);
                let picked = {
                    let (me, id) = (me.clone(), id.clone());
                    move |value: &str, _: &mut Window, cx: &mut App| {
                        let (id, value) = (id.clone(), value.to_string());
                        me.update(cx, |this, cx| {
                            this.set_value(&id, "target", Some(json!({"kind": value, "id": ""})), cx)
                        });
                    }
                };
                let kind_field = choice_field(
                    format!("{item}-target-kind"),
                    "Target kind",
                    &target_kind,
                    kinds,
                    "Kind",
                    picked,
                    cx,
                );
                let target = self.pick(&me, item, step, "target.id", "Target", targets, "Target", cx);
                let mode = self.text(item, step, "target.mode", "Mode", window, cx);
                let record = self.expr(item, step, "target.recordId", "Record id", false, "", window, cx);
                let when = self.when(item, step, window, cx);
                out.push(row(vec![kind_field, target, mode, record, when]));
            }
            "openForm" => {
                let form = self.pick(&me, item, step, "formId", "Form", env.forms.clone(), "Form", cx);
                let modes = FORM_MODES.iter().map(|(v, l)| (v.to_string(), l.to_string())).collect();
                let mode_value = match text_of(step, "mode").as_str() {
                    "" => "detail".to_string(),
                    other => other.to_string(),
                };
                let mode = choice_field(
                    format!("{item}-form-mode"),
                    "Form mode",
                    &mode_value,
                    modes,
                    "Mode",
                    Self::picker(&me, &id, "mode"),
                    cx,
                );
                let record = self.expr(item, step, "recordId", "Record id", false, "", window, cx);
                let popup = step.get("popup").and_then(Value::as_bool).unwrap_or(false);
                let toggle = {
                    let (me, id) = (me.clone(), id.clone());
                    check(
                        SharedString::from(format!("{item}-popup")),
                        "Open as popup and wait for it to close",
                        popup,
                        move |on, _, cx| {
                            let id = id.clone();
                            me.update(cx, |this, cx| {
                                this.edit(
                                    move |root| {
                                        if on {
                                            set_field(root, &id, "popup", Some(json!(true)))
                                        } else {
                                            let a = set_field(root, &id, "popup", None);
                                            let b = set_field(root, &id, "storeAs", None);
                                            a || b
                                        }
                                    },
                                    cx,
                                )
                            });
                        },
                    )
                };
                let mut cells = vec![form, mode, record, div().child(toggle)];
                if popup {
                    cells.push(self.text(item, step, "storeAs", "Store result as", window, cx));
                }
                cells.push(self.when(item, step, window, cx));
                out.push(row(cells));
            }
            "closeForm" => {
                let value = self.expr(item, step, "value", "Return value", false, "", window, cx);
                let when = self.when(item, step, window, cx);
                out.push(row(vec![value, when]));
            }
            "openReport" | "openDashboard" => {
                let (field, label, options) = if kind == "openReport" {
                    ("reportId", "Report", env.reports.clone())
                } else {
                    ("dashboardId", "Dashboard", env.dashboards.clone())
                };
                let target = self.pick(&me, item, step, field, label, options, label, cx);
                let when = self.when(item, step, window, cx);
                out.push(row(vec![target, when]));
                out.push(self.value_map(item, step, "params", ("Parameters", "Parameter"), window, cx).into_any_element());
            }
            "setState" => {
                let scope = self.pick(
                    &me,
                    item,
                    step,
                    "scope",
                    "Scope",
                    vec![("app".into(), "Application".into()), ("form".into(), "Form".into())],
                    "Scope",
                    cx,
                );
                let key = self.text(item, step, "key", "Key", window, cx);
                let value = self.expr(item, step, "value", "Value", true, "", window, cx);
                let when = self.when(item, step, window, cx);
                out.push(row(vec![scope, key, value, when]));
            }
            "confirm" | "fail" => {
                let label = if kind == "confirm" { "Message" } else { "Failure message" };
                let message = self.expr(item, step, "message", label, true, "", window, cx);
                let when = self.when(item, step, window, cx);
                out.push(row(vec![message, when]));
            }
            "message" => {
                let text = self.expr(item, step, "text", "Text", true, "", window, cx);
                let tone_value = match text_of(step, "tone").as_str() {
                    "" => "info".to_string(),
                    other => other.to_string(),
                };
                let tone = choice_field(
                    format!("{item}-tone"),
                    "Tone",
                    &tone_value,
                    vec![("info".into(), "Information".into()), ("error".into(), "Error".into())],
                    "Tone",
                    Self::picker(&me, &id, "tone"),
                    cx,
                );
                let when = self.when(item, step, window, cx);
                out.push(row(vec![text, tone, when]));
            }
            "setField" => {
                let field = self.text(item, step, "field", "Field", window, cx);
                let value = self.expr(item, step, "value", "Value", true, "", window, cx);
                let when = self.when(item, step, window, cx);
                out.push(row(vec![field, value, when]));
            }
            "condition" => {
                out.push(self.expr(item, step, "when", "If", true, "", window, cx).into_any_element());
                for (branch, noun) in [(Branch::Then, "Then step"), (Branch::Else, "Else step")] {
                    let branch_path = path.branch(&id, branch);
                    let nested: Vec<Value> = step
                        .get(branch.key())
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let prefix = format!("{item}-{}", branch.key());
                    let list = self.list(&branch_path, &prefix, noun, &prefix, &nested, env, window, cx);
                    out.push(list.into_any_element());
                }
            }
            "runAction" => {
                let mut options = env.actions.clone();
                options.retain(|(a, _)| *a != self.action_id);
                options.insert(0, (String::new(), "(none)".to_string()));
                let action = self.pick(&me, item, step, "actionId", "Action to run", options, "(none)", cx);
                let when = self.when(item, step, window, cx);
                out.push(row(vec![action, when]));
            }
            _ => out.push(div().child("Unknown step kind.").into_any_element()),
        }
        out
    }

    fn match_fields(
        &mut self,
        me: &Entity<Self>,
        item: &str,
        step: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = text_of(step, "id");
        let current = step.get("match").and_then(Value::as_str) == Some("current");
        let picked = {
            let (me, id) = (me.clone(), id.clone());
            move |value: &str, _: &mut Window, cx: &mut App| {
                let (id, value) = (id.clone(), value.to_string());
                me.update(cx, |this, cx| {
                    let want_current = value == "current";
                    this.edit(
                        move |root| {
                            let is_current = find_mut(root, &id)
                                .and_then(|s| s.get("match").map(|m| m.as_str() == Some("current")))
                                .unwrap_or(false);
                            if is_current == want_current {
                                return false;
                            }
                            let next = if want_current { json!("current") } else { json!({}) };
                            set_field(root, &id, "match", Some(next))
                        },
                        cx,
                    )
                });
            }
        };
        let which = choice_field(
            format!("{item}-match-kind"),
            "Which rows",
            if current { "current" } else { "match" },
            vec![
                ("current".into(), "The current record".into()),
                ("match".into(), "Rows matching columns".into()),
            ],
            "Rows",
            picked,
            cx,
        );
        let mut col = v_flex().gap_2().child(which);
        if !current {
            col = col.child(self.value_map(item, step, "match", ("Match", "Column"), window, cx));
        }
        col.into_any_element()
    }
}

/// `storeAs` -> `store-as`, `recordId` -> `record-id`.
fn kebab(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    match out.as_str() {
        "query-id" => "query".into(),
        "form-id" => "form".into(),
        "report-id" => "report".into(),
        "dashboard-id" => "dashboard".into(),
        "action-id" => "action".into(),
        "id" => "target".into(),
        _ => out,
    }
}

impl Render for StepEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let env = Env::read(self.shell.read(cx), cx);
        let steps = self.steps(cx);
        self.list(&ListPath::root(), "step", "Step", "steps", &steps, &env, window, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::{clears_when_empty, kebab, value_of};
    use serde_json::json;

    #[test]
    fn optional_fields_clear_when_emptied() {
        assert!(clears_when_empty("updateRecord", "when"));
        assert!(!clears_when_empty("condition", "when"));
        assert!(clears_when_empty("createRecord", "storeAs"));
        assert!(!clears_when_empty("runQuery", "storeAs"));
        assert!(clears_when_empty("closeForm", "value"));
        assert!(!clears_when_empty("setState", "value"));
        assert!(clears_when_empty("navigate", "target.mode"));
        assert!(!clears_when_empty("navigate", "target.id"));
    }

    #[test]
    fn ids_are_kebab_case_names_of_the_field() {
        assert_eq!(kebab("storeAs"), "store-as");
        assert_eq!(kebab("recordId"), "record-id");
        assert_eq!(kebab("queryId"), "query");
        assert_eq!(kebab("id"), "target");
        assert_eq!(kebab("table"), "table");
    }

    #[test]
    fn nested_values_are_read_through_the_dotted_field() {
        let step = json!({"target": {"kind": "form", "id": "f1"}, "table": "t"});
        assert_eq!(value_of(&step, "target.id"), "f1");
        assert_eq!(value_of(&step, "table"), "t");
        assert_eq!(value_of(&step, "missing"), "");
    }
}
