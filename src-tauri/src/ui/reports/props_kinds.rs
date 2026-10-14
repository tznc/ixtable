//! Kind-specific property controls: chart, table, subreport and conditional formatting
//! (ports of `ChartProperties.tsx`, `TableProperties`, `SubreportProperties` and
//! `ConditionsEditor.tsx`).
//!
//! Test ids: chart `chart-type`, `chart-title`, `chart-query`, `chart-x`, `chart-y`,
//! `chart-group`, `chart-stacked`, `chart-format`; table `table-query`,
//! `table-col-N-header|expression|width|format`, `remove-table-col-N`, `table-add-field`,
//! `add-table-column`; subreport `subreport-report`, `link-N-child`, `link-N-parent`,
//! `remove-link-N`, `add-link`; conditions `add-rule`, `rule-N-when`, `rule-N-bold`,
//! `rule-N-text`, `rule-N-fill`, `remove-rule-N` (N counts from 1).

use super::designer::ReportDesigner;
use super::extra;
use super::props::{fill_options, number_text, GRAYS};
use crate::report_engine::model::{field_expression, new_id, REPORT_SCOPE_NAMES};
use crate::reports::Component;
use crate::ui::prelude::*;
use serde_json::{json, Map, Value};

const CHART_TYPES: [(&str, &str); 6] = [
    ("bar", "Bar"),
    ("line", "Line"),
    ("area", "Area"),
    ("pie", "Pie"),
    ("donut", "Donut"),
    ("scatter", "Scatter"),
];

/// Names a conditional-formatting rule may use: the report names and the value itself.
fn condition_names() -> Vec<&'static str> {
    let mut names: Vec<&str> = REPORT_SCOPE_NAMES.to_vec();
    names.push("value");
    names
}

impl ReportDesigner {
    /// Replaces the list of objects under `key` of the selected component.
    pub(super) fn edit_list(
        &mut self,
        key: &'static str,
        edit: impl FnOnce(&mut Vec<Map<String, Value>>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        self.update_component(
            &id,
            "Edit report component",
            move |c| {
                let mut list = extra::objects(c, key);
                edit(&mut list);
                if list.is_empty() && key == "conditions" {
                    c.extra.remove(key);
                } else {
                    extra::set_objects(c, key, list);
                }
            },
            cx,
        );
    }

    /// Sets a field of item `index` of a list setting of the selected component.
    pub(super) fn edit_item(&mut self, key: &'static str, index: usize, field: &'static str, value: Value, cx: &mut Context<Self>) {
        self.edit_list(
            key,
            move |list| {
                if let Some(item) = list.get_mut(index) {
                    item.insert(field.to_string(), value);
                }
            },
            cx,
        );
    }

    /// The rows choice shared by tables and charts: this band's rows or a saved query.
    fn query_options(&self, cx: &App) -> Vec<(String, String)> {
        let mut options = vec![(String::new(), "This band's rows".to_string())];
        options.extend(
            self.shell
                .read(cx)
                .config(cx)
                .saved_queries
                .iter()
                .map(|q| (q.id.clone(), format!("Query: {}", q.name))),
        );
        options
    }

    pub(super) fn chart_properties(&mut self, c: &Component, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id = c.id.clone();
        let kind = extra::text(c, "chartType");
        let pie = kind == "pie" || kind == "donut";
        let mut section = super::fields::section("chart-properties", "Chart", cx);
        let target = id.clone();
        section = section.child(self.pick(
            "chart-type",
            "Chart type",
            &kind,
            super::fields::pairs(&CHART_TYPES),
            "Bar",
            move |this, value, cx| this.set_extra(&target, "chartType", json!(value), cx),
            cx,
        ));
        section = section.child(self.labeled("chart-title", "Chart title", &extra::text(c, "title"), "", window, cx));
        let target = id.clone();
        let options = self.query_options(cx);
        section = section.child(self.pick(
            "chart-query",
            "Chart rows from",
            &extra::text(c, "queryId"),
            options,
            "This band's rows",
            move |this, value, cx| this.set_extra(&target, "queryId", json!(value), cx),
            cx,
        ));
        let x_label = if kind == "scatter" { "X column" } else { "Category column" };
        section = section.child(self.labeled("chart-x", x_label, &extra::text(c, "xField"), "", window, cx));
        let y_label = if pie { "Value column" } else { "Value columns" };
        let y = extra::strings(c, "yFields").join(", ");
        let placeholder = if pie { "amount" } else { "amount, cost" };
        section = section.child(self.labeled("chart-y", y_label, &y, placeholder, window, cx));
        if !pie {
            section = section.child(self.labeled("chart-group", "Series by column", &extra::text(c, "groupBy"), "", window, cx));
        }
        if kind == "bar" || kind == "area" {
            let target = id;
            section = section.child(self.flag(
                "chart-stacked",
                "Stacked",
                extra::flag(c, "stacked"),
                move |this, v, cx| this.set_extra(&target, "stacked", if v { json!(true) } else { Value::Null }, cx),
                cx,
            ));
        }
        section.child(self.labeled("chart-format", "Value format", &extra::text(c, "format"), "#,##0", window, cx))
    }

    pub(super) fn table_properties(&mut self, c: &Component, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut section = super::fields::section("table-properties", "Table rows and columns", cx);
        let target = c.id.clone();
        let options = self.query_options(cx);
        section = section.child(self.pick(
            "table-query",
            "Rows from",
            &extra::text(c, "queryId"),
            options,
            "This band's rows",
            move |this, value, cx| this.set_extra(&target, "queryId", json!(value), cx),
            cx,
        ));
        for (i, col) in extra::objects(c, "columns").into_iter().enumerate() {
            let n = i + 1;
            let width = col.get("width").and_then(Value::as_f64).unwrap_or(100.0);
            let mut item = super::fields::section(SharedString::from(format!("table-col-{n}")), &format!("Column {n}"), cx);
            item = item
                .child(self.labeled(&format!("table-col-{n}-header"), &format!("Column {n} header"), &extra::str_of(&col, "header"), "", window, cx))
                .child(self.expression(
                    &format!("table-col-{n}-expression"),
                    &format!("Column {n} expression"),
                    &extra::str_of(&col, "expression"),
                    "",
                    &REPORT_SCOPE_NAMES,
                    window,
                    cx,
                ))
                .child(
                    h_flex()
                        .gap_2()
                        .child(self.labeled(&format!("table-col-{n}-width"), &format!("Column {n} width"), &number_text(width), "", window, cx).w(px(140.)))
                        .child(self.labeled(&format!("table-col-{n}-format"), &format!("Column {n} format"), &extra::str_of(&col, "format"), "", window, cx).w(px(140.))),
                )
                .child(
                    Button::new(SharedString::from(format!("remove-table-col-{n}")))
                        .outline()
                        .small()
                        .icon(IconName::Trash)
                        .label(format!("Remove column {n}"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.edit_list("columns", move |list| drop(list.remove(i)), cx);
                        })),
                );
            section = section.child(item);
        }
        let columns = self.columns.clone();
        section = section.child(self.pick(
            "table-add-field",
            "Add column for field",
            "",
            Self::same(&columns),
            "Choose a field…",
            |this, value, cx| {
                if !value.is_empty() {
                    this.add_table_column(value, cx);
                }
            },
            cx,
        ));
        section.child(
            Button::new("add-table-column")
                .outline()
                .small()
                .icon(IconName::Plus)
                .label("Add column")
                .on_click(cx.listener(|this, _, _, cx| this.add_table_column("", cx))),
        )
    }

