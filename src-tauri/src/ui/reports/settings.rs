//! Band and report settings of the designer's properties panel (port of
//! `ReportSettings.tsx`) and the dispatcher for every text edit of the panel.
//!
//! Test ids: `band-height`, `band-keep-together`, `band-break-before`,
//! `band-break-after`; parameters `param-<name>`; `page-size`, `page-orientation`,
//! `margin-top|right|bottom|left`; groups `add-group`, `group-N-field`,
//! `group-N-expression`, `group-N-descending`, `group-N-newPage`, `group-N-repeatHeader`,
//! `group-N-resetPageNumber`, `group-N-newpage-hint`, `remove-group-N` (N counts from 1).

use super::designer::{parse_number, ReportDesigner};
use super::fields::pairs;
use crate::ui::section::section;
use super::props::number_text;
use crate::report_engine::model::{
    band_entries, band_min_height, field_expression, group_label, is_page_band, new_group, with_band, REPORT_SCOPE_NAMES,
};
use crate::report_engine::parameters::display;
use crate::reports::Report;
use crate::ui::prelude::*;
use serde_json::{json, Value};

const GROUP_FLAGS: [(&str, &str); 3] = [
    ("newPage", "Start on a new page"),
    ("repeatHeader", "Repeat header on each page"),
    ("resetPageNumber", "Restart page numbers"),
];

/// `prefix<n>-<rest>` as (n, rest), with n counted from 1.
fn indexed<'a>(key: &'a str, prefix: &str) -> Option<(usize, &'a str)> {
    let rest = key.strip_prefix(prefix)?;
    let (n, rest) = rest.split_once('-')?;
    Some((n.parse::<usize>().ok()?.checked_sub(1)?, rest))
}

/// A typed parameter value: a number when it reads as one, else the text.
fn parameter_value(raw: &str) -> Value {
    match raw.trim().parse::<f64>() {
        Ok(n) if !raw.trim().is_empty() && n.is_finite() => {
            if n.fract() == 0.0 && n.abs() < 1e15 {
                json!(n as i64)
            } else {
                json!(n)
            }
        }
        _ => json!(raw),
    }
}

impl ReportDesigner {
    /// Every text edit of the panel arrives here with the key of its input.
    pub(super) fn on_edit(this: &mut Self, key: &String, text: String, cx: &mut Context<Self>) {
        if let Some(name) = key.strip_prefix("param-") {
            let name = name.to_string();
            let value = parameter_value(&text);
            this.change("Edit parameters", move |r| drop(r.params.insert(name, value)), cx);
            return;
        }
        if let Some(side) = key.strip_prefix("margin-") {
            if let Some(n) = parse_number(&text, 0.0) {
                let side = side.to_string();
                this.change(
                    "Page setup",
                    move |r| match side.as_str() {
                        "top" => r.page.margins.top = n,
                        "right" => r.page.margins.right = n,
                        "bottom" => r.page.margins.bottom = n,
                        _ => r.page.margins.left = n,
                    },
                    cx,
                );
            }
            return;
        }
        if let Some((index, rest)) = indexed(key, "group-") {
            if rest == "expression" {
                this.edit_group(index, move |g| g.group_by = text, cx);
            }
            return;
        }
        if let Some((index, rest)) = indexed(key, "table-col-") {
            let value = match rest {
                "width" => parse_number(&text, 1.0).map(|n| json!(n)),
                _ => Some(json!(text)),
            };
            let field = match rest {
                "header" => "header",
                "expression" => "expression",
                "width" => "width",
                _ => "format",
            };
            if let Some(value) = value {
                this.edit_item("columns", index, field, value, cx);
            }
            return;
        }
        if let Some((index, rest)) = indexed(key, "link-") {
            let field = if rest == "child" { "child" } else { "master" };
            this.edit_item("links", index, field, json!(text), cx);
            return;
        }
        if let Some((index, _)) = indexed(key, "rule-") {
            this.edit_item("conditions", index, "when", json!(text), cx);
            return;
        }
        let Some(id) = this_selected(this) else {
            this_band_edit(this, key, &text, cx);
            return;
        };
        match key.as_str() {
            "prop-text" => this.set_extra(&id, "text", json!(text), cx),
            "prop-expression" => this.set_extra(&id, "expression", json!(text), cx),
            "prop-format" => this.set_extra(&id, "format", json!(text), cx),
            "prop-x" | "prop-y" | "prop-w" | "prop-h" => {
                let min = if key == "prop-w" || key == "prop-h" { 1.0 } else { 0.0 };
                if let Some(n) = parse_number(&text, min) {
                    this.set_geometry(&id, &key[5..], n, cx);
                }
            }
            "prop-font-size" => {
                if let Some(n) = parse_number(&text, 4.0) {
                    this.set_style(&id, "fontSize", json!(n), cx);
                }
            }
            "prop-border-width" => {
                if let Some(n) = parse_number(&text, 0.0) {
                    this.set_style(&id, "borderWidth", json!(n), cx);
                }
            }
            "chart-title" | "chart-group" | "chart-format" => {
                let field = match key.as_str() {
                    "chart-title" => "title",
                    "chart-group" => "groupBy",
                    _ => "format",
                };
                let v = if text.is_empty() { Value::Null } else { json!(text) };
                this.set_extra(&id, field, v, cx);
            }
            "chart-x" => this.set_extra(&id, "xField", json!(text), cx),
            "chart-y" => {
                let parts: Vec<String> = text.split(',').map(|s| s.trim().to_string()).collect();
                let last = parts.len().saturating_sub(1);
                let kept: Vec<String> = parts
                    .into_iter()
                    .enumerate()
                    .filter(|(i, s)| !s.is_empty() || *i < last)
                    .map(|(_, s)| s)
                    .collect();
                this.set_extra(&id, "yFields", json!(kept), cx);
            }
            _ => this_band_edit(this, key, &text, cx),
        }
    }

