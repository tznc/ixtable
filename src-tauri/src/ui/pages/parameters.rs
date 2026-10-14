//! Page parameters: the values a page takes when navigation, a page button or an "Open
//! page" step opens it. Components read them as `params`, like filter values.
//!
//! Test ids: `add-page-param`, `page-params-hint` while there are none, and per parameter
//! (`<n>` from 1) `page-param-name:<n>`, `page-param-type:<n>`, `page-param-default:<n>`,
//! `page-param-required:<n>` and `page-param-remove:<n>`.

use super::editor::PageEditor;
use crate::{
    archive::QueryParameter,
    pages::Page,
    ui::{
        controls::{check, choice_field, input},
        prelude::*,
    },
};
use serde_json::Value;

const PARAMETER_TYPES: [&str; 6] = ["text", "integer", "number", "boolean", "date", "timestamp"];

impl PageEditor {
    fn change_param(&mut self, index: usize, label: &str, mutate: impl FnOnce(&mut QueryParameter) + 'static, cx: &mut Context<Self>) {
        self.edit(label, move |p| {
            if let Some(param) = p.parameters.get_mut(index) {
                mutate(param);
            }
        }, cx);
    }

    pub(super) fn on_param_edit(&mut self, key: &str, text: &str, cx: &mut Context<Self>) -> bool {
        let Some((field, n)) = key.split_once(':').filter(|(f, _)| f.starts_with("page-param-")) else { return false };
        let Some(index) = n.parse::<usize>().ok().and_then(|n| n.checked_sub(1)) else { return true };
        let Some(current) = self.dashboard(cx).and_then(|p| p.parameters.get(index).cloned()) else { return true };
        let (field, text) = (field.to_string(), text.to_string());
        let apply = move |p: &mut QueryParameter| match field.as_str() {
            "page-param-name" => p.name = text.trim().to_string(),
            "page-param-default" => p.default_value = (!text.is_empty()).then(|| Value::String(text.clone())),
            _ => {}
        };
        let mut next = current.clone();
        apply(&mut next);
        if next != current {
            self.change_param(index, "Edit page parameter", apply, cx);
        }
        true
    }

    fn add_param(&mut self, cx: &mut Context<Self>) {
        let Some(page) = self.dashboard(cx) else { return };
        let name = (1..)
            .map(|n| if n == 1 { "param".to_string() } else { format!("param{n}") })
            .find(|n| !page.parameters.iter().any(|p| &p.name == n))
            .unwrap_or_default();
        let param = QueryParameter { name, logical_type: "text".into(), default_value: None, required: false };
        self.edit("Add page parameter", move |p| p.parameters.push(param), cx);
    }

    pub(super) fn render_parameters(&mut self, page: &Page, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        let mut panel = v_flex().gap_2().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Parameters"));
        if page.parameters.is_empty() {
            panel = panel.child(widgets::styled_text(
                "page-params-hint",
                "Parameters are values this page receives when it is opened. Components read them as params.",
                move |d| d.text_xs().text_color(muted),
            ));
        }
        for (i, param) in page.parameters.iter().enumerate() {
            let n = i + 1;
            let name = self.inputs.get(format!("page-param-name:{n}"), &param.name, "", window, cx);
            let shown = match &param.default_value {
                None | Some(Value::Null) => String::new(),
                Some(Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
            };
            let default = self.inputs.get(format!("page-param-default:{n}"), &shown, "", window, cx);
            let logical = choice_field(
                format!("page-param-type:{n}"),
                "Value type",
                &param.logical_type,
                PARAMETER_TYPES.iter().map(|t| (t.to_string(), t.to_string())).collect(),
                "text",
                Self::on_pick(cx, move |this, v, cx| {
                    let v = v.to_string();
                    this.change_param(i, "Edit page parameter", move |p| p.logical_type = v, cx)
                }),
                cx,
            );
            let required = param.required;
            panel = panel.child(
                v_flex()
                    .id(SharedString::from(format!("page-param:{n}")))
                    .aria_label(format!("Parameter {}", param.name))
                    .gap_2()
                    .p_2()
                    .border_1()
                    .border_color(border)
                    .rounded_md()
                    .child(widgets::field("Parameter name", input(&name, format!("page-param-name:{n}"), "Parameter name", true), cx))
                    .child(logical)
                    .child(widgets::field("Default value", input(&default, format!("page-param-default:{n}"), "Default value", false), cx))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                check(format!("page-param-required:{n}"), "Required", required, {
                                    let editor = cx.entity();
                                    move |value, _, cx| {
                                        editor.update(cx, |this, cx| this.change_param(i, "Edit page parameter", move |p| p.required = value, cx))
                                    }
                                }),
                            )
                            .child(
                                Button::new(SharedString::from(format!("page-param-remove:{n}")))
                                    .outline()
                                    .small()
                                    .icon(IconName::Trash)
                                    .tooltip(format!("Remove parameter {}", param.name))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.edit("Remove page parameter", move |p| {
                                            if i < p.parameters.len() {
                                                p.parameters.remove(i);
                                            }
                                        }, cx)
                                    })),
                            ),
                    ),
            );
        }
        panel.child(
            Button::new("add-page-param")
                .outline()
                .small()
                .icon(IconName::Plus)
                .label("Add parameter")
                .on_click(cx.listener(|this, _, _, cx| this.add_param(cx))),
        )
    }
}
