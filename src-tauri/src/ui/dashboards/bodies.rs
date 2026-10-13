//! The bodies of the live dashboard's components (port of `widgets.tsx`): each reuses an
//! existing primitive (result rows, expressions, actions).
//!
//! Test ids (`<name>` is the component's title, or its kind): `gate:<name>` (a loading,
//! error or choose-a-query message), `kpi-value:<name>`, `kpi-compare:<name>`,
//! `kpi-error:<name>`, `table-error:<name>`, `col:<name>:<c>`, `cell:<name>:<r>:<c>` (label:
//! the cell's text), `table-prev:<name>` / `table-next:<name>` with `table-page:<name>`
//! ("Page 1 of 3"), `filter:<label>` (a choice, an input, or for a date range the group with
//! `filter:<label>:from` / `filter:<label>:to`), `dash-button:<label>` and `text:<name>:<n>`.

use super::{chart, view::DashboardView};
use crate::{
    dashboard_engine::data::{kpi_value, result_rows, table_slice, text_paragraphs},
    dashboards::{DashboardComponent, DashboardFilter, FilterControl},
    forms::rbac::Operation,
    ui::{
        dashboards::data::QueryState,
        prelude::*,
        controls::{choice, input},
        runtime::message,
    },
};
use serde_json::{Map, Value};

impl DashboardView {
    /// The rows of a query-backed component, or the message that stands in for them.
    fn gate(
        &self,
        component: &DashboardComponent,
        name: &str,
        state: Option<QueryState>,
        cx: &App,
    ) -> Result<std::sync::Arc<crate::queries::QueryRun>, AnyElement> {
        let say = |text: &str, error: bool| message(format!("gate:{name}"), text.to_string(), error, cx).into_any_element();
        if component.query_id.as_deref().is_none_or(str::is_empty) {
            return Err(say("Choose a saved query for this component.", false));
        }
        match state {
            Some(QueryState::Denied) => Err(say("You do not have access to this data.", true)),
            Some(QueryState::Ready(run)) | Some(QueryState::Loading(Some(run))) => Ok(run),
            Some(QueryState::Error(text)) => Err(say(&text, true)),
            Some(QueryState::Cancelled) => Err(say("Query cancelled.", false)),
            Some(QueryState::Loading(None)) | None => Err(say("Loading…", false)),
        }
    }

    pub(super) fn kpi_body(
        &mut self,
        component: &DashboardComponent,
        name: &str,
        state: Option<QueryState>,
        scope: &Map<String, Value>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let run = match self.gate(component, name, state, cx) {
            Ok(run) => run,
            Err(element) => return element,
        };
        let rows = result_rows(&run.columns, &run.rows);
        let theme = cx.theme();
        match kpi_value(component, &rows, scope) {
            Err(text) => message(format!("kpi-error:{name}"), text, true, cx).into_any_element(),
            Ok(kpi) => {
                let (muted, success, danger) = (theme.muted_foreground, theme.success, theme.danger);
                v_flex()
                    .gap_1()
                    .child(widgets::styled_text(format!("kpi-value:{name}"), kpi.text.clone(), |d| {
                        d.text_3xl().font_weight(FontWeight::SEMIBOLD)
                    }))
                    .children(kpi.comparison.map(|c| {
                        let up = c.delta.unwrap_or(0.0) >= 0.0;
                        let arrow = match c.delta {
                            None => "",
                            Some(_) if up => "▲ ",
                            Some(_) => "▼ ",
                        };
                        let color = match c.delta {
                            None => muted,
                            Some(_) if up => success,
                            Some(_) => danger,
                        };
                        widgets::styled_text(format!("kpi-compare:{name}"), format!("{arrow}{} vs {}", c.text, c.label), move |d| {
                            d.text_sm().text_color(color)
                        })
                    }))
                    .into_any_element()
            }
        }
    }

    pub(super) fn chart_widget(
        &mut self,
        component: &DashboardComponent,
        name: &str,
        state: Option<QueryState>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let run = match self.gate(component, name, state, cx) {
            Ok(run) => run,
            Err(element) => return element,
        };
        let rows = result_rows(&run.columns, &run.rows);
        let figure = chart::chart_figure(component, &rows);
        chart::chart_body(&figure, cx)
    }