    fn add_table_column(&mut self, field: &str, cx: &mut Context<Self>) {
        let field = field.to_string();
        self.edit_list(
            "columns",
            move |list| {
                let mut col = Map::new();
                col.insert("id".into(), json!(new_id()));
                col.insert("header".into(), json!(if field.is_empty() { "Column" } else { field.as_str() }));
                col.insert("expression".into(), json!(if field.is_empty() { String::new() } else { field_expression(&field) }));
                col.insert("width".into(), json!(100));
                list.push(col);
            },
            cx,
        );
    }

    pub(super) fn subreport_properties(&mut self, c: &Component, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let report = self.report(cx);
        let all = self.all_reports(cx);
        let choices = report.as_ref().map(|r| self.subreport_choices(r, cx)).unwrap_or_default();
        let current = extra::text(c, "reportId");
        let mut options = vec![(String::new(), "None".to_string())];
        // The report already chosen stays listed even when it is no longer embeddable.
        if let Some(r) = all.iter().find(|r| r.id == current && !choices.iter().any(|x| x.id == r.id)) {
            options.push((r.id.clone(), r.name.clone()));
        }
        options.extend(choices.iter().map(|r| (r.id.clone(), r.name.clone())));
        let target = c.id.clone();
        let mut section = super::fields::section("subreport-properties", "Subreport", cx);
        section = section
            .child(self.pick(
                "subreport-report",
                "Report",
                &current,
                options,
                "None",
                move |this, value, cx| this.set_extra(&target, "reportId", json!(value), cx),
                cx,
            ))
            .child(widgets::styled_text(
                "subreport-hint",
                "Subreport rows print when each child field equals the parent field of this band's row. Without links, every row prints.",
                |d| d.text_xs(),
            ));
        for (i, link) in extra::objects(c, "links").into_iter().enumerate() {
            let n = i + 1;
            let mut item = super::fields::section(SharedString::from(format!("link-{n}")), &format!("Link {n}"), cx);
            item = item
                .child(self.labeled(&format!("link-{n}-child"), &format!("Link {n} child field"), &extra::str_of(&link, "child"), "", window, cx))
                .child(self.labeled(&format!("link-{n}-parent"), &format!("Link {n} parent field"), &extra::str_of(&link, "master"), "", window, cx))
                .child(
                    Button::new(SharedString::from(format!("remove-link-{n}")))
                        .outline()
                        .small()
                        .icon(IconName::Trash)
                        .label(format!("Remove link {n}"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.edit_list("links", move |list| drop(list.remove(i)), cx);
                        })),
                );
            section = section.child(item);
        }
        section.child(
            Button::new("add-link")
                .outline()
                .small()
                .icon(IconName::Plus)
                .label("Add link")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.edit_list(
                        "links",
                        |list| {
                            let mut link = Map::new();
                            link.insert("child".into(), json!(""));
                            link.insert("master".into(), json!(""));
                            list.push(link);
                        },
                        cx,
                    );
                })),
        )
    }

    /// Sets a style entry of rule `index`: `None` leaves it as is, `Some(Null)` clears it.
    fn set_rule_style(&mut self, index: usize, key: &'static str, value: Option<Value>, cx: &mut Context<Self>) {
        self.edit_list(
            "conditions",
            move |list| {
                let Some(rule) = list.get_mut(index) else {
                    return;
                };
                let mut style = match rule.remove("style") {
                    Some(Value::Object(m)) => m,
                    _ => Map::new(),
                };
                match value {
                    Some(v) => style.insert(key.to_string(), v),
                    None => style.remove(key),
                };
                rule.insert("style".into(), Value::Object(style));
            },
            cx,
        );
    }

    pub(super) fn conditions_editor(&mut self, c: &Component, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut section = super::fields::section("conditions", "Conditional formatting", cx);
        for (i, rule) in extra::objects(c, "conditions").into_iter().enumerate() {
            let n = i + 1;
            let style = rule.get("style").and_then(Value::as_object).cloned().unwrap_or_default();
            let bold = style.get("bold").and_then(Value::as_bool).unwrap_or(false);
            let mut item = super::fields::section(SharedString::from(format!("rule-{n}")), &format!("Rule {n}"), cx);
            item = item
                .child(self.expression(
                    &format!("rule-{n}-when"),
                    &format!("Rule {n} condition"),
                    &extra::str_of(&rule, "when"),
                    "value < 0",
                    &condition_names(),
                    window,
                    cx,
                ))
                .child(self.flag(
                    &format!("rule-{n}-bold"),
                    &format!("Rule {n} bold"),
                    bold,
                    move |this, v, cx| this.set_rule_style(i, "bold", v.then(|| json!(true)), cx),
                    cx,
                ));
            // "keep" leaves the setting as is, "" clears it, a number sets it.
            let gray = style.get("gray").map(|v| v.as_f64().map_or(String::new(), number_text));
            let mut grays = vec![("keep".to_string(), "As is".to_string())];
            grays.extend(GRAYS.iter().map(|(name, v)| (number_text(*v), name.to_string())));
            let mut fills = vec![("keep".to_string(), "As is".to_string())];
            fills.extend(fill_options());
            let fill = style.get("fill").map(|v| v.as_f64().map_or(String::new(), number_text));
            item = item
                .child(self.pick(
                    &format!("rule-{n}-text"),
                    &format!("Rule {n} text"),
                    gray.as_deref().unwrap_or("keep"),
                    grays,
                    "As is",
                    move |this, value, cx| {
                        let v = match value {
                            "keep" | "" => None,
                            n => n.parse::<f64>().ok().map(|n| json!(n)),
                        };
                        this.set_rule_style(i, "gray", v, cx);
                    },
                    cx,
                ))
                .child(self.pick(
                    &format!("rule-{n}-fill"),
                    &format!("Rule {n} fill"),
                    fill.as_deref().unwrap_or("keep"),
                    fills,
                    "As is",
                    move |this, value, cx| {
                        let v = match value {
                            "keep" => None,
                            "" => Some(Value::Null),
                            n => n.parse::<f64>().ok().map(|n| json!(n)),
                        };
                        this.set_rule_style(i, "fill", v, cx);
                    },
                    cx,
                ))
                .child(
                    Button::new(SharedString::from(format!("remove-rule-{n}")))
                        .outline()
                        .small()
                        .icon(IconName::Trash)
                        .label(format!("Remove rule {n}"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.edit_list("conditions", move |list| drop(list.remove(i)), cx);
                        })),
                );
            section = section.child(item);
        }
        section.child(
            Button::new("add-rule")
                .outline()
                .small()
                .icon(IconName::Plus)
                .label("Add rule")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.edit_list(
                        "conditions",
                        |list| {
                            let mut rule = Map::new();
                            rule.insert("id".into(), json!(new_id()));
                            rule.insert("when".into(), json!(""));
                            rule.insert("style".into(), json!({ "bold": true }));
                            list.push(rule);
                        },
                        cx,
                    );
                })),
        )
    }
}
