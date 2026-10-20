//! The Design tab of a dashboard (port of `PageEditor.tsx`): the component palette,
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
    page_engine::{
        data::{dashboard_params, default_filter_values},
        model::{
            COMPONENT_KINDS, chart_label, constraints_for, dashboard_issues, kind_key, kind_label, new_component,
        },
    },
    pages::{ChartType, ComponentKind, Page, PageComponent},
    design::Placement,
    ui::{
        grid::canvas::{GridCanvas, GridItem},
        prelude::*,
        query::{QueryPicker, QueryPickerEvent},
        controls::Inputs,
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
    pub error: Option<crate::manager::AppError>,
}

pub struct PageEditor {
    pub(super) shell: Entity<Shell>,
    pub(super) dashboard_id: String,
    pub selected: Option<String>,
    pub(super) inputs: Inputs<PageEditor, String>,
    pub(super) pickers: HashMap<String, (Entity<QueryPicker>, Subscription)>,
    pub(super) areas: HashMap<String, (Entity<TextareaState>, Subscription)>,
    pub(super) columns: Columns,
}

impl PageEditor {
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

    pub fn dashboard(&self, cx: &App) -> Option<Page> {
        self.shell.read(cx).config(cx).pages.iter().find(|d| d.id == self.dashboard_id).cloned()
    }

    pub fn selected_component(&self, cx: &App) -> Option<PageComponent> {
        let dashboard = self.dashboard(cx)?;
        dashboard.components.into_iter().find(|c| Some(&c.id) == self.selected.as_ref())
    }

