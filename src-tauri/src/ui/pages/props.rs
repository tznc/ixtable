//! Properties of the selected dashboard component (port of `ComponentProperties.tsx` and
//! `ConditionProperties.tsx`). Field pickers list the query's result columns.
//!
//! Test ids (every control takes its id; a choice's options are the items of the
//! `popup-menu` it opens; an expression's problem line is `<id>-problem`): `prop-title`,
//! `prop-query` (the shared `QueryPicker`; options are `prop-query/<query id>`),
//! `prop-columns-error`, `prop-value-field`, `prop-expression`, `prop-format`,
//! `prop-comparison-field`, `prop-comparison-expression`, `prop-comparison-label`,
//! `prop-chart-type`, `prop-x`, `prop-y:<column>`, `prop-group-by`, `prop-stacked`,
//! `prop-column:<column>`, `prop-page-size`, `prop-filter`, `prop-form`, `prop-form-mode`,
//! `prop-record-id`, `prop-report`, `prop-label`, `prop-action`, `prop-text`,
//! `prop-row-filter`, `prop-style-<n>-column|when|tone|up|down|remove`, `prop-add-style`,
//! `prop-visible-when`, `prop-enabled-when`, `prop-hint`, `prop-region`, `prop-delete` and
//! `dashboard-settings`.

use super::{editor::PageEditor, mode_name, parse_mode};
use crate::{
    page_engine::model::{CHART_TYPES, chart_key, chart_label, embeddable_modes, kind_label, new_id},
    pages::{ChartType, ComponentKind, Page, PageComponent},
    design::{ConditionalStyle, Tone},
    forms::state::expression_problem,
    ui::{
        automation::action_picker,
        prelude::*,
        controls::{check, choice_field, expr_field, input},
    },
};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::input::InputEvent;

const DASHBOARD_NAMES: [&str; 2] = ["params", "app"];
const TONES: [(&str, &str); 5] =
    [("positive", "Positive"), ("negative", "Negative"), ("warning", "Warning"), ("muted", "Muted"), ("emphasis", "Emphasis")];

fn opt(text: &str) -> Option<String> {
    (!text.is_empty()).then(|| text.to_string())
}

fn tone(name: &str) -> Tone {
    match name {
        "positive" => Tone::Positive,
        "negative" => Tone::Negative,
        "warning" => Tone::Warning,
        "muted" => Tone::Muted,
        "emphasis" => Tone::Emphasis,
        other => Tone::Other(other.into()),
    }
}

/// Names a table row filter or style may use: one result row as `record`, plus `value`.
fn row_names(columns: &[String], value: bool) -> Vec<String> {
    let mut names: Vec<String> = if columns.is_empty() { vec!["record".into()] } else { columns.iter().map(|c| format!("record.{c}")).collect() };
    names.extend(DASHBOARD_NAMES.iter().map(|n| n.to_string()));
    if value {
        names.push("value".into());
    }
    names
}

/// Sets the text field named by `key`; false when `key` is not one.
fn apply_text(key: &str, c: &mut PageComponent, text: &str) -> bool {
    match key {
        "prop-title" => c.title = text.into(),
        "prop-expression" => c.expression = opt(text),
        "prop-format" => c.format = opt(text),
        "prop-comparison-expression" => {
            let has_field = c.comparison.as_ref().is_some_and(|k| k.value_field.is_some());
            c.comparison = if !text.is_empty() || has_field {
                let mut k = c.comparison.take().unwrap_or_default();
                k.expression = opt(text);
                Some(k)
            } else {
                None
            };
        }
        "prop-comparison-label" => {
            let mut k = c.comparison.take().unwrap_or_default();
            k.label = text.into();
            c.comparison = Some(k);
        }
        "prop-page-size" => c.page_size = Some(text.trim().parse::<f64>().map_or(1, |n| n.round().max(1.0) as u32)),
        "prop-label" => c.label = opt(text),
        "prop-record-id" => c.record_id = opt(text),
        "prop-row-filter" => c.filter = opt(text),
        "prop-visible-when" => c.visible_when = opt(text),
        "prop-enabled-when" => c.enabled_when = opt(text),
        "prop-text" => c.text = Some(text.into()),
        other if other.starts_with("prop-page-param:") => {
            let name = other.trim_start_matches("prop-page-param:").to_string();
            if text.trim().is_empty() {
                c.page_params.remove(&name);
            } else {
                c.page_params.insert(name, text.into());
            }
        }
        other => {
            let Some(rest) = other.strip_prefix("prop-style-").and_then(|r| r.strip_suffix("-when")) else { return false };
            let Some(style) = rest.parse::<usize>().ok().and_then(|n| c.styles.get_mut(n)) else { return false };
            style.when = text.into();
        }
    }
    true
}

