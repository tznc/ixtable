//! The properties panel of the designer (port of `ComponentProperties.tsx`): the selected
//! component's settings, or the band and report settings when nothing is selected.
//!
//! Test ids: `props-title`, `prop-text`, `prop-bound-field` (the Value picker), `prop-expression` (shown for a custom value),
//! `prop-format`, `prop-running-sum`, `prop-image`, `prop-direction`, `prop-x`, `prop-y`,
//! `prop-w`, `prop-h`, `prop-font-size`, `prop-align`, `prop-bold`, `prop-can-grow`,
//! `prop-border-width`, `prop-fill`, `delete-component`. Chart, table, subreport and
//! condition controls are in `props_kinds.rs`, band and report settings in `settings.rs`.

use super::designer::ReportDesigner;
use super::extra;
use super::fields::pairs;
use crate::ui::controls::{check, choice_field, expr_field, input};
use crate::report_engine::model::{field_expression, is_page_band, kind_label, BandKey, REPORT_SCOPE_NAMES};
use crate::reports::{Component, Report};
use crate::ui::prelude::*;
use serde_json::{json, Value};

/// The Value picker's option that reveals the expression input.
const CUSTOM_VALUE: &str = "\u{1}custom";

/// Background fills offered for report boxes (gray levels).
pub const FILLS: [(&str, Option<f64>); 4] = [
    ("None", None),
    ("Light gray", Some(0.9)),
    ("Gray", Some(0.75)),
    ("Dark gray", Some(0.5)),
];

/// Text gray levels offered by conditional formatting.
pub const GRAYS: [(&str, f64); 4] = [("Black", 0.0), ("Dark gray", 0.35), ("Gray", 0.55), ("Light gray", 0.75)];

/// A number as the property fields show it (`10`, `7.5`).
pub fn number_text(n: f64) -> String {
    if n.is_finite() {
        format!("{n}")
    } else {
        "0".to_string()
    }
}

/// The fill options as (value, label) pairs; "" is no fill.
pub fn fill_options() -> Vec<(String, String)> {
    FILLS
        .iter()
        .map(|(name, v)| (v.map_or(String::new(), number_text), name.to_string()))
        .collect()
}

impl ReportDesigner {
    /// A labeled single-line input.
    pub(super) fn labeled(
        &mut self,
        id: &str,
        label: &str,
        value: &str,
        placeholder: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let state = self.inputs.get(id.to_string(), value, placeholder, window, cx);
        widgets::field(label, input(&state, id.to_string(), label, false), cx)
    }

    /// A labeled expression input with its live problem line.
    pub(super) fn expression(
        &mut self,
        id: &str,
        label: &str,
        value: &str,
        placeholder: &str,
        names: &[&str],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let state = self.inputs.get(id.to_string(), value, placeholder, window, cx);
        let problem = (!value.trim().is_empty())
            .then(|| {
                let messages: Vec<String> =
                    crate::expr::check(value, Some(names)).into_iter().map(|d| d.message).collect();
                (!messages.is_empty()).then(|| messages.join("; "))
            })
            .flatten();
        expr_field(id, label, &state, problem, cx)
    }