    /// Applies `mutate` to the dashboard as one undo step named `label`.
    pub(super) fn edit(&mut self, label: &str, mutate: impl FnOnce(&mut Page) + 'static, cx: &mut Context<Self>) {
        let (id, store) = (self.dashboard_id.clone(), self.shell.read(cx).store.clone());
        let result = store.update(cx, |s, cx| {
            s.update(
                label,
                move |config| {
                    if let Some(dashboard) = config.pages.iter_mut().find(|d| d.id == id) {
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
    pub(super) fn change(&mut self, label: &str, mutate: impl FnOnce(&mut PageComponent) + 'static, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else { return };
        self.change_component(&id, label, mutate, cx);
    }

    pub(super) fn change_component(
        &mut self,
        id: &str,
        label: &str,
        mutate: impl FnOnce(&mut PageComponent) + 'static,
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

    pub fn add(&mut self, kind: ComponentKind, extra: impl FnOnce(&mut PageComponent) + 'static, cx: &mut Context<Self>) {
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

    /// Opens the selected form or report component's definition in its designer. A component
    /// without one gets a new form or report first, in the same undo step.
    pub fn open_target(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(component) = self.selected_component(cx) else { return };
        let config = self.shell.read(cx).config(cx);
        let existing = match component.kind {
            ComponentKind::Form => component.form_id.clone().filter(|id| config.design.forms.iter().any(|f| &f.id == id)),
            ComponentKind::Report => component.report_id.clone().filter(|id| config.reports.iter().any(|r| &r.id == id)),
            _ => return,
        };
        let target = match existing {
            Some(id) => id,
            None => self.create_target(&component, cx),
        };
        let reveal = if component.kind == ComponentKind::Form {
            RevealTarget::Design { object_id: target, element_id: None }
        } else {
            RevealTarget::Reports { object_id: target, element_id: None }
        };
        self.shell.update(cx, |s, cx| s.request_reveal(reveal, window, cx));
    }

    /// Adds a form or report named after the page and points `component` at it.
    fn create_target(&mut self, component: &PageComponent, cx: &mut Context<Self>) -> String {
        let config = self.shell.read(cx).config(cx);
        let page = self.dashboard(cx).map(|p| p.name).unwrap_or_default();
        let (page_id, cid) = (self.dashboard_id.clone(), component.id.clone());
        if component.kind == ComponentKind::Form {
            let taken: Vec<String> = config.design.forms.iter().map(|f| f.name.clone()).collect();
            let form = crate::forms::schema::new_form(&unique(&format!("{page} form"), &taken), None);
            let (id, mode) = (form.id.clone(), form.modes.first().copied());
            self.edit_config("New form", move |c| {
                c.design.forms.push(form);
                if let Some(target) = find_component(c, &page_id, &cid) {
                    target.form_id = Some(id);
                    target.mode = mode;
                }
            }, cx);
            return self.selected_component(cx).and_then(|c| c.form_id).unwrap_or_default();
        }
        let taken: Vec<String> = config.reports.iter().map(|r| r.name.clone()).collect();
        let report = crate::report_engine::model::new_report(&unique(&format!("{page} report"), &taken));
        let id = report.id.clone();
        self.edit_config("New report", move |c| {
            c.reports.push(report);
            if let Some(target) = find_component(c, &page_id, &cid) {
                target.report_id = Some(id);
            }
        }, cx);
        self.selected_component(cx).and_then(|c| c.report_id).unwrap_or_default()
    }

    fn edit_config(&mut self, label: &str, mutate: impl FnOnce(&mut crate::archive::DocumentConfig) + 'static, cx: &mut Context<Self>) {
        let store = self.shell.read(cx).store.clone();
        if let Err(e) = store.update(cx, |s, cx| s.update(label, mutate, cx)) {
            self.shell.update(cx, |s, cx| s.set_error(Some(e), cx));
        }
        cx.notify();
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
    pub(super) fn load_columns(&mut self, query_id: Option<&str>, dashboard: &Page, cx: &mut Context<Self>) {
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
            move || crate::page_engine::load::run_saved(&label, &id, &params, Some(1), &uuid::Uuid::new_v4().to_string()),
            move |this, result, cx| {
                if this.columns.key != key {
                    return;
                }
                match result {
                    Ok(run) => this.columns.columns = run.columns,
                    Err(e) => this.columns.error = Some(e),
                }
                cx.notify();
            },
        );
    }

    fn on_edit(this: &mut Self, key: &String, text: String, cx: &mut Context<Self>) {
        if !this.on_component_edit(key, &text, cx) && !this.on_filter_edit(key, &text, cx) && !this.on_param_edit(key, &text, cx) {
            this.on_layout_edit(key, &text, cx);
        }
    }

    fn summary(&self, c: &PageComponent, dashboard: &Page, cx: &App) -> String {
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
            ComponentKind::Button => {
                let opens = c.target_page_id.as_deref().filter(|t| !t.is_empty()).map(|t| {
                    config.pages.iter().find(|p| p.id == t).map_or("a missing page".to_string(), |p| p.name.clone())
                });
                match (c.action_id.as_deref().filter(|a| !a.is_empty()), opens) {
                    (Some(_), Some(page)) => format!("Runs an action, then opens {page}"),
                    (Some(_), None) => format!("Runs an action · “{}”", c.label.clone().unwrap_or_default()),
                    (None, Some(page)) => format!("Opens {page}"),
                    (None, None) => "No action".into(),
                }
            }
            ComponentKind::Text => {
                let text: String = c.text.clone().unwrap_or_default().chars().take(80).collect();
                if text.is_empty() { "Empty text".into() } else { text }
            }
        }
    }

    fn card(&self, c: &PageComponent, dashboard: &Page, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let span = c.placement.row_span.max(1) as f32;
        let min = span * ROW_HEIGHT * 0.6 + (span - 1.0) * dashboard.layout.row_gap as f32;
        let muted = theme.muted_foreground;
        let title = if c.title.is_empty() { kind_label(c.kind).to_string() } else { c.title.clone() };
        v_flex()
            .size_full()
            .min_h(px(min))
            .overflow_hidden()
            .gap_1()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .child(
                // Title and type tag on separate rows; both truncate instead of wrapping.
                v_flex()
                    .min_w_0()
                    .child(div().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(div().min_w_0().truncate().text_xs().text_color(muted).child(kind_label(c.kind))),
            )
            .child(widgets::styled_text(format!("card-summary:{}", c.id), self.summary(c, dashboard, cx), move |d| {
                d.min_w_0().truncate().text_xs().text_color(muted)
            }))
            .into_any_element()
    }

    fn render_palette(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .id("dash-palette")
            .aria_label("Add component")
            .w(px(180.))
            .flex_none()
            .gap_1()
            .p_3()
            .border_r_1()
            .border_color(theme.border)
            .child(widgets::styled_text("components-heading", "Components", |d| d.text_base().font_weight(FontWeight::SEMIBOLD)))
            .children(COMPONENT_KINDS.into_iter().map(|kind| {
                let label = if kind == ComponentKind::Kpi { "Add KPI".to_string() } else { format!("Add {}", kind_label(kind).to_lowercase()) };
                let label = if kind == ComponentKind::Button { "Add button".to_string() } else { label };
                let key = match kind {
                    ComponentKind::Filter => "add-filter-component".to_string(),
                    other => format!("add-{}", kind_key(other)),
                };
                Button::new(SharedString::from(key))
                    .outline()
                    .small()
                    .start_label(Some(component_icon(kind).into()), label)
                    .on_click(cx.listener(move |this, _, _, cx| this.add(kind, |_| {}, cx)))
            }))
    }

    fn render_canvas(&self, dashboard: &Page, cx: &mut Context<Self>) -> impl IntoElement {
        let issues = dashboard_issues(dashboard, self.shell.read(cx).config(cx));
        let (select, resize, mv) = (cx.entity(), cx.entity(), cx.entity());
        let canvas = if dashboard.components.is_empty() {
            widgets::empty_state(
                "dash-hint",
                "This page is empty",
                "Add components from the list on the left. They snap to the page grid.",
                cx,
            )
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
                        .aria_label("Page problems")
                        .gap_1()
                        .children(issues.iter().enumerate().map(|(i, issue)| {
                            let tone = if issue.severity == Severity::Error { widgets::Severity::Danger } else { widgets::Severity::Warning };
                            widgets::callout(format!("issue:{i}"), tone, &issue.message, None, cx)
                        }))
                        .test_support(),
                )
            })
    }

    fn place(&mut self, id: &str, placement: Placement, cx: &mut Context<Self>) {
        self.change_component(id, "Move component", move |c| c.placement = placement, cx);
    }
}

pub(super) fn component_icon(kind: ComponentKind) -> IconName {
    match kind {
        ComponentKind::Kpi => IconName::Gauge,
        ComponentKind::Chart => IconName::ChartColumn,
        ComponentKind::Table => IconName::Table2,
        ComponentKind::Filter => IconName::Funnel,
        ComponentKind::Form => IconName::LayoutDashboard,
        ComponentKind::Report => IconName::FileText,
        ComponentKind::Button => IconName::Play,
        ComponentKind::Text => IconName::Menu,
    }
}

fn find_component<'a>(config: &'a mut crate::archive::DocumentConfig, page: &str, id: &str) -> Option<&'a mut PageComponent> {
    config.pages.iter_mut().find(|p| p.id == page)?.components.iter_mut().find(|c| c.id == id)
}

/// `base`, or `base 2`, `base 3`, ... the first not taken.
fn unique(base: &str, taken: &[String]) -> String {
    let base = base.trim();
    std::iter::once(base.to_string())
        .chain((2..).map(|n| format!("{base} {n}")))
        .find(|name| !taken.contains(name))
        .unwrap_or_default()
}

impl Render for PageEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .aria_label("Page designer")
            .size_full()
            // Keep the palette at the top instead of centred beside a tall canvas.
            .items_start()
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
