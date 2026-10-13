//! The Design tab of a dashboard (port of `DashboardEditor.tsx`): the component palette,
//! the editable grid canvas, the problems list and the properties panel.
//!
//! Test ids: the palette buttons `add-<kind>` (`add-kpi`, `add-chart`, `add-table`,
//! `add-filter-component`, `add-form`, `add-report`, `add-button`, `add-text`), the canvas
//! `dash-canvas` (label "<name> layout") whose items are the component ids (label: the
//! title; see `ui::grid::canvas`), `card-summary:<id>` (the card's second line),
//! `dash-issues` and `issue:<n>`, and `dashboard-settings` (back from a component to the
//! dashboard's settings). The panel is `properties`; its controls are in `props.rs`,
//! `filters.rs` and `settings.rs`.

use super::ROW_HEIGHT;
use crate::{
    dashboard_engine::{
        data::{dashboard_params, default_filter_values},
        model::{
            COMPONENT_KINDS, chart_label, constraints_for, dashboard_issues, kind_key, kind_label, new_component,
        },
    },
    dashboards::{ChartType, ComponentKind, Dashboard, DashboardComponent},
    design::Placement,
    ui::{
        grid::canvas::{GridCanvas, GridItem},
        prelude::*,
        query::{QueryPicker, QueryPickerEvent},
        reports::fields::Inputs,
        shell::{RevealTarget, Shell},
    },
    validation::Severity,
};
use gpui_kit::component::input::TextareaState;
use serde_json::{Map, Value};
use std::collections::HashMap;

/// Result columns of a component's query, found by running it once for one row.
#[derive(Default)]
pub(super) struct Columns {
    pub key: String,
    pub columns: Vec<String>,
    pub error: String,
}

pub struct DashboardEditor {
    pub(super) shell: Entity<Shell>,
    pub(super) dashboard_id: String,
    pub selected: Option<String>,
    pub(super) inputs: Inputs<DashboardEditor, String>,
    pub(super) pickers: HashMap<String, (Entity<QueryPicker>, Subscription)>,
    pub(super) areas: HashMap<String, (Entity<TextareaState>, Subscription)>,
    pub(super) columns: Columns,
}

impl DashboardEditor {
    pub fn new(shell: Entity<Shell>, dashboard_id: &str, focus: Option<String>) -> Self {
        Self {
            shell,
            dashboard_id: dashboard_id.to_string(),
            selected: focus,
            inputs: Inputs::new(Self::on_edit),
            pickers: HashMap::new(),
            areas: HashMap::new(),
            columns: Columns::default(),
        }
    }

    pub fn dashboard(&self, cx: &App) -> Option<Dashboard> {
        self.shell.read(cx).config(cx).dashboards.iter().find(|d| d.id == self.dashboard_id).cloned()
    }

    pub fn selected_component(&self, cx: &App) -> Option<DashboardComponent> {
        let dashboard = self.dashboard(cx)?;
        dashboard.components.into_iter().find(|c| Some(&c.id) == self.selected.as_ref())
    }

