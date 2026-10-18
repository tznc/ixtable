//! A live dashboard (port of `PageView.tsx`): the filter bar, the components on the
//! shared grid, and parallel query loading with progress and Cancel. Used by the Studio's
//! View tab (detached) and by Runtime navigation (attached to the `Runtime`).
//!
//! Test ids: `dash-view`, `dash-refresh`, `dash-progress` (label "Loading page data…
//! Ns (X of Y queries done)"), `dash-progress-bar`, `dash-cancel`, `dash-notice`,
//! `dash-missing`, `dash-empty`; each component is `widget:<title>` (its title, or its kind
//! when it has none) with the lines `widget-problem:<title>:visible|enabled` and
//! `widget-disabled:<title>`. The bodies' ids are in `bodies.rs`.

use super::{
    data::{Loader, QueryRunner, QueryState},
    embedded::EmbeddedForm,
    ROW_HEIGHT,
};
use crate::{
    page_engine::{
        data::{dashboard_params, default_filter_values},
        model::kind_label,
    },
    pages::{ComponentKind, Page, PageComponent, PageFilter},
    forms::{
        actions::{NavigationTarget, NoticeTone, StateScope},
        navigation::{PageKind, RuntimePage},
        rbac::{can_read_dashboard_query, Operation},
        state::{condition_result, ConditionResult},
    },
    ui::{
        grid::canvas::{GridCanvas, GridItem},
        prelude::*,
        controls::Inputs,
        reports::ReportPreview,
        runtime::{
            back::back_crumb,
            host::{context, run_action_job, HostEvent, UiHost},
            message, BackLink, RuntimeEnv,
        },
        shell::Shell,
        toast::Toast,
        widgets::Severity,
    },
};
use serde_json::{Map, Value};
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

const DEBOUNCE_MS: u64 = 250;

/// Whether a component is on screen and enabled, and why a condition failed.
#[derive(Debug, Clone, PartialEq)]
pub struct WidgetState {
    pub title: String,
    pub shown: bool,
    pub enabled: bool,
    pub visible_error: Option<String>,
    pub enabled_error: Option<String>,
}

pub struct PageView {
    pub(super) shell: Entity<Shell>,
    pub(super) env: RuntimeEnv,
    dashboard_id: String,
    // Page parameters (a navigate action's `params`); filter values override the same names.
    initial: Map<String, Value>,
    back: Option<BackLink>,
    // Control values and the values the queries last ran for, by filter id.
    pub values: Map<String, Value>,
    settled: Map<String, Value>,
    debounce: u64,
    pub loader: Loader,
    pub(super) pages: HashMap<String, usize>,
    /// The last button error, shown above the grid until the next run.
    pub notice: Option<String>,
    pub(super) options: HashMap<String, Vec<String>>,
    options_asked: HashSet<String>,
    pub(super) inputs: Inputs<PageView, String>,
    forms: HashMap<String, Entity<EmbeddedForm>>,
    reports: HashMap<String, Entity<ReportPreview>>,
    _subscription: Subscription,
}

impl PageView {
    pub fn new(
        shell: Entity<Shell>,
        env: RuntimeEnv,
        dashboard_id: &str,
        params: Map<String, Value>,
        back: Option<BackLink>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe(&env.bus.clone(), |this, _, _: &crate::ui::events::DocEvent, cx| {
            this.loader.refresh();
            cx.notify();
        });
        let mut view = Self {
            shell,
            env,
            dashboard_id: dashboard_id.to_string(),
            initial: params.clone(),
            back,
            values: Map::new(),
            settled: Map::new(),
            debounce: 0,
            loader: Loader::new(),
            pages: HashMap::new(),
            notice: None,
            options: HashMap::new(),
            options_asked: HashSet::new(),
            inputs: Inputs::new(Self::on_edit),
            forms: HashMap::new(),
            reports: HashMap::new(),
            _subscription: subscription,
        };
        if let Some(dashboard) = view.dashboard(cx) {
            view.initial = crate::page_engine::links::with_defaults(&dashboard, params);
            view.values = default_filter_values(&dashboard.filters);
            for filter in &dashboard.filters {
                if let Some(value) = view.initial.get(&filter.param) {
                    view.values.insert(filter.id.clone(), value.clone());
                }
            }
            view.settled = view.values.clone();
        }
        view
    }

    pub(super) fn dashboard(&self, cx: &App) -> Option<Page> {
        self.env.config(cx).pages.iter().find(|d| d.id == self.dashboard_id).cloned()
    }