    /// A checkbox that edits the designer.
    pub(super) fn flag(
        &self,
        id: &str,
        label: &str,
        checked: bool,
        on: impl Fn(&mut Self, bool, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let entity = cx.entity();
        check(SharedString::from(id.to_string()), label, checked, move |value, _, cx| {
            entity.update(cx, |this, cx| on(this, value, cx))
        })
    }

    /// A labeled choice that edits the designer.
    pub(super) fn pick(
        &self,
        id: &str,
        label: &str,
        current: &str,
        options: Vec<(String, String)>,
        placeholder: &str,
        on: impl Fn(&mut Self, &str, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Div {
        let entity = cx.entity();
        choice_field(
            SharedString::from(id.to_string()),
            label,
            current,
            options,
            placeholder,
            move |value, _, cx| entity.update(cx, |this, cx| on(this, value, cx)),
            cx,
        )
    }

    /// A list of strings as (value, label) pairs of themselves.
    pub(super) fn same(values: &[String]) -> Vec<(String, String)> {
        values.iter().map(|v| (v.clone(), v.clone())).collect()
    }

    pub(super) fn render_properties(&mut self, report: &Report, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self
            .selected
            .as_ref()
            .and_then(|id| crate::report_engine::model::find_component(report, id))
            .map(|(key, c)| (key, c.clone()));
        let muted = cx.theme().muted_foreground;
        let body = match selected {
            Some((key, c)) => self.component_properties(&key, &c, window, cx).into_any_element(),
            None => self.band_and_report(report, window, cx).into_any_element(),
        };
        v_flex()
            .gap_2()
            .child(div().text_xs().text_color(muted).child("PROPERTIES"))
            .child(body)
    }

    fn component_properties(&mut self, key: &BandKey, c: &Component, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let page_band = is_page_band(key);
        let is_text = matches!(c.kind.as_str(), "staticText" | "field" | "calculated");
        let id = c.id.clone();
        let mut col = v_flex().gap_2().child(widgets::styled_text(
            "props-title",
            format!("{} properties", kind_label(&c.kind)),
            |d| d.font_weight(FontWeight::BOLD),
        ));
        if c.kind == "staticText" {
            col = col.child(self.labeled("prop-text", "Text", &extra::text(c, "text"), "", window, cx));
        }
        if matches!(c.kind.as_str(), "field" | "calculated") {
            let expression = extra::text(c, "expression");
            let bound = extra::bound_field(&expression).filter(|f| self.columns.contains(f));
            let picker = c.kind == "field" && !self.columns.is_empty();
            let custom = !picker
                || self.custom_value.contains(&id)
                || (bound.is_none() && !expression.trim().is_empty());
            if picker {
                let (target, columns) = (id.clone(), self.columns.clone());
                let mut options = Self::same(&columns);
                options.push((CUSTOM_VALUE.to_string(), "Custom expression…".to_string()));
                let current = if custom { CUSTOM_VALUE.to_string() } else { bound.clone().unwrap_or_default() };
                col = col.child(self.pick(
                    "prop-bound-field",
                    "Value",
                    &current,
                    options,
                    "Choose a field…",
                    move |this, value, cx| {
                        if value == CUSTOM_VALUE {
                            this.custom_value.insert(target.clone());
                            cx.notify();
                        } else if !value.is_empty() {
                            this.custom_value.remove(&target);
                            this.set_extra(&target, "expression", json!(field_expression(value)), cx);
                        }
                    },
                    cx,
                ));
            }
            if custom {
                let placeholder = if c.kind == "field" { "record.amount" } else { "sum(rows.amount)" };
                let label = if picker { "Custom expression" } else if c.kind == "field" { "Value" } else { "Expression" };
                col = col.child(self.expression("prop-expression", label, &expression, placeholder, &REPORT_SCOPE_NAMES, window, cx));
            }
            col = col.child(self.labeled("prop-format", "Format", &extra::text(c, "format"), "#,##0.00 or MMM d, yyyy", window, cx));
            if !page_band {
                let target = id.clone();
                col = col.child(self.pick(
                    "prop-running-sum",
                    "Running sum",
                    &extra::text(c, "runningSum"),
                    pairs(&[("", "No"), ("group", "Over group"), ("all", "Over all")]),
                    "No",
                    move |this, value, cx| {
                        let v = if value.is_empty() { Value::Null } else { json!(value) };
                        this.set_extra(&target, "runningSum", v, cx);
                    },
                    cx,
                ));
            }
        }
        match c.kind.as_str() {
            "chart" => col = col.child(self.chart_properties(c, window, cx)),
            "table" => col = col.child(self.table_properties(c, window, cx)),
            "subreport" => col = col.child(self.subreport_properties(c, window, cx)),
            "image" => {
                let target = id.clone();
                let mut options = vec![(String::new(), "None".to_string())];
                options.extend(
                    self.assets
                        .iter()
                        .filter(|a| a.media_type.starts_with("image/"))
                        .map(|a| (a.id.clone(), a.display_name.clone())),
                );
                col = col.child(self.pick(
                    "prop-image",
                    "Image asset",
                    &extra::text(c, "assetId"),
                    options,
                    "None",
                    move |this, value, cx| this.set_extra(&target, "assetId", json!(value), cx),
                    cx,
                ));
            }
            "line" => {
                let target = id.clone();
                let current = extra::text(c, "orientation");
                col = col.child(self.pick(
                    "prop-direction",
                    "Direction",
                    if current.is_empty() { "horizontal" } else { &current },
                    pairs(&[("horizontal", "Horizontal"), ("vertical", "Vertical")]),
                    "Horizontal",
                    move |this, value, cx| this.set_extra(&target, "orientation", json!(value), cx),
                    cx,
                ));
            }
            _ => {}
        }
        col = col.child(self.geometry_section(c, window, cx));
        col = col.child(self.style_section(c, page_band, is_text, window, cx));
        if is_text {
            col = col.child(self.conditions_editor(c, window, cx));
        }
        let target = id;
        col.child(
            Button::new("delete-component")
                .danger()
                .outline()
                .small()
                .icon(IconName::Trash)
                .label("Delete component")
                .on_click(cx.listener(move |this, _, _, cx| this.remove(&target, cx))),
        )
    }

    fn geometry_section(&mut self, c: &Component, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (x, y) = (number_text(c.x), number_text(c.y));
        let (w, h) = (number_text(c.w), number_text(c.h));
        let grid = h_flex()
            .flex_wrap()
            .gap_2()
            .child(self.labeled("prop-x", "X", &x, "", window, cx).w(px(150.)))
            .child(self.labeled("prop-y", "Y", &y, "", window, cx).w(px(150.)))
            .child(self.labeled("prop-w", "Width", &w, "", window, cx).w(px(150.)))
            .child(self.labeled("prop-h", "Height", &h, "", window, cx).w(px(150.)));
        let summary = format!("X {}, Y {}, {} x {}", x, y, w, h);
        crate::ui::section::section("prop-geometry", "Position and size (pt)", false, window, cx).summary(summary).child(grid).render(cx)
    }

    fn style_section(
        &mut self,
        c: &Component,
        page_band: bool,
        is_text: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = c.id.clone();
        let mut section = crate::ui::section::section("prop-style", "Style", true, window, cx);
        let table = c.kind == "table";
        if is_text || table {
            let size = extra::style_num(c, "fontSize").unwrap_or(if table { 9.0 } else { 10.0 });
            let align = extra::style_text(c, "align").unwrap_or_else(|| "left".into());
            let target = id.clone();
            section = section.child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.labeled("prop-font-size", "Font size", &number_text(size), "", window, cx).w(px(120.)))
                    .child(self.pick(
                        "prop-align",
                        "Alignment",
                        &align,
                        pairs(&[("left", "Left"), ("center", "Center"), ("right", "Right")]),
                        "Left",
                        move |this, value, cx| this.set_style(&target, "align", json!(value), cx),
                        cx,
                    )),
            );
        }
        if is_text {
            let target = id.clone();
            section = section.child(self.flag(
                "prop-bold",
                "Bold",
                extra::style_flag(c, "bold"),
                move |this, v, cx| this.set_style(&target, "bold", json!(v), cx),
                cx,
            ));
            if !page_band {
                let target = id.clone();
                section = section.child(self.flag(
                    "prop-can-grow",
                    "Can grow",
                    extra::flag(c, "canGrow"),
                    move |this, v, cx| this.set_extra(&target, "canGrow", if v { json!(true) } else { Value::Null }, cx),
                    cx,
                ));
            }
        }
        if c.kind != "subreport" {
            let default = if matches!(c.kind.as_str(), "line" | "rectangle") { 1.0 } else { 0.0 };
            let width = extra::style_num(c, "borderWidth").unwrap_or(default);
            let label = if c.kind == "line" { "Line width" } else { "Border width" };
            section = section.child(self.labeled("prop-border-width", label, &number_text(width), "", window, cx).w(px(120.)));
        }
        if !matches!(c.kind.as_str(), "line" | "table" | "subreport") {
            let target = id;
            let current = extra::style_num(c, "fill").map_or(String::new(), number_text);
            section = section.child(self.pick(
                "prop-fill",
                "Fill",
                &current,
                fill_options(),
                "None",
                move |this, value, cx| {
                    let v = value.parse::<f64>().map_or(Value::Null, |n| json!(n));
                    this.set_style(&target, "fill", v, cx);
                },
                cx,
            ));
        }
        section.render(cx)
    }

    /// Sets a `style` entry of a component; `Null` removes it.
    pub(super) fn set_style(&mut self, id: &str, key: &'static str, value: Value, cx: &mut Context<Self>) {
        self.update_component(id, "Edit report component", move |c| extra::set_style(c, key, value), cx);
    }
}