impl PageEditor {
    pub(super) fn on_component_edit(&mut self, key: &str, text: &str, cx: &mut Context<Self>) -> bool {
        if !key.starts_with("prop-") {
            return false;
        }
        let Some(current) = self.selected_component(cx) else { return true };
        let mut next = current.clone();
        if !apply_text(key, &mut next, text) || next == current {
            return true;
        }
        let (key, text) = (key.to_string(), text.to_string());
        self.change("Edit component", move |c| {
            apply_text(&key, c, &text);
        }, cx);
        true
    }

    /// "Edit form" / "New form" (and the report equivalents): opens the definition's designer.
    fn open_button(id: &'static str, label: &'static str, cx: &Context<Self>) -> Button {
        Button::new(id)
            .primary()
            .small()
            .icon(IconName::Pencil)
            .label(label)
            .on_click(cx.listener(|this, _, window, cx| this.open_target(window, cx)))
    }

    /// A callback that runs `f` on this editor with the picked value.
    pub(super) fn on_pick(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, &str, &mut Context<Self>) + 'static,
    ) -> impl Fn(&str, &mut Window, &mut App) + 'static {
        let editor = cx.entity();
        move |value, _, cx| editor.update(cx, |this, cx| f(this, value, cx))
    }

    pub(super) fn text_field(
        &mut self,
        id: &str,
        label: &str,
        value: &str,
        placeholder: &str,
        code: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let state = self.inputs.get(id.to_string(), value, placeholder, window, cx);
        widgets::field(label, input(&state, id.to_string(), label, code), cx)
    }

    /// An expression input with its live problem line.
    pub(super) fn expression_field(
        &mut self,
        id: &str,
        label: &str,
        value: &str,
        placeholder: &str,
        names: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let state = self.inputs.get(id.to_string(), value, placeholder, window, cx);
        let problem = expression_problem(Some(value), &[], Some(names));
        expr_field(id, label, &state, (!problem.is_empty()).then_some(problem), cx)
    }

    fn column_select(
        &self,
        id: &str,
        label: &str,
        value: Option<&str>,
        on_pick: impl Fn(&mut Self, Option<String>, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut options = vec![(String::new(), "(none)".to_string())];
        options.extend(self.columns.columns.iter().map(|c| (c.clone(), c.clone())));
        let pick = Self::on_pick(cx, move |this, v, cx| on_pick(this, opt(v), cx));
        choice_field(id.to_string(), label, value.unwrap_or(""), options, "(none)", pick, cx)
    }

    fn column_checks(
        &self,
        id: &str,
        legend: &str,
        selected: &[String],
        on_change: fn(&mut PageComponent, Vec<String>),
        cx: &mut Context<Self>,
    ) -> Div {
        let mut all = self.columns.columns.clone();
        let extra: Vec<String> = selected.iter().filter(|c| !all.contains(c)).cloned().collect();
        all.extend(extra);
        let hint = all.is_empty().then(|| widgets::text("prop-columns-hint", "Choose a query to list its columns."));
        v_flex().gap_1().child(div().text_sm().font_weight(FontWeight::MEDIUM).child(legend.to_string())).children(hint).children(
            all.iter().map(|column| {
                let (selected_now, all_now, name) = (selected.to_vec(), all.clone(), column.clone());
                let on = selected.contains(column);
                let editor = cx.entity();
                check(format!("{id}:{column}"), column, on, move |checked, _, cx| {
                    let (selected_now, all_now, name) = (selected_now.clone(), all_now.clone(), name.clone());
                    let next: Vec<String> = if checked {
                        all_now.into_iter().filter(|x| *x == name || selected_now.contains(x)).collect()
                    } else {
                        selected_now.into_iter().filter(|x| *x != name).collect()
                    };
                    editor.update(cx, |this, cx| this.change("Edit component", move |c| on_change(c, next), cx));
                })
            }),
        )
    }

    fn style_rows(&mut self, component: &PageComponent, names: &[String], window: &mut Window, cx: &mut Context<Self>) -> Div {
        let count = component.styles.len();
        let mut rows = v_flex().gap_2();
        for (n, style) in component.styles.iter().enumerate() {
            let mut columns = vec![(String::new(), "(none)".to_string())];
            columns.extend(self.columns.columns.iter().map(|c| (c.clone(), c.clone())));
            let column = choice_field(
                format!("prop-style-{n}-column"),
                &format!("Style {} column", n + 1),
                style.column.as_deref().unwrap_or(""),
                columns,
                "(none)",
                Self::on_pick(cx, move |this, v, cx| {
                    let v = opt(v);
                    this.change("Edit conditional styles", move |c| {
                        if let Some(s) = c.styles.get_mut(n) {
                            s.column = v;
                        }
                    }, cx)
                }),
                cx,
            );
            let when = self.expression_field(
                &format!("prop-style-{n}-when"),
                &format!("Style {} when", n + 1),
                &style.when,
                "record.amount > 100",
                names,
                window,
                cx,
            );
            let tones = TONES.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect();
            let tone_pick = choice_field(
                format!("prop-style-{n}-tone"),
                &format!("Style {} tone", n + 1),
                style.tone.as_str(),
                tones,
                "Emphasis",
                Self::on_pick(cx, move |this, v, cx| {
                    let t = tone(v);
                    this.change("Edit conditional styles", move |c| {
                        if let Some(s) = c.styles.get_mut(n) {
                            s.tone = t;
                        }
                    }, cx)
                }),
                cx,
            );
            let editor = cx.entity();
            let button = |id: String, label: &'static str, icon: IconName, disabled: bool, f: fn(&mut Vec<ConditionalStyle>, usize)| {
                let editor = editor.clone();
                Button::new(SharedString::from(id))
                    .ghost()
                    .small()
                    .icon(icon)
                    .tooltip(label)
                    .disabled(disabled)
                    .on_click(move |_, _, cx| {
                        editor.update(cx, |this, cx| this.change("Edit conditional styles", move |c| f(&mut c.styles, n), cx))
                    })
            };
            rows = rows.child(
                v_flex()
                    .gap_1()
                    .p_2()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_md()
                    .child(column)
                    .child(when)
                    .child(tone_pick)
                    .child(
                        h_flex()
                            .gap_1()
                            .child(button(format!("prop-style-{n}-up"), "Move style up", IconName::ArrowUp, n == 0, |s, n| s.swap(n, n - 1)))
                            .child(button(format!("prop-style-{n}-down"), "Move style down", IconName::ArrowDown, n + 1 == count, |s, n| s.swap(n, n + 1)))
                            .child(button(format!("prop-style-{n}-remove"), "Remove style", IconName::Trash, false, |s, n| {
                                s.remove(n);
                            })),
                    ),
            );
        }
        rows
    }

    fn condition_properties(&mut self, c: &PageComponent, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let mut panel = v_flex().gap_2();
        if c.kind == ComponentKind::Table {
            let columns = self.columns.columns.clone();
            panel = panel
                .child(self.expression_field(
                    "prop-row-filter",
                    "Row filter",
                    c.filter.as_deref().unwrap_or(""),
                    "record.amount > 0",
                    &row_names(&columns, false),
                    window,
                    cx,
                ))
                .child(self.style_rows(c, &row_names(&columns, true), window, cx))
                .child(
                    Button::new("prop-add-style")
                        .outline()
                        .small()
                        .icon(IconName::Plus)
                        .label("Add style")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.change("Edit conditional styles", |c| {
                                c.styles.push(ConditionalStyle { id: new_id(), when: String::new(), tone: Tone::Emphasis, column: None })
                            }, cx)
                        })),
                );
        }
        let names: Vec<String> = DASHBOARD_NAMES.iter().map(|n| n.to_string()).collect();
        panel
            .child(self.expression_field(
                "prop-visible-when",
                "Visible when",
                c.visible_when.as_deref().unwrap_or(""),
                "params.region <> null",
                &names,
                window,
                cx,
            ))
            .child(self.expression_field(
                "prop-enabled-when",
                "Enabled when",
                c.enabled_when.as_deref().unwrap_or(""),
                "not isnull(params.region)",
                &names,
                window,
                cx,
            ))
    }

    fn text_area(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) -> Entity<TextareaState> {
        if let Some((area, _)) = self.areas.get("prop-text") {
            let area = area.clone();
            let shown = area.read(cx).value().to_string();
            if shown != value && !area.read(cx).focus_handle(cx).is_focused(window) {
                area.update(cx, |a, cx| a.set_value(value.to_string(), window, cx));
            }
            return area;
        }
        let initial = value.to_string();
        let area = cx.new(|cx| TextareaState::new(window, cx).default_value(initial));
        let subscription = cx.subscribe(&area, |this, area, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let text = area.read(cx).value().to_string();
                this.on_component_edit("prop-text", &text, cx);
            }
        });
        self.areas.insert("prop-text".into(), (area.clone(), subscription));
        area
    }

    pub(super) fn render_component_panel(
        &mut self,
        dashboard: &Page,
        c: &PageComponent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let uses_query = matches!(c.kind, ComponentKind::Kpi | ComponentKind::Table | ComponentKind::Chart);
        self.load_columns(if uses_query { c.query_id.as_deref() } else { None }, dashboard, cx);
        let config = self.shell.read(cx).config(cx).clone();
        let theme = cx.theme().clone();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let mut panel = v_flex()
            .gap_2()
            .child(
                Button::new("dashboard-settings")
                    .ghost()
                    .small()
                    .start_label(None, "Page settings")
                    .on_click(cx.listener(|this, _, _, cx| this.select(None, cx))),
            )
            .child(div().text_xs().text_color(muted).child(format!("{} PROPERTIES", kind_label(c.kind).to_uppercase())))
            .child(self.text_field("prop-title", "Title", &c.title, "", false, window, cx));
        if uses_query {
            let picker = self.picker(
                "prop-query",
                "Query",
                c.query_id.as_deref().unwrap_or(""),
                |this, _, id, cx| this.change("Set component query", move |c| c.query_id = opt(&id), cx),
                cx,
            );
            panel = panel.child(picker);
            if !self.columns.error.is_empty() {
                panel = panel.child(widgets::styled_text(
                    "prop-columns-error",
                    format!("Could not read the query's columns: {}", self.columns.error),
                    move |d| d.text_sm().text_color(danger),
                ));
            }
        }
        let names: Vec<String> = ["rows", "params", "app"].iter().map(|n| n.to_string()).collect();
        match c.kind {
            ComponentKind::Kpi => {
                let comparison = c.comparison.clone().unwrap_or_default();
                let comparison_field = comparison.value_field.clone();
                panel = panel
                    .child(self.column_select("prop-value-field", "Value field", c.value_field.as_deref(), |this, v, cx| {
                        this.change("Edit component", move |c| c.value_field = v, cx)
                    }, cx))
                    .child(self.expression_field(
                        "prop-expression",
                        "Value expression",
                        c.expression.as_deref().unwrap_or(""),
                        "sum(rows.amount)",
                        &names,
                        window,
                        cx,
                    ))
                    .child(self.text_field("prop-format", "Format", c.format.as_deref().unwrap_or(""), "#,##0.00", true, window, cx))
                    .child(
                        v_flex()
                            .gap_2()
                            .p_2()
                            .border_1()
                            .border_color(theme.border)
                            .rounded_md()
                            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Comparison"))
                            .child(self.column_select("prop-comparison-field", "Comparison field", comparison_field.as_deref(), |this, v, cx| {
                                this.change("Edit component", move |c| {
                                    let has_expression = c.comparison.as_ref().is_some_and(|k| k.expression.is_some());
                                    c.comparison = if v.is_some() || has_expression {
                                        let mut k = c.comparison.take().unwrap_or_default();
                                        k.value_field = v;
                                        Some(k)
                                    } else {
                                        None
                                    };
                                }, cx)
                            }, cx))
                            .child(self.expression_field(
                                "prop-comparison-expression",
                                "Comparison expression",
                                comparison.expression.as_deref().unwrap_or(""),
                                "sum(rows.amount)",
                                &names,
                                window,
                                cx,
                            ))
                            .child(self.text_field("prop-comparison-label", "Comparison label", &comparison.label, "target", false, window, cx)),
                    );
            }
            ComponentKind::Chart => {
                let chart_type = c.chart_type.unwrap_or(ChartType::Bar);
                let types = CHART_TYPES.iter().map(|t| (chart_key(*t).to_string(), chart_label(*t).to_string())).collect();
                panel = panel
                    .child(choice_field(
                        "prop-chart-type",
                        "Chart type",
                        chart_key(chart_type),
                        types,
                        "Bar",
                        Self::on_pick(cx, |this, v, cx| {
                            let t = CHART_TYPES.iter().copied().find(|t| chart_key(*t) == v).unwrap_or(ChartType::Bar);
                            this.change("Set chart type", move |c| c.chart_type = Some(t), cx)
                        }),
                        cx,
                    ))
                    .child(self.column_select(
                        "prop-x",
                        if chart_type == ChartType::Scatter { "X field (number)" } else { "X field" },
                        c.x.as_deref(),
                        |this, v, cx| this.change("Edit component", move |c| c.x = v, cx),
                        cx,
                    ))
                    .child(self.column_checks("prop-y", "Value fields", &c.y, |c, y| c.y = y, cx))
                    .child(self.column_select("prop-group-by", "Group by", c.group_by.as_deref(), |this, v, cx| {
                        this.change("Edit component", move |c| c.group_by = v, cx)
                    }, cx));
                if matches!(chart_type, ChartType::Bar | ChartType::Area) {
                    let editor = cx.entity();
                    panel = panel.child(check("prop-stacked", "Stacked", c.stacked, move |on, _, cx| {
                        editor.update(cx, |this, cx| this.change("Edit component", move |c| c.stacked = on, cx))
                    }));
                }
                panel = panel.child(self.text_field("prop-format", "Format", c.format.as_deref().unwrap_or(""), "#,##0", true, window, cx));
            }
            ComponentKind::Table => {
                panel = panel
                    .child(self.column_checks("prop-column", "Columns", &c.columns, |c, v| c.columns = v, cx))
                    .child(self.text_field("prop-page-size", "Page size", &c.page_size.unwrap_or(10).to_string(), "", false, window, cx));
            }
            ComponentKind::Filter => {
                let mut options = vec![(String::new(), "(none)".to_string())];
                options.extend(dashboard.filters.iter().map(|f| (f.id.clone(), if f.name.is_empty() { f.param.clone() } else { f.name.clone() })));
                panel = panel.child(choice_field(
                    "prop-filter",
                    "Filter",
                    c.filter_id.as_deref().unwrap_or(""),
                    options,
                    "(none)",
                    Self::on_pick(cx, |this, v, cx| {
                        let v = opt(v);
                        this.change("Edit component", move |c| c.filter_id = v, cx)
                    }),
                    cx,
                ));
            }
            ComponentKind::Form => {
                let chosen = config.design.forms.iter().find(|f| Some(&f.id) == c.form_id.as_ref());
                let modes = embeddable_modes(chosen);
                // A query-sourced form opens its first row; it has no record key to bind.
                let query_sourced = chosen.is_some_and(|f| !super::embedded::table_sourced(f));
                let mut forms = vec![(String::new(), "(none)".to_string())];
                forms.extend(config.design.forms.iter().map(|f| (f.id.clone(), f.name.clone())));
                panel = panel
                    .child(choice_field(
                        "prop-form",
                        "Form",
                        c.form_id.as_deref().unwrap_or(""),
                        forms,
                        "(none)",
                        Self::on_pick(cx, |this, v, cx| {
                            let form = this.shell.read(cx).config(cx).design.forms.iter().find(|f| f.id == v).cloned();
                            let modes = embeddable_modes(form.as_ref());
                            let clear_record = form.as_ref().is_some_and(|f| !super::embedded::table_sourced(f));
                            let id = opt(v);
                            this.change("Edit component", move |c| {
                                c.form_id = id;
                                c.mode = match c.mode {
                                    Some(m) if modes.contains(&m) => Some(m),
                                    _ => modes.first().copied(),
                                };
                                if clear_record {
                                    c.record_id = None;
                                }
                            }, cx)
                        }),
                        cx,
                    ))
                    .child(choice_field(
                        "prop-form-mode",
                        "Form mode",
                        &c.mode.or_else(|| modes.first().copied()).map(mode_name).unwrap_or_default(),
                        modes.iter().map(|m| (mode_name(*m), mode_name(*m))).collect(),
                        "",
                        Self::on_pick(cx, |this, v, cx| {
                            let mode = parse_mode(v);
                            this.change("Edit component", move |c| c.mode = mode, cx)
                        }),
                        cx,
                    ));
                panel = panel.child(Self::open_button("edit-form", if chosen.is_some() { "Edit form" } else { "New form" }, cx));
                if matches!(c.mode, Some(crate::design::FormMode::Detail | crate::design::FormMode::Edit)) && !query_sourced {
                    panel = panel.child(self.expression_field(
                        "prop-record-id",
                        "Record id",
                        c.record_id.as_deref().unwrap_or(""),
                        "Blank: first row of the form's source",
                        &names,
                        window,
                        cx,
                    ));
                }
            }
            ComponentKind::Report => {
                let mut options = vec![(String::new(), "(none)".to_string())];
                options.extend(config.reports.iter().map(|r| (r.id.clone(), r.name.clone())));
                panel = panel.child(choice_field(
                    "prop-report",
                    "Report",
                    c.report_id.as_deref().unwrap_or(""),
                    options,
                    "(none)",
                    Self::on_pick(cx, |this, v, cx| {
                        let v = opt(v);
                        this.change("Edit component", move |c| c.report_id = v, cx)
                    }),
                    cx,
                ));
                let chosen = config.reports.iter().any(|r| Some(&r.id) == c.report_id.as_ref());
                panel = panel.child(Self::open_button("edit-report", if chosen { "Edit report" } else { "New report" }, cx));
            }
            ComponentKind::Button => {
                let editor = cx.entity();
                panel = panel.child(self.text_field("prop-label", "Button label", c.label.as_deref().unwrap_or(""), "", false, window, cx)).child(
                    action_picker("prop-action", "Action", c.action_id.as_deref(), None, &config.actions, move |v, _, cx| {
                        editor.update(cx, |this, cx| this.change("Edit component", move |c| c.action_id = v, cx))
                    }, cx),
                );
                let mut pages = vec![(String::new(), "(none)".to_string())];
                pages.extend(config.pages.iter().filter(|p| p.id != dashboard.id).map(|p| (p.id.clone(), p.name.clone())));
                panel = panel.child(choice_field(
                    "prop-target-page",
                    "Then open page",
                    c.target_page_id.as_deref().unwrap_or(""),
                    pages,
                    "(none)",
                    Self::on_pick(cx, |this, v, cx| {
                        let v = opt(v);
                        this.change("Edit component", move |c| {
                            if c.target_page_id != v {
                                c.page_params.clear();
                            }
                            c.target_page_id = v;
                        }, cx)
                    }),
                    cx,
                ));
                let target = config.pages.iter().find(|p| Some(&p.id) == c.target_page_id.as_ref()).cloned();
                for param in target.iter().flat_map(|p| p.parameters.iter()) {
                    let label = if param.required { format!("{} (required)", param.name) } else { param.name.clone() };
                    panel = panel.child(self.expression_field(
                        &format!("prop-page-param:{}", param.name),
                        &label,
                        c.page_params.get(&param.name).map(String::as_str).unwrap_or(""),
                        "Expression over params and app",
                        &names,
                        window,
                        cx,
                    ));
                }
            }
            ComponentKind::Text => {
                let area = self.text_area(c.text.as_deref().unwrap_or(""), window, cx);
                panel = panel.child(widgets::field(
                    "Text",
                    div()
                        .id("prop-text")
                        .aria_label("Text")
                        .h(px(110.))
                        .w_full()
                        .rounded_md()
                        .border_1()
                        .border_color(theme.border)
                        .child(Textarea::new(&area).h(px(108.)))
                        .test_support(),
                    cx,
                ));
            }
        }
        panel = panel.child(self.condition_properties(c, window, cx));
        let p = &c.placement;
        panel = panel.child(widgets::styled_text(
            "prop-hint",
            format!(
                "Column {}, row {}, {} × {}. Alt+Arrow resizes, Alt+Shift+Arrow moves.",
                p.column, p.row, p.column_span, p.row_span
            ),
            move |d| d.text_xs().text_color(muted),
        ));
        if let Some(region) = self.region_picker(dashboard, c, cx) {
            panel = panel.child(region);
        }
        panel
            .child(
                Button::new("prop-delete")
                    .danger()
                    .outline()
                    .small()
                    .icon(IconName::Trash)
                    .label("Delete component")
                    .on_click(cx.listener(|this, _, _, cx| this.remove_selected(cx))),
            )
            .into_any_element()
    }

    /// Region choice for the component's placement: "None" keeps the column and row placement.
    fn region_picker(&self, dashboard: &Page, c: &PageComponent, cx: &mut Context<Self>) -> Option<Div> {
        let names: Vec<String> = dashboard.layout.named_regions.iter().map(|r| r.name.clone()).filter(|n| !n.is_empty()).collect();
        let current = c.placement.region.clone().unwrap_or_default();
        if names.is_empty() && current.is_empty() {
            return None;
        }
        let mut options = vec![(String::new(), "None (use column and row)".to_string())];
        if !current.is_empty() && !names.contains(&current) {
            options.push((current.clone(), format!("{current} (missing)")));
        }
        options.extend(names.into_iter().map(|n| (n.clone(), n)));
        Some(choice_field(
            "prop-region",
            "Region",
            &current,
            options,
            "None (use column and row)",
            Self::on_pick(cx, |this, v, cx| {
                let region = opt(v);
                this.change("Place component", move |c| c.placement.region = region, cx)
            }),
            cx,
        ))
    }
}