    /// Applies `mutate` to the dashboard as one undo step named `label`.
    pub(super) fn edit(&mut self, label: &str, mutate: impl FnOnce(&mut Dashboard) + 'static, cx: &mut Context<Self>) {
        let (id, store) = (self.dashboard_id.clone(), self.shell.read(cx).store.clone());
        let result = store.update(cx, |s, cx| {
            s.update(
                label,
                move |config| {
                    if let Some(dashboard) = config.dashboards.iter_mut().find(|d| d.id == id) {
                        mutate(dashboard);
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

    /// Applies `mutate` to the selected component.
    pub(super) fn change(&mut self, label: &str, mutate: impl FnOnce(&mut DashboardComponent) + 'static, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else { return };
        self.change_component(&id, label, mutate, cx);
    }

    pub(super) fn change_component(
        &mut self,
        id: &str,
        label: &str,
        mutate: impl FnOnce(&mut DashboardComponent) + 'static,
        cx: &mut Context<Self>,
    ) {
        let id = id.to_string();
        self.edit(
            label,
            move |d| {
                if let Some(component) = d.components.iter_mut().find(|c| c.id == id) {
                    mutate(component);
                }
            },
            cx,
        );
    }

    pub fn add(&mut self, kind: ComponentKind, extra: impl FnOnce(&mut DashboardComponent) + 'static, cx: &mut Context<Self>) {
        let Some(dashboard) = self.dashboard(cx) else { return };
        let component = new_component(kind, &dashboard, extra);
        self.selected = Some(component.id.clone());
        let label = format!("Add {}", kind_label(kind));
        self.edit(&label, move |d| d.components.push(component), cx);
    }

    pub fn remove_selected(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected.take() else { return };
        self.edit("Delete component", move |d| d.components.retain(|c| c.id != id), cx);
    }

    pub fn select(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.selected = id;
        cx.notify();
    }

    /// Opens the element another mode asked to reveal (a Problems link).
    pub fn reveal(&mut self, cx: &mut Context<Self>) {
        let request = self.shell.update(cx, |s, _| s.take_reveal(crate::ui::shell::ModeId::Dashboards));
        if let Some(RevealTarget::Dashboards { element_id, .. }) = request {
            self.selected = element_id;
            cx.notify();
        }
    }

    /// A query picker for `key` showing `value`; changes arrive as `on_pick`.
    pub(super) fn picker(
        &mut self,
        key: &str,
        label: &str,
        value: &str,
        on_pick: fn(&mut Self, &str, String, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> Entity<QueryPicker> {
        if let Some((picker, _)) = self.pickers.get(key) {
            picker.update(cx, |p, cx| p.set_value(value, cx));
            return picker.clone();
        }
        let (store, label, value) = (self.shell.read(cx).store.clone(), label.to_string(), value.to_string());
        let id = SharedString::from(key.to_string());
        let picker = cx.new(|cx| QueryPicker::new(id, store, cx).label(label).value(value));
        let owner = key.to_string();
        let subscription = cx.subscribe(&picker, move |this, _, event: &QueryPickerEvent, cx| {
            let QueryPickerEvent::Change(id) = event;
            on_pick(this, &owner, id.clone(), cx);
        });
        self.pickers.insert(key.to_string(), (picker.clone(), subscription));
        picker
    }

    /// Finds the columns of `query_id` when it, its SQL or the parameters changed.
    pub(super) fn load_columns(&mut self, query_id: Option<&str>, dashboard: &Dashboard, cx: &mut Context<Self>) {
        let Some(query_id) = query_id.filter(|q| !q.is_empty()) else {
            self.columns = Columns::default();
            return;
        };
        let config = self.shell.read(cx).config(cx);
        let sql = config.saved_queries.iter().find(|q| q.id == query_id).map(|q| q.sql.clone()).unwrap_or_default();
        let params: Map<String, Value> = dashboard_params(&dashboard.filters, &default_filter_values(&dashboard.filters));
        let key = serde_json::json!([query_id, sql, params]).to_string();
        if self.columns.key == key {
            return;
        }
        self.columns = Columns { key: key.clone(), ..Columns::default() };
        let (label, id) = (self.shell.read(cx).label().to_string(), query_id.to_string());
        task::spawn_call(
            cx,
            move || crate::dashboard_engine::load::run_saved(&label, &id, &params, Some(1), &uuid::Uuid::new_v4().to_string()),
            move |this, result, cx| {
                if this.columns.key != key {
                    return;
                }
                match result {
                    Ok(run) => this.columns.columns = run.columns,
                    Err(e) => this.columns.error = e.message,
                }
                cx.notify();
            },
        );
    }

    fn on_edit(this: &mut Self, key: &String, text: String, cx: &mut Context<Self>) {
        if !this.on_component_edit(key, &text, cx) && !this.on_filter_edit(key, &text, cx) {
            this.on_layout_edit(key, &text, cx);
        }
    }

    fn summary(&self, c: &DashboardComponent, dashboard: &Dashboard, cx: &App) -> String {
        let config = self.shell.read(cx).config(cx);
        let query_name = |id: &Option<String>| match id.as_deref().filter(|i| !i.is_empty()) {
            Some(id) => config.saved_queries.iter().find(|q| q.id == id).map_or("Missing query".to_string(), |q| q.name.clone()),
            None => "No query".to_string(),
        };
        match c.kind {
            ComponentKind::Kpi => {
                let value = c.expression.clone().filter(|e| !e.is_empty()).or(c.value_field.clone().filter(|v| !v.is_empty()));
                format!("{} · {}", query_name(&c.query_id), value.unwrap_or_else(|| "no value".into()))
            }
            ComponentKind::Chart => format!(
                "{} · {} · {} → {}",
                chart_label(c.chart_type.unwrap_or(ChartType::Bar)),
                query_name(&c.query_id),
                c.x.clone().unwrap_or_else(|| "?".into()),
                if c.y.is_empty() { "?".to_string() } else { c.y.join(", ") }
            ),
            ComponentKind::Table => format!("{} · {} rows per page", query_name(&c.query_id), c.page_size.unwrap_or(10)),
            ComponentKind::Filter => dashboard
                .filters
                .iter()
                .find(|f| Some(&f.id) == c.filter_id.as_ref())
                .map_or("No filter".to_string(), |f| f.name.clone()),
            ComponentKind::Form => match c.form_id.as_deref().filter(|f| !f.is_empty()) {
                Some(_) => format!("Form · {}", c.mode.map_or("list".into(), super::mode_name)),
                None => "No form".into(),
            },
            ComponentKind::Report => if c.report_id.as_deref().is_some_and(|r| !r.is_empty()) { "Report preview" } else { "No report" }.into(),
            ComponentKind::Button => match c.action_id.as_deref().filter(|a| !a.is_empty()) {
                Some(_) => format!("Runs an action · “{}”", c.label.clone().unwrap_or_default()),
                None => "No action".into(),
            },
            ComponentKind::Text => {
                let text: String = c.text.clone().unwrap_or_default().chars().take(80).collect();
                if text.is_empty() { "Empty text".into() } else { text }
            }
        }
    }

    fn card(&self, c: &DashboardComponent, dashboard: &Dashboard, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let span = c.placement.row_span.max(1) as f32;
        let min = span * ROW_HEIGHT * 0.6 + (span - 1.0) * dashboard.layout.row_gap as f32;
        let muted = theme.muted_foreground;
        let title = if c.title.is_empty() { kind_label(c.kind).to_string() } else { c.title.clone() };
        v_flex()
            .size_full()
            .min_h(px(min))
            .gap_1()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .child(
                h_flex()
                    .justify_between()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(div().text_xs().text_color(muted).child(kind_label(c.kind))),
            )
            .child(widgets::styled_text(format!("card-summary:{}", c.id), self.summary(c, dashboard, cx), move |d| {
                d.text_xs().text_color(muted)
            }))
            .into_any_element()
    }

    fn render_palette(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        v_flex()
            .id("dash-palette")
            .aria_label("Add component")
            .w(px(180.))
            .flex_none()
            .gap_1()
            .p_3()
            .border_r_1()
            .border_color(theme.border)
            .child(div().text_xs().text_color(muted).child("COMPONENTS"))
            .children(COMPONENT_KINDS.into_iter().map(|kind| {
                let label = if kind == ComponentKind::Kpi { "Add KPI".to_string() } else { format!("Add {}", kind_label(kind).to_lowercase()) };
                let key = match kind {
                    ComponentKind::Filter => "add-filter-component".to_string(),
                    other => format!("add-{}", kind_key(other)),
                };
                Button::new(SharedString::from(key))
                    .outline()
                    .small()
                    .justify_start()
                    .icon(component_icon(kind))
                    .label(label)
                    .on_click(cx.listener(move |this, _, _, cx| this.add(kind, |_| {}, cx)))
            }))
    }

    fn render_canvas(&self, dashboard: &Dashboard, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger, warning) = (theme.muted_foreground, theme.danger, theme.warning);
        let issues = dashboard_issues(dashboard, self.shell.read(cx).config(cx));
        let (select, resize, mv) = (cx.entity(), cx.entity(), cx.entity());
        let canvas = if dashboard.components.is_empty() {
            div()
                .text_sm()
                .text_color(muted)
                .child(widgets::text("dash-hint", "Add components from the palette. They snap to the dashboard grid."))
                .into_any_element()
        } else {
            GridCanvas::new("dash-canvas", dashboard.layout.clone())
                .editable(true)
                .selected(self.selected.clone().map(SharedString::from))
                .label(format!("{} layout", dashboard.name))
                .on_select(move |id, _, cx| {
                    let id = id.to_string();
                    select.update(cx, |this, cx| this.select(Some(id), cx));
                })
                .on_resize(move |id, placement, _, cx| {
                    let id = id.to_string();
                    resize.update(cx, |this, cx| this.place(&id, placement, cx));
                })
                .on_move(move |id, placement, _, cx| {
                    let id = id.to_string();
                    mv.update(cx, |this, cx| this.place(&id, placement, cx));
                })
                .items(dashboard.components.iter().map(|c| {
                    GridItem::new(c.id.clone(), c.placement.clone(), self.card(c, dashboard, cx))
                        .label(if c.title.is_empty() { kind_label(c.kind).to_string() } else { c.title.clone() })
                        .constraints(constraints_for(c.kind))
                }))
                .into_any_element()
        };
        v_flex()
            .id("dash-canvas-pane")
            .flex_1()
            .min_w_0()
            .h_full()
            .gap_3()
            .p_3()
            .overflow_y_scroll()
            .child(canvas)
            .when(!issues.is_empty(), |el| {
                el.child(
                    v_flex()
                        .id("dash-issues")
                        .aria_label("Dashboard problems")
                        .gap_1()
                        .children(issues.iter().enumerate().map(|(i, issue)| {
                            let color = if issue.severity == Severity::Error { danger } else { warning };
                            widgets::styled_text(format!("issue:{i}"), issue.message.clone(), move |d| d.text_sm().text_color(color))
                        }))
                        .test_support(),
                )
            })
    }

    fn place(&mut self, id: &str, placement: Placement, cx: &mut Context<Self>) {
        self.change_component(id, "Move component", move |c| c.placement = placement, cx);
    }
}

fn component_icon(kind: ComponentKind) -> IconName {
    match kind {
        ComponentKind::Kpi => IconName::Gauge,
        ComponentKind::Chart => IconName::ChartColumn,
        ComponentKind::Table => IconName::Table,
        ComponentKind::Filter => IconName::Funnel,
        ComponentKind::Form => IconName::LayoutDashboard,
        ComponentKind::Report => IconName::FileText,
        ComponentKind::Button => IconName::Play,
        ComponentKind::Text => IconName::Menu,
    }
}

impl Render for DashboardEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.reveal(cx);
        let Some(dashboard) = self.dashboard(cx) else {
            return div().into_any_element();
        };
        let selected = self.selected_component(cx);
        if selected.is_none() {
            self.selected = None;
        }
        let panel = match &selected {
            Some(component) => self.render_component_panel(&dashboard, component, window, cx),
            None => self.render_settings_panel(&dashboard, window, cx),
        };
        h_flex()
            .id("dash-editor")
            .aria_label("Dashboard designer")
            .size_full()
            .child(self.render_palette(cx))
            .child(self.render_canvas(&dashboard, cx))
            .child(
                v_flex()
                    .id("properties")
                    .aria_label("Properties")
                    .w(px(320.))
                    .flex_none()
                    .h_full()
                    .gap_2()
                    .p_3()
                    .border_l_1()
                    .border_color(cx.theme().border)
                    .overflow_y_scroll()
                    .child(panel)
                    .test_support(),
            )
            .into_any_element()
    }
}
