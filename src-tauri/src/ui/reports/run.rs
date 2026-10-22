//! A report opened from Runtime navigation or an action (port of `ReportRun.tsx`). When
//! the report's queries declare parameters the opener didn't supply, it first prompts for
//! them (prefilled with defaults), then shows the preview.
//!
//! Test ids: the prompt is `report-params` (label "<report> parameters") with inputs
//! `param-<name>` (a checkbox for a boolean), `params-error`, `params-cancel`,
//! `params-run`; the bar above the preview has `report-not-run` and `change-params`
//! ("Change parameters…" / "Enter parameters…").

use crate::ui::controls::{check, input, Inputs};
use super::preview::ReportPreview;
use crate::archive::QueryParameter;
use crate::report_engine::parameters::{display, parse_parameter, report_parameters, Raw};
use crate::ui::{prelude::*, shell::Shell};
use serde_json::{Map, Value};
use std::collections::HashMap;

pub enum PromptEvent {
    Run(Map<String, Value>),
    Cancel,
}

/// Asks for a report's parameter values, prefilled with the given values or defaults.
pub struct ParameterPrompt {
    report_name: String,
    parameters: Vec<QueryParameter>,
    raw: HashMap<String, Raw>,
    error: String,
    inputs: Inputs<ParameterPrompt, String>,
    focus: FocusHandle,
    focused_first: bool,
}
impl EventEmitter<PromptEvent> for ParameterPrompt {}

impl ParameterPrompt {
    pub fn new(
        report_name: &str,
        parameters: Vec<QueryParameter>,
        initial: &Map<String, Value>,
        cx: &mut Context<Self>,
    ) -> Self {
        let raw = parameters
            .iter()
            .map(|p| {
                let value = initial.get(&p.name).or(p.default_value.as_ref());
                let raw = if p.logical_type == "boolean" {
                    Raw::Flag(value == Some(&Value::Bool(true)))
                } else {
                    Raw::Text(display(value))
                };
                (p.name.clone(), raw)
            })
            .collect();
        Self {
            report_name: report_name.to_string(),
            parameters,
            raw,
            error: String::new(),
            inputs: Inputs::new(Self::on_edit),
            focus: cx.focus_handle(),
            focused_first: false,
        }
    }

    fn on_edit(this: &mut Self, key: &String, text: String, cx: &mut Context<Self>) {
        this.raw.insert(key.clone(), Raw::Text(text));
        cx.notify();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let mut values = Map::new();
        for p in &self.parameters {
            let raw = self.raw.get(&p.name).cloned().unwrap_or(Raw::Text(String::new()));
            match parse_parameter(p, &raw) {
                Ok(value) => {
                    values.insert(p.name.clone(), value);
                }
                Err(message) => {
                    self.error = message;
                    cx.notify();
                    return;
                }
            }
        }
        cx.emit(PromptEvent::Run(values));
    }
}

impl Render for ParameterPrompt {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let mut rows = vec![];
        for (i, p) in self.parameters.clone().into_iter().enumerate() {
            let label = format!("{}{}", p.name, if p.required { " (required)" } else { "" });
            let id = format!("param-{}", p.name);
            if p.logical_type == "boolean" {
                let checked = self.raw.get(&p.name) == Some(&Raw::Flag(true));
                let (entity, name) = (entity.clone(), p.name.clone());
                rows.push(
                    div()
                        .child(check(SharedString::from(id), &label, checked, move |value, _, cx| {
                            entity.update(cx, |this, cx| {
                                this.raw.insert(name.clone(), Raw::Flag(value));
                                cx.notify();
                            })
                        }))
                        .into_any_element(),
                );
                continue;
            }
            let text = match self.raw.get(&p.name) {
                Some(Raw::Text(t)) => t.clone(),
                _ => String::new(),
            };
            let state = self.inputs.get(p.name.clone(), &text, "", window, cx);
            if i == 0 && !self.focused_first {
                self.focused_first = true;
                state.update(cx, |s, cx| s.focus(window, cx));
            }
            rows.push(widgets::field(&label, input(&state, id, &label, false), cx).into_any_element());
        }
        let theme = cx.theme();
        let (background, border, danger) = (theme.background, theme.border, theme.danger);
        let cancel = entity.clone();
        div()
            .id("report-params")
            .aria_label(format!("{} parameters", self.report_name))
            .key_context("ReportParams")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| match e.keystroke.key.as_str() {
                "escape" => cx.emit(PromptEvent::Cancel),
                "enter" => this.submit(cx),
                _ => {}
            }))
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui_kit::black().opacity(0.4))
            .child(
                v_flex()
                    .w(px(380.))
                    .gap_2()
                    .p_4()
                    .rounded_lg()
                    .border_1()
                    .border_color(border)
                    .bg(background)
                    .child(widgets::section_heading("params-title", &self.report_name))
                    .child(div().ty(Type::Body).child("Enter the values to run this report with."))
                    .children(rows)
                    .when(!self.error.is_empty(), |el| {
                        el.child(widgets::styled_text("params-error", self.error.clone(), move |d| {
                            d.text_color(danger)
                        }))
                    })
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("params-cancel")
                                    .outline()
                                    .small()
                                    .label("Cancel")
                                    .on_click(move |_, _, cx| cancel.update(cx, |_, cx| cx.emit(PromptEvent::Cancel))),
                            )
                            .child(
                                Button::new("params-run")
                                    .primary()
                                    .small()
                                    .label("Run report")
                                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                            ),
                    ),
            )
            .test_support()
    }
}

