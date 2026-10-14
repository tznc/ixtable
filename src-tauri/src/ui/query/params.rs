//! Parameter declarations and the run-time "Run with" values. Port of
//! src/query/ParametersPanel.tsx.
//!
//! Test ids: `param-type:<name>`, `param-default:<name>`, `param-required:<name>`,
//! `param-remove:<name>`, `declare:<name>`, `new-param-name`, `add-param`,
//! `param-value:<name>`.

use super::{QueryMode, choice::Opt, inputs::Field};
use crate::{
    archive::{QueryParameter, SavedQuery},
    ui::{prelude::*, widgets},
};
use gpui_kit::component::checkbox::Checkbox;
use serde_json::Value;

/// Parameter logical types accepted by `queries` (`logical_type`).
pub const PARAMETER_TYPES: [&str; 6] = ["text", "integer", "number", "boolean", "date", "timestamp"];

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A default value as the input shows it.
pub fn show_default(value: &Option<Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

impl QueryMode {
    fn new_parameter_name(&self, cx: &App) -> String {
        self.input_text(&Field::NewParam, cx).trim().to_string()
    }

    /// Whether Add parameter can be pressed (a valid name that is not declared yet).
    pub fn can_add_parameter(&self, cx: &App) -> bool {
        let name = self.new_parameter_name(cx);
        valid_name(&name)
            && self
                .draft
                .as_ref()
                .is_some_and(|d| !d.query.parameters.iter().any(|p| p.name == name))
    }

    pub(super) fn declare_parameter(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.to_string();
        self.edit_query(
            |q| {
                q.parameters.push(QueryParameter {
                    name,
                    logical_type: "text".into(),
                    default_value: None,
                    required: false,
                })
            },
            cx,
        );
    }

    pub(super) fn add_typed_parameter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_add_parameter(cx) {
            return;
        }
        let name = self.new_parameter_name(cx);
        self.declare_parameter(&name, cx);
        self.set_input(&Field::NewParam, "", window, cx);
    }

    fn patch_parameter(&mut self, name: &str, f: impl FnOnce(&mut QueryParameter), cx: &mut Context<Self>) {
        let name = name.to_string();
        self.edit_query(
            |q| {
                if let Some(p) = q.parameters.iter_mut().find(|p| p.name == name) {
                    f(p);
                }
            },
            cx,
        );
    }

    pub(super) fn render_parameters_panel(
        &mut self,
        query: &SavedQuery,
        referenced: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let missing: Vec<String> = referenced
            .iter()
            .filter(|n| !query.parameters.iter().any(|p| p.name == **n))
            .cloned()
            .collect();
        let mut rows = vec![];
        for p in &query.parameters {
            let name = p.name.clone();
            let default = self.input_el(
                Field::ParamDefault(name.clone()),
                format!("param-default:{name}"),
                format!("Default for {name}"),
                &show_default(&p.default_value),
                "Default",
                window,
                cx,
            );
            let types: Vec<Opt> = PARAMETER_TYPES.iter().map(|t| Opt::same(*t)).collect();
            let kind = self
                .choice(
                    format!("param-type:{name}"),
                    p.logical_type.clone(),
                    types,
                    {
                        let name = name.clone();
                        move |this, value, _, cx| this.patch_parameter(&name, |p| p.logical_type = value, cx)
                    },
                    cx,
                )
                .render(cx);
            rows.push(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().w(px(110.)).text_sm().child(format!("${name}")))
                    .child(kind)
                    .child(div().w(px(160.)).child(default))
                    .child(
                        Checkbox::new(SharedString::from(format!("param-required:{name}")))
                            .label("Required")
                            .checked(p.required)
                            .on_click(cx.listener({
                                let name = name.clone();
                                move |this, checked: &bool, _, cx| {
                                    let checked = *checked;
                                    this.patch_parameter(&name, |p| p.required = checked, cx)
                                }
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("param-remove:{name}")))
                            .ghost()
                            .small()
                            .icon(IconName::Trash)
                            .tooltip(format!("Remove parameter {name}"))
                            .on_click(cx.listener({
                                let name = name.clone();
                                move |this, _, _, cx| {
                                    this.edit_query(|q| q.parameters.retain(|p| p.name != name), cx)
                                }
                            })),
                    ),
            );
        }
        let declares: Vec<_> = missing.into_iter().map(|n| {
            Button::new(SharedString::from(format!("declare:{n}")))
                .outline()
                .small()
                .icon(IconName::Plus)
                .label(format!("Declare ${n}"))
                .on_click(cx.listener(move |this, _, _, cx| this.declare_parameter(&n, cx)))
        }).collect();
        let new_name = self.input_el(
            Field::NewParam,
            "new-param-name",
            "New parameter name",
            "",
            "parameter_name",
            window,
            cx,
        );
        let valid = self.can_add_parameter(cx);
        super::panel("query-parameters", "Parameters", cx)
            .when(query.parameters.is_empty(), |el| {
                el.child(widgets::text(
                    "no-parameters",
                    "No parameters. Use $name in filters or SQL.",
                ))
            })
            .children(rows)
            .children(declares)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().w(px(200.)).child(new_name))
                    .child(
                        Button::new("add-param")
                            .outline()
                            .small()
                            .icon(IconName::Plus)
                            .label("Add parameter")
                            .disabled(!valid)
                            .on_click(cx.listener(|this, _, window, cx| this.add_typed_parameter(window, cx))),
                    ),
            )
    }

    /// Run-time values for parameters; empty fields fall back to defaults.
    pub(super) fn render_parameter_values(
        &mut self,
        query: &SavedQuery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if query.parameters.is_empty() {
            return None;
        }
        let mut rows = vec![];
        for p in &query.parameters {
            let name = p.name.clone();
            let label = format!("{}{}", p.name, if p.required { " *" } else { "" });
            let control = if p.logical_type == "boolean" {
                let default = show_default(&p.default_value);
                let current = self.values.get(&name).cloned().unwrap_or_default();
                let options = vec![
                    Opt::new("", format!("Default ({})", if default.is_empty() { "none" } else { &default })),
                    Opt::same("true"),
                    Opt::same("false"),
                ];
                self.choice(
                    format!("param-value:{name}"),
                    current,
                    options,
                    {
                        let name = name.clone();
                        move |this, value, _, cx| {
                            this.values.insert(name.clone(), value);
                            this.parameters_changed(cx);
                        }
                    },
                    cx,
                )
                .render(cx)
            } else {
                let hint = show_default(&p.default_value);
                let hint = if hint.is_empty() { p.logical_type.clone() } else { hint };
                let initial = self.values.get(&name).cloned().unwrap_or_default();
                div()
                    .w(px(220.))
                    .child(self.input_el(
                        Field::ParamValue(name.clone()),
                        format!("param-value:{name}"),
                        format!("Value for {name}"),
                        &initial,
                        &hint,
                        window,
                        cx,
                    ))
                    .into_any_element()
            };
            rows.push(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().w(px(110.)).text_sm().child(label))
                    .child(control),
            );
        }
        Some(
            super::panel("query-param-values", "Run with", cx)
                .children(rows)
                .into_any_element(),
        )
    }
}