    pub(super) fn table_body(
        &mut self,
        component: &DashboardComponent,
        name: &str,
        state: Option<QueryState>,
        scope: &Map<String, Value>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let run = match self.gate(component, name, state, cx) {
            Ok(run) => run,
            Err(element) => return element,
        };
        let page = self.pages.get(&component.id).copied().unwrap_or(0);
        let slice = match table_slice(component, &run.columns, &run.rows, scope, page) {
            Ok(slice) => slice,
            Err(text) => return message(format!("table-error:{name}"), text, true, cx).into_any_element(),
        };
        let theme = cx.theme().clone();
        let border = theme.border;
        // Each column is as wide as its longest text (within bounds); longer text truncates.
        let widths: Vec<f32> = (0..slice.columns.len())
            .map(|c| {
                let longest = slice.rows.iter().filter_map(|r| r.get(c)).map(|t| t.chars().count()).chain([slice.columns[c].chars().count()]).max().unwrap_or(0);
                (longest as f32 * 8.0 + 28.).clamp(56., 260.)
            })
            .collect();
        let header = h_flex().border_b_1().border_color(border).children(slice.columns.iter().enumerate().map(|(c, column)| {
            widgets::styled_text(format!("col:{name}:{c}"), column.clone(), { let w = widths[c]; move |d| {
                d.flex_none().w(px(w)).px_2().py_1().text_sm().font_weight(FontWeight::SEMIBOLD).whitespace_nowrap().truncate()
            } })
        }));
        let rows = slice.rows.iter().enumerate().map(|(r, row)| {
            h_flex().border_b_1().border_color(border).children(row.iter().enumerate().map(|(c, text)| {
                let color = slice.tones[r][c].as_ref().and_then(|t| widgets::tone_color(t, cx));
                widgets::styled_text(format!("cell:{name}:{r}:{c}"), text.clone(), { let w = widths[c]; move |d| {
                    d.flex_none().w(px(w)).px_2().py_1().text_sm().whitespace_nowrap().truncate().when_some(color, |d, color| d.text_color(color))
                } })
            }))
        });
        let id = component.id.clone();
        let (current, pages) = (slice.page, slice.pages);
        let pager = (pages > 1).then(|| {
            let (prev_id, next_id) = (id.clone(), id.clone());
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    Button::new(SharedString::from(format!("table-prev:{name}")))
                        .outline()
                        .small()
                        .label("Previous")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.pages.insert(prev_id.clone(), current.saturating_sub(1));
                            cx.notify();
                        })),
                )
                .child(widgets::text(format!("table-page:{name}"), format!("Page {} of {}", current + 1, pages)))
                .child(
                    Button::new(SharedString::from(format!("table-next:{name}")))
                        .outline()
                        .small()
                        .label("Next")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.pages.insert(next_id.clone(), current + 1);
                            cx.notify();
                        })),
                )
        });
        // Columns keep a minimum width; a wider table scrolls inside its card.
        let min_width = px(widths.iter().sum::<f32>());
        let table = v_flex().w_full().min_w(min_width).child(header).children(rows);
        let scroller = div()
            .id(SharedString::from(format!("table-scroll:{name}")))
            .w_full()
            .overflow_x_scroll()
            .child(table);
        v_flex().gap_2().w_full().min_w_0().child(scroller).children(pager).into_any_element()
    }

    pub(super) fn button_body(&mut self, component: &DashboardComponent, name: &str, cx: &mut Context<Self>) -> AnyElement {
        let action = component.action_id.clone().filter(|a| !a.is_empty());
        let allowed = action.as_ref().is_none_or(|a| self.env.can("action", a, Operation::Execute, cx));
        let label = component.label.clone().filter(|l| !l.is_empty()).unwrap_or_else(|| if component.title.is_empty() { "Run".into() } else { component.title.clone() });
        let _ = name;
        Button::new(SharedString::from(format!("dash-button:{label}")))
            .primary()
            .label(label)
            .disabled(action.is_none() || !allowed)
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(action) = action.clone() {
                    this.run_button(action, cx);
                }
            }))
            .into_any_element()
    }

    pub(super) fn text_body(&self, component: &DashboardComponent, name: &str) -> AnyElement {
        v_flex()
            .gap_2()
            .children(text_paragraphs(component.text.as_deref().unwrap_or("")).into_iter().enumerate().map(|(i, p)| {
                widgets::styled_text(format!("text:{name}:{i}"), p, |d| d.text_sm())
            }))
            .into_any_element()
    }

    /// One dashboard filter as a labelled control. Blank means "all" (the query default).
    pub(super) fn filter_input(&mut self, filter: &DashboardFilter, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let label = if filter.name.is_empty() { filter.param.clone() } else { filter.name.clone() };
        let value = self.values.get(&filter.id).cloned().unwrap_or(Value::Null);
        if filter.control == FilterControl::DateRange {
            let range = value.as_object().cloned().unwrap_or_default();
            let side = |key: &str| range.get(key).and_then(Value::as_str).unwrap_or("").to_string();
            let (from, to) = (side("from"), side("to"));
            let from = self.inputs.get(format!("{}:from", filter.id), &from, "YYYY-MM-DD", window, cx);
            let to = self.inputs.get(format!("{}:to", filter.id), &to, "YYYY-MM-DD", window, cx);
            return v_flex()
                .id(SharedString::from(format!("filter:{label}")))
                .aria_label(label.clone())
                .gap_1()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(label.clone()))
                .child(
                    h_flex()
                        .gap_2()
                        .child(widgets::field("From", input(&from, format!("filter:{label}:from"), "From", false), cx))
                        .child(widgets::field("To", input(&to, format!("filter:{label}:to"), "To", false), cx)),
                )
                .test_support()
                .into_any_element();
        }
        let text = match &value {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let id = SharedString::from(format!("filter:{label}"));
        let control = if filter.control == FilterControl::Select {
            let loaded = filter.options_query_id.as_ref().and_then(|q| self.options.get(q)).cloned().unwrap_or_default();
            let values = if filter.options.is_empty() { loaded } else { filter.options.clone() };
            let mut options = vec![(String::new(), "All".to_string())];
            options.extend(values.into_iter().map(|v| (v.clone(), v)));
            let (this, filter_id) = (cx.weak_entity(), filter.id.clone());
            choice(id, &text, options, "All", move |picked, _, cx| {
                let value = Value::String(picked.to_string());
                let (filter_id, this) = (filter_id.clone(), this.clone());
                let _ = this.update(cx, |t, cx| t.set_filter(&filter_id, value, cx));
            })
            .into_any_element()
        } else {
            let placeholder = if filter.control == FilterControl::Date { "YYYY-MM-DD" } else { "" };
            let state = self.inputs.get(filter.id.clone(), &text, placeholder, window, cx);
            input(&state, id, &label, false).into_any_element()
        };
        widgets::field(&label, control, cx).into_any_element()
    }
}