pub struct ReportRun {
    shell: Entity<Shell>,
    report_id: String,
    given: Map<String, Value>,
    /// The values the report ran with; `None` until the prompt was answered.
    pub values: Option<Map<String, Value>>,
    pub prompting: bool,
    ready: bool,
    prompt: Option<(Entity<ParameterPrompt>, Subscription)>,
    pub preview: Option<Entity<ReportPreview>>,
    _subscription: Subscription,
}

impl ReportRun {
    pub fn new(shell: Entity<Shell>, report_id: &str, params: Option<Map<String, Value>>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
        Self {
            shell,
            report_id: report_id.to_string(),
            given: params.unwrap_or_default(),
            values: None,
            prompting: false,
            ready: false,
            prompt: None,
            preview: None,
            _subscription: subscription,
        }
    }

    fn parameters(&self, cx: &App) -> Vec<QueryParameter> {
        let config = self.shell.read(cx).config(cx);
        match config.reports.iter().find(|r| r.id == self.report_id) {
            Some(report) => report_parameters(report, &config.saved_queries),
            None => vec![],
        }
    }

    /// The first render decides whether the opener supplied everything.
    fn init(&mut self, cx: &mut Context<Self>) {
        self.ready = true;
        let parameters = self.parameters(cx);
        // A required parameter passed as null still needs a value, so it prompts too.
        let complete = parameters.iter().all(|p| {
            self.given
                .get(&p.name)
                .is_some_and(|v| !p.required || !v.is_null())
        });
        if complete {
            self.show(self.given.clone(), cx);
        } else {
            self.open_prompt(cx);
        }
    }

    fn show(&mut self, values: Map<String, Value>, cx: &mut Context<Self>) {
        self.values = Some(values.clone());
        match &self.preview {
            Some(preview) => preview.update(cx, |p, cx| p.set_params(Some(values), cx)),
            None => {
                let (shell, id) = (self.shell.clone(), self.report_id.clone());
                self.preview = Some(cx.new(|cx| ReportPreview::new(shell, &id, Some(values), true, cx)));
            }
        }
        cx.notify();
    }

    /// The text the open prompt holds for parameter `name`.
    #[cfg(test)]
    pub fn prompt_text(&self, name: &str, cx: &App) -> Option<String> {
        let (prompt, _) = self.prompt.as_ref()?;
        match prompt.read(cx).raw.get(name) {
            Some(Raw::Text(t)) => Some(t.clone()),
            Some(Raw::Flag(f)) => Some(f.to_string()),
            None => None,
        }
    }

    /// Opens the parameter prompt, prefilled with the last values or the given ones.
    pub fn open_prompt(&mut self, cx: &mut Context<Self>) {
        let config = self.shell.read(cx).config(cx);
        let Some(report) = config.reports.iter().find(|r| r.id == self.report_id) else {
            return;
        };
        let name = report.name.clone();
        let parameters = self.parameters(cx);
        let initial = self.values.clone().unwrap_or_else(|| self.given.clone());
        let prompt = cx.new(|cx| ParameterPrompt::new(&name, parameters, &initial, cx));
        let subscription = cx.subscribe(&prompt, |this, _, event: &PromptEvent, cx| {
            this.prompting = false;
            this.prompt = None;
            if let PromptEvent::Run(entered) = event {
                let mut values = this.given.clone();
                values.extend(entered.clone());
                this.show(values, cx);
            }
            cx.notify();
        });
        self.prompt = Some((prompt, subscription));
        self.prompting = true;
        cx.notify();
    }
}

impl Render for ReportRun {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.ready {
            self.init(cx);
        }
        let has_report = self.shell.read(cx).config(cx).reports.iter().any(|r| r.id == self.report_id);
        if !has_report && self.preview.is_none() {
            let (shell, id, given) = (self.shell.clone(), self.report_id.clone(), self.given.clone());
            self.preview = Some(cx.new(|cx| ReportPreview::new(shell, &id, Some(given), true, cx)));
        }
        let with_parameters = !self.parameters(cx).is_empty();
        let not_run = self.values.is_none();
        v_flex()
            .id("report-run")
            .relative()
            .size_full()
            .when(with_parameters && !self.prompting, |el| {
                el.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .px_3()
                        .py_1p5()
                        .when(not_run, |row| row.child(widgets::text("report-not-run", "The report was not run.")))
                        .child(
                            Button::new("change-params")
                                .outline()
                                .small()
                                .label(if not_run { "Enter parameters…" } else { "Change parameters…" })
                                .on_click(cx.listener(|this, _, _, cx| this.open_prompt(cx))),
                        ),
                )
            })
            .when_some(self.preview.clone(), |el, preview| {
                el.child(div().flex_1().min_h_0().child(preview))
            })
            .when_some(self.prompt.as_ref().map(|(p, _)| p.clone()), |el, prompt| el.child(prompt))
    }
}