    /// The query parameters in force: page parameters, overridden by the filters' settled values.
    pub fn params(&self, dashboard: &Page) -> Map<String, Value> {
        let mut values = self.settled.clone();
        for (id, value) in default_filter_values(&dashboard.filters) {
            values.entry(id).or_insert(value);
        }
        let mut params = self.initial.clone();
        params.extend(dashboard_params(&dashboard.filters, &values));
        params
    }

    fn scope(&self, params: &Map<String, Value>, cx: &App) -> Map<String, Value> {
        let mut scope = Map::new();
        scope.insert("params".into(), Value::Object(params.clone()));
        scope.insert("app".into(), Value::Object(self.env.app(cx)));
        scope
    }

    /// Changes a filter's control value; queries re-run once it has settled for 250 ms.
    pub fn set_filter(&mut self, id: &str, value: Value, cx: &mut Context<Self>) {
        self.values.insert(id.to_string(), value);
        self.debounce += 1;
        let seq = self.debounce;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(DEBOUNCE_MS)).await;
            let _ = this.update(cx, |this, cx| {
                if this.debounce == seq {
                    this.settled = this.values.clone();
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn on_edit(this: &mut Self, key: &String, text: String, cx: &mut Context<Self>) {
        // Keys are `<filter id>` or `<filter id>:from|to` (date ranges).
        let (id, side) = match key.rsplit_once(':') {
            Some((id, side)) if side == "from" || side == "to" => (id.to_string(), Some(side.to_string())),
            _ => (key.clone(), None),
        };
        let value = match side {
            Some(side) => {
                let mut range = this.values.get(&id).and_then(|v| v.as_object().cloned()).unwrap_or_default();
                range.insert(side, Value::String(text));
                Value::Object(range)
            }
            None => Value::String(text),
        };
        if this.values.get(&id) != Some(&value) {
            this.set_filter(&id, value, cx);
        }
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.loader.refresh();
        cx.notify();
    }

    pub fn cancel_queries(&mut self, cx: &mut Context<Self>) {
        let runner = QueryRunner::current(cx);
        let label = self.env.label.to_string();
        self.loader.cancel(&label, &runner);
    }

    /// A note from a dashboard button: a toast for news, an inline error callout above
    /// the grid for a failure (design-language.md, "Toast").
    pub(super) fn say(&mut self, text: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        let text = text.into();
        if error {
            self.notice = Some(text);
        } else {
            self.notice = None;
            self.shell.update(cx, |shell, cx| shell.toast(Toast::info(text), cx));
        }
        cx.notify();
    }

    fn on_host_event(&mut self, event: HostEvent, cx: &mut Context<Self>) {
        match event {
            HostEvent::Navigate(target) => self.navigate(target, cx),
            HostEvent::SetState(StateScope::App, key, value) => {
                let env = self.env.clone();
                cx.defer(move |cx| env.set_app_state(&key, value, cx));
            }
            // There is no form on a dashboard to hold the state.
            HostEvent::SetState(StateScope::Form, ..) => self.say("A page button cannot set form state.", true, cx),
            HostEvent::Notify(text, tone) => self.say(text, tone == NoticeTone::Error, cx),
            HostEvent::Refresh => self.refresh(cx),
            _ => {}
        }
    }

    fn navigate(&mut self, target: NavigationTarget, cx: &mut Context<Self>) {
        let runtime = self.env.runtime.clone();
        let kind = PageKind::parse(&target.kind);
        match (runtime, kind) {
            (Some(runtime), Some(kind)) => {
                let page = RuntimePage {
                    kind,
                    id: target.id,
                    mode: target.mode,
                    record_id: target.record_id,
                    params: target.params,
                    nav_id: None,
                };
                cx.defer(move |cx| {
                    runtime.update(cx, |r, cx| r.navigate(page, cx));
                });
            }
            _ => {
                let text = format!("This action opens a {}. Open it in the Runtime to follow navigation.", target.kind);
                self.say(text, false, cx);
            }
        }
    }

    /// Opens the page a button links to, with its parameter values evaluated over the
    /// page's `params` and `app`.
    pub(super) fn open_linked_page(&mut self, component: &PageComponent, cx: &mut Context<Self>) {
        let Some(target) = component.target_page_id.clone().filter(|t| !t.is_empty()) else { return };
        let Some(dashboard) = self.dashboard(cx) else { return };
        let scope = self.scope(&self.params(&dashboard), cx);
        let params = crate::page_engine::links::evaluate_params(&component.page_params, &scope);
        self.navigate(NavigationTarget { kind: "page".into(), id: target, mode: None, record_id: None, params }, cx);
    }

    pub(super) fn run_button(&mut self, action_id: String, link: Option<PageComponent>, cx: &mut Context<Self>) {
        let Some(dashboard) = self.dashboard(cx) else { return };
        let params = self.params(&dashboard);
        // Buttons run through the Automation runner unless a runner is set.
        crate::ui::runtime::automation_bridge::ensure_runner();
        let host = UiHost::new(self.env.config(cx).clone(), self.env.role_id(cx), self.env.attached());
        let (config, label, app) = (self.env.config(cx).clone(), self.env.label.to_string(), self.env.app(cx));
        run_action_job(
            cx,
            host,
            self.env.runtime.clone(),
            move |host| {
                let mut ctx = context(config, &label, host, None, Map::new(), app);
                ctx.params = Some(params);
                crate::forms::actions::run_action(&action_id, &ctx)
            },
            Self::on_host_event,
            move |this, result, cx| {
                if !result.ok {
                    if let Some(error) = result.error.filter(|e| !e.is_empty()) {
                        this.say(error, true, cx);
                    }
                } else if let Some(link) = &link {
                    this.open_linked_page(link, cx);
                }
            },
        );
    }

    /// The embedded form of component `id`, once it was built.
    pub fn embedded(&self, id: &str) -> Option<Entity<EmbeddedForm>> {
        self.forms.get(id).cloned()
    }

    /// The state of a component's query: `Denied` when the role may not read it.
    pub fn component_state(&self, dashboard: &Page, component: &PageComponent, cx: &App) -> Option<QueryState> {
        let readable = |query: &str| self.env_can_read(dashboard, query, cx);
        self.state_of(component, &readable)
    }

    fn env_can_read(&self, dashboard: &Page, query: &str, cx: &App) -> bool {
        can_read_dashboard_query(self.env.config(cx), self.env.role_id(cx).as_deref(), dashboard, query)
    }

    fn state_of(&self, component: &PageComponent, readable: &dyn Fn(&str) -> bool) -> Option<QueryState> {
        let id = component.query_id.as_deref().filter(|q| !q.is_empty())?;
        if !readable(id) {
            return Some(QueryState::Denied);
        }
        self.loader.states.get(id).cloned()
    }

    /// The saved queries a load runs: the ones the components use that the role may read,
    /// each once.
    pub fn runnable_queries(&self, dashboard: &Page, cx: &App) -> Vec<String> {
        let mut ids: Vec<String> = vec![];
        for c in &dashboard.components {
            let id = c.query_id.clone().unwrap_or_default();
            let uses_query = matches!(c.kind, ComponentKind::Kpi | ComponentKind::Table | ComponentKind::Chart);
            if uses_query && !id.is_empty() && !ids.contains(&id) && self.env_can_read(dashboard, &id, cx) {
                ids.push(id);
            }
        }
        ids
    }

    /// Which components are on screen and enabled for the current parameters.
    pub fn widgets(&self, cx: &App) -> Vec<WidgetState> {
        let Some(dashboard) = self.dashboard(cx) else { return vec![] };
        let scope = self.scope(&self.params(&dashboard), cx);
        dashboard
            .components
            .iter()
            .map(|c| {
                let visible = condition_result(c.visible_when.as_deref(), &scope, true);
                let enabled = condition_result(c.enabled_when.as_deref(), &scope, true);
                WidgetState {
                    title: if c.title.is_empty() { kind_label(c.kind).to_string() } else { c.title.clone() },
                    shown: visible.value || visible.error.is_some(),
                    enabled: enabled.value,
                    visible_error: visible.error,
                    enabled_error: enabled.error,
                }
            })
            .collect()
    }

    /// Loads the choices of select filters that take them from a query.
    fn load_options(&mut self, dashboard: &Page, cx: &mut Context<Self>) {
        for filter in &dashboard.filters {
            let Some(query) = filter.options_query_id.clone().filter(|_| filter.options.is_empty()) else { continue };
            if !self.options_asked.insert(query.clone()) {
                continue;
            }
            let (runner, label) = (QueryRunner::current(cx), self.env.label.to_string());
            let id = query.clone();
            task::spawn_call(
                cx,
                move || {
                    let run_id = uuid::Uuid::new_v4().to_string();
                    (runner.run)(&label, &id, &Map::new(), &run_id).map(|r| crate::page_engine::data::option_values(&r.rows)).unwrap_or_default()
                },
                move |this, values, cx| {
                    this.options.insert(query, values);
                    cx.notify();
                },
            );
        }
    }

    fn form_for(&mut self, component: &PageComponent, params: &Map<String, Value>, cx: &mut Context<Self>) -> Entity<EmbeddedForm> {
        if let Some(form) = self.forms.get(&component.id).cloned() {
            form.update(cx, |f, cx| f.set_component(component, params.clone(), cx));
            return form;
        }
        let (env, component, params) = (self.env.clone(), component.clone(), params.clone());
        let form = cx.new(|cx| EmbeddedForm::new(env, &component, params, cx));
        self.forms.insert(component.id.clone(), form.clone());
        form
    }

    fn report_for(&mut self, component: &PageComponent, params: &Map<String, Value>, cx: &mut Context<Self>) -> Entity<ReportPreview> {
        let report_id = component.report_id.clone().unwrap_or_default();
        if let Some(preview) = self.reports.get(&component.id).cloned() {
            preview.update(cx, |p, cx| p.set_params(Some(params.clone()), cx));
            return preview;
        }
        let (shell, params) = (self.shell.clone(), params.clone());
        let preview = cx.new(|cx| ReportPreview::new(shell, &report_id, Some(params), false, cx));
        self.reports.insert(component.id.clone(), preview.clone());
        preview
    }

    fn render_progress(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.loader.slow() {
            return None;
        }
        let (total, done) = (self.loader.total(), self.loader.total() - self.loader.pending());
        let text = format!("Loading page data… {}s ({done} of {total} queries done)", self.loader.elapsed_ms / 1000);
        let theme = cx.theme();
        let fraction = done as f32 / total.max(1) as f32;
        Some(
            h_flex()
                .gap_3()
                .items_center()
                .px_3()
                .child(widgets::text("dash-progress", text))
                .child(
                    div()
                        .id("dash-progress-bar")
                        .aria_label(format!("Page queries progress: {done} of {total}"))
                        .w(px(160.))
                        .h(px(6.))
                        .rounded_full()
                        .bg(theme.border)
                        .child(div().h_full().w(relative(fraction)).rounded_full().bg(theme.primary))
                        .test_support(),
                )
                .child(
                    Button::new("dash-cancel")
                        .outline()
                        .small()
                        .icon(IconName::CircleX)
                        .label("Cancel queries")
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_queries(cx))),
                )
                .into_any_element(),
        )
    }

    fn render_component(
        &mut self,
        dashboard: &Page,
        component: &PageComponent,
        visible: &ConditionResult,
        scope: &Map<String, Value>,
        params: &Map<String, Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let name = if component.title.is_empty() { kind_label(component.kind).to_string() } else { component.title.clone() };
        let enabled = condition_result(component.enabled_when.as_deref(), scope, true);
        let problems: Vec<(&str, String)> = [
            visible.error.as_ref().map(|e| ("visible", format!("Visible when: {e}"))),
            enabled.error.as_ref().map(|e| ("enabled", format!("Enabled when: {e}"))),
        ]
        .into_iter()
        .flatten()
        .collect();
        let body = if visible.error.is_some() { None } else { Some(self.render_body(dashboard, component, &name, scope, params, window, cx)) };
        let show_title = !component.title.is_empty() && !matches!(component.kind, ComponentKind::Button | ComponentKind::Filter);
        let rows = component.placement.row_span.max(1) as f32;
        let min_height = rows * ROW_HEIGHT + (rows - 1.0) * dashboard.layout.row_gap as f32;
        let muted = theme.muted_foreground;
        let disabled = !enabled.value;
        v_flex()
            .id(SharedString::from(format!("widget:{name}")))
            .aria_label(name.clone())
            .size_full()
            .min_w_0()
            .overflow_hidden()
            .min_h(px(min_height))
            .gap_2()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .when(show_title, |el| {
                el.child(widgets::styled_text(format!("widget-title:{name}"), component.title.clone(), |d| {
                    d.text_sm().font_weight(FontWeight::SEMIBOLD)
                }))
            })
            .children(problems.into_iter().map(|(which, text)| {
                widgets::callout(format!("widget-problem:{name}:{which}"), widgets::Severity::Danger, &text, None, cx)
            }))
            .when(disabled && enabled.error.is_none(), |el| {
                el.child(widgets::styled_text(format!("widget-disabled:{name}"), "Not available right now.", move |d| {
                    d.text_sm().text_color(muted)
                }))
            })
            .children(body.map(|body| {
                // Presentation only, not access control (use roles for that): a disabled
                // component is dimmed and an overlay swallows the clicks.
                div()
                    .relative()
                    .w_full()
                    .min_w_0()
                    .when(disabled, |el| el.opacity(0.5))
                    .child(body)
                    .when(disabled, |el| el.child(div().absolute().size_full().top_0().left_0().occlude()))
            }))
            .test_support()
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_body(
        &mut self,
        dashboard: &Page,
        component: &PageComponent,
        name: &str,
        scope: &Map<String, Value>,
        params: &Map<String, Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = self.component_state(dashboard, component, cx);
        let muted = |id: &str, text: &str, cx: &App| message(format!("{id}:{name}"), text.to_string(), false, cx).into_any_element();
        match component.kind {
            ComponentKind::Kpi => self.kpi_body(component, name, state, scope, cx),
            ComponentKind::Chart => self.chart_widget(component, name, state, cx),
            ComponentKind::Table => self.table_body(component, name, state, scope, cx),
            ComponentKind::Filter => match dashboard.filters.iter().find(|f| Some(&f.id) == component.filter_id.as_ref()) {
                Some(filter) => {
                    let filter = filter.clone();
                    self.filter_input(&filter, window, cx)
                }
                None => muted("gate", "Choose a filter.", cx),
            },
            ComponentKind::Form => {
                if component.form_id.as_deref().is_none_or(str::is_empty) {
                    return muted("gate", "Choose a form.", cx);
                }
                self.form_for(component, params, cx).into_any_element()
            }
            ComponentKind::Report => {
                let Some(report) = component.report_id.clone().filter(|r| !r.is_empty()) else {
                    return muted("gate", "Choose a report.", cx);
                };
                if !self.env.can("report", &report, Operation::Read, cx) {
                    return message(format!("gate:{name}"), "You do not have access to this report.", true, cx).into_any_element();
                }
                self.report_for(component, params, cx).into_any_element()
            }
            ComponentKind::Button => self.button_body(component, name, cx),
            ComponentKind::Text => self.text_body(component, name),
        }
    }

    /// The filters that are not placed on the canvas, shown in the toolbar.
    fn bar_filters(dashboard: &Page) -> Vec<PageFilter> {
        let placed: HashSet<&str> = dashboard
            .components
            .iter()
            .filter(|c| c.kind == ComponentKind::Filter)
            .filter_map(|c| c.filter_id.as_deref())
            .collect();
        dashboard.filters.iter().filter(|f| !placed.contains(f.id.as_str())).cloned().collect()
    }
}

impl Render for PageView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(dashboard) = self.dashboard(cx) else {
            return message("dash-missing", "This page does not exist.", true, cx).into_any_element();
        };
        let params = self.params(&dashboard);
        let scope = self.scope(&params, cx);
        let ids = self.runnable_queries(&dashboard, cx);
        let label = self.env.label.to_string();
        Loader::sync(self, |view| &mut view.loader, &label, ids, &params, cx);
        self.load_options(&dashboard, cx);
        let bar = Self::bar_filters(&dashboard);
        let toolbar = h_flex()
            .id("dash-toolbar")
            .aria_label(format!("{} filters", dashboard.name))
            .gap_3()
            .items_end()
            .flex_wrap()
            .px_3()
            .py_2()
            .children(bar.iter().map(|f| self.filter_input(f, window, cx)))
            .child(
                Button::new("dash-refresh")
                    .outline()
                    .small()
                    .icon(IconName::RefreshCw)
                    .label("Refresh")
                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
            );
        let progress = self.render_progress(cx);
        let notice = self.notice.clone().map(|text| {
            div().px_3().child(widgets::callout("dash-notice", Severity::Danger, &text, None, cx))
        });
        let grid = if dashboard.components.is_empty() {
            message("dash-empty", "This page has no components yet.", false, cx).into_any_element()
        } else {
            // Hidden components are not rendered; a condition that fails to evaluate keeps its
            // component on screen with the error.
            let mut canvas = GridCanvas::new("dash-grid", dashboard.layout.clone()).label(dashboard.name.clone());
            for c in &dashboard.components {
                let visible = condition_result(c.visible_when.as_deref(), &scope, true);
                if !visible.value && visible.error.is_none() {
                    continue;
                }
                let widget = self.render_component(&dashboard, c, &visible, &scope, &params, window, cx);
                canvas = canvas.item(GridItem::new(c.id.clone(), c.placement.clone(), widget).label(c.title.clone()));
            }
            canvas.into_any_element()
        };
        v_flex()
            .id("dash-view")
            .size_full()
            .gap_2()
            .overflow_y_scroll()
            .children(self.back.as_ref().map(back_crumb))
            .child(toolbar)
            .children(progress)
            .children(notice)
            .child(div().px_3().pb_3().child(grid))
            .into_any_element()
    }
}