    fn edit_group(&mut self, index: usize, edit: impl FnOnce(&mut crate::reports::ReportGroup) + 'static, cx: &mut Context<Self>) {
        self.change(
            "Edit group",
            move |r| {
                if let Some(g) = r.bands.groups.get_mut(index) {
                    edit(g);
                }
            },
            cx,
        );
    }

    pub(super) fn band_and_report(&mut self, report: &Report, window: &mut Window, cx: &mut Context<Self>) -> Div {
        v_flex()
            .gap_2()
            .child(self.band_properties(report, window, cx))
            .child(self.report_settings(report, window, cx))
    }

    fn band_properties(&mut self, report: &Report, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let key = self.active_band(report);
        let entries = band_entries(report);
        let Some(entry) = entries.iter().find(|e| e.key == key) else {
            return div().into_any_element();
        };
        let height = number_text(entry.band.height);
        let min = band_min_height(entry.band);
        let page_band = is_page_band(&key);
        let mut s = section("band-properties", &entry.label, true, window, cx);
        s = s.child(self.labeled("band-height", &format!("Band height (at least {})", number_text(min)), &height, "", window, cx));
        let k = key.clone();
        s = s.child(self.flag(
            "band-keep-together",
            "Keep together",
            entry.band.keep_together,
            move |this, v, cx| this.change("Edit band", { let k = k.clone(); move |r| with_band(r, &k, |b| b.keep_together = v) }, cx),
            cx,
        ));
        if !page_band {
            let k = key.clone();
            s = s.child(self.flag(
                "band-break-before",
                "Page break before",
                entry.band.page_break_before,
                move |this, v, cx| this.change("Edit band", { let k = k.clone(); move |r| with_band(r, &k, |b| b.page_break_before = v) }, cx),
                cx,
            ));
            let k = key.clone();
            s = s.child(self.flag(
                "band-break-after",
                "Page break after",
                entry.band.page_break_after,
                move |this, v, cx| this.change("Edit band", { let k = k.clone(); move |r| with_band(r, &k, |b| b.page_break_after = v) }, cx),
                cx,
            ));
        }
        s.render(cx).into_any_element()
    }

    fn report_settings(&mut self, report: &Report, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut out = v_flex().gap_2();
        let query = report
            .dataset_query_id
            .as_ref()
            .and_then(|id| self.shell.read(cx).config(cx).saved_queries.iter().find(|q| &q.id == id).cloned());
        if let Some(q) = query.filter(|q| !q.parameters.is_empty()) {
            let mut s = section("report-parameters", "Parameters", true, window, cx).count(q.parameters.len());
            for p in &q.parameters {
                let shown = display(report.params.get(&p.name).or(p.default_value.as_ref()));
                s = s.child(self.labeled(&format!("param-{}", p.name), &p.name, &shown, "", window, cx));
            }
            out = out.child(s.render(cx));
        }
        let page = report.page.clone();
        let m = &page.margins;
        let margin = |this: &mut Self, side: &str, v: f64, window: &mut Window, cx: &mut Context<Self>| {
            let label = format!("{}{} margin", side[..1].to_uppercase(), &side[1..]);
            this.labeled(&format!("margin-{side}"), &label, &number_text(v), "", window, cx).w(px(150.))
        };
        let orientation = if page.orientation == "landscape" { "landscape" } else { "portrait" };
        out = out.child(
            section("page-setup", "Page setup", false, window, cx)
                .summary(format!("{}, {orientation}", page.size))
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .child(self.pick(
                            "page-size",
                            "Page size",
                            &page.size,
                            pairs(&[("A4", "A4"), ("Letter", "Letter")]),
                            "A4",
                            |this, v, cx| { let v = v.to_string(); this.change("Page setup", move |r| r.page.size = v, cx) },
                            cx,
                        ))
                        .child(self.pick(
                            "page-orientation",
                            "Orientation",
                            &page.orientation,
                            pairs(&[("portrait", "Portrait"), ("landscape", "Landscape")]),
                            "Portrait",
                            |this, v, cx| { let v = v.to_string(); this.change("Page setup", move |r| r.page.orientation = v, cx) },
                            cx,
                        )),
                )
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .child(margin(self, "top", m.top, window, cx))
                        .child(margin(self, "right", m.right, window, cx))
                        .child(margin(self, "bottom", m.bottom, window, cx))
                        .child(margin(self, "left", m.left, window, cx)),
                )
                .render(cx),
        );
        out.child(self.groups_section(report, window, cx))
    }

    fn groups_section(&mut self, report: &Report, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut s = section("report-groups", "Groups", true, window, cx)
            .count(report.bands.groups.len())
            .summary("No groups yet.");
        let muted = cx.theme().muted_foreground;
        for (i, g) in report.bands.groups.iter().enumerate() {
            let n = i + 1;
            let mut item = section(SharedString::from(format!("group-{n}")), &group_label(g, i), true, window, cx);
            if !self.columns.is_empty() {
                let columns = self.columns.clone();
                item = item.child(self.pick(
                    &format!("group-{n}-field"),
                    "Field",
                    "",
                    Self::same(&columns),
                    "Choose a field…",
                    move |this, v, cx| {
                        if !v.is_empty() {
                            let expression = field_expression(v);
                            this.edit_group(i, move |g| g.group_by = expression, cx);
                        }
                    },
                    cx,
                ));
            }
            item = item
                .child(self.expression(
                    &format!("group-{n}-expression"),
                    "Expression",
                    &g.group_by,
                    "",
                    &REPORT_SCOPE_NAMES,
                    window,
                    cx,
                ))
                .child(self.flag(
                    &format!("group-{n}-descending"),
                    "Sort descending",
                    g.descending,
                    move |this, v, cx| this.edit_group(i, move |g| g.descending = v, cx),
                    cx,
                ));
            for (flag, text) in GROUP_FLAGS {
                let forced = flag == "newPage" && g.reset_page_number;
                let checked = forced
                    || match flag {
                        "newPage" => g.new_page,
                        "repeatHeader" => g.repeat_header,
                        _ => g.reset_page_number,
                    };
                // A forced flag is shown on and can't be turned off from here.
                item = item.child(self.flag(
                    &format!("group-{n}-{flag}"),
                    text,
                    checked,
                    move |this, v, cx| {
                        if forced {
                            return;
                        }
                        this.edit_group(
                            i,
                            move |g| match flag {
                                "newPage" => g.new_page = v,
                                "repeatHeader" => g.repeat_header = v,
                                _ => g.reset_page_number = v,
                            },
                            cx,
                        )
                    },
                    cx,
                ));
                if forced {
                    item = item.child(widgets::styled_text(
                        format!("group-{n}-newpage-hint"),
                        "Always on while group page numbers restart.",
                        move |d| d.text_xs().text_color(muted),
                    ));
                }
            }
            item = item.child(
                Button::new(SharedString::from(format!("remove-group-{n}")))
                    .outline()
                    .small()
                    .icon(IconName::Trash)
                    .label("Remove group")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.change("Remove group", move |r| drop(r.bands.groups.remove(i)), cx);
                    })),
            );
            s = s.child(item.render(cx));
        }
        let first = self.columns.first().map(|c| field_expression(c)).unwrap_or_default();
        s.child(
            Button::new("add-group")
                .outline()
                .small()
                .icon(IconName::Plus)
                .label("Add group")
                .on_click(cx.listener(move |this, _, _, cx| {
                    let group = new_group(&first);
                    this.change("Add group", move |r| r.bands.groups.push(group), cx);
                })),
        )
        .render(cx)
    }
}

/// The selected component's id, if any.
fn this_selected(this: &ReportDesigner) -> Option<String> {
    this.selected.clone()
}

/// Edits that belong to the band panel (band height).
fn this_band_edit(this: &mut ReportDesigner, key: &str, text: &str, cx: &mut Context<ReportDesigner>) {
    if key != "band-height" {
        return;
    }
    let Some(report) = this.report(cx) else {
        return;
    };
    let band_key = this.active_band(&report);
    let min = crate::report_engine::model::get_band(&report, &band_key).map_or(0.0, band_min_height);
    if let Some(n) = parse_number(text, min) {
        this.change("Edit band", move |r| with_band(r, &band_key, |b| b.height = n), cx);
    }
}
