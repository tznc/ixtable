//! Dashboards mode (PRD §16): dashboard list, designer on the shared grid, and live view.
//! Port of `src/dashboards/`; the non-UI logic is `crate::dashboard_engine`.
//!
//! - `DashboardsMode` is the mode's view: a dashboard list, the toolbar (name, duplicate,
//!   delete, Design/View tabs) and the designer or the live view.
//! - `DashboardEditor` (`editor.rs`, `props.rs`, `filters.rs`, `settings.rs`) edits one
//!   dashboard.
//! - `DashboardView` (`view.rs`, `bodies.rs`, `chart.rs`, `embedded.rs`, `data.rs`) shows it
//!   live, in the Studio's View tab and, through `register`, as a Runtime page.
//!
//! Test ids: `new-dashboard`, `dashboard-item-N`, `no-dashboards`, `dashboard-name`,
//! `duplicate-dashboard`, `delete-dashboard`, `tab-design`, `tab-view`, `mode-title`,
//! `no-dashboard`.

mod chart;
mod data;
mod editor;
mod embedded;
mod filters;
mod props;
mod settings;
mod view;
mod bodies;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_embedded;
#[cfg(test)]
mod tests_runtime;
#[cfg(test)]
mod tests_view;

pub use data::{QueryRunner, QueryState, SLOW_QUERY_MS};
pub use editor::DashboardEditor;
pub use embedded::{Binding, EmbeddedForm};
pub use view::{DashboardView, WidgetState};

use crate::{
    dashboard_engine::model::{duplicate_dashboard, new_dashboard},
    dashboards::Dashboard,
    design::FormMode,
    ui::{
        prelude::*,
        reports::fields::{input, Inputs},
        runtime::{
            run_mode::{PageBuilders, PageBuilder},
            RuntimeEnv,
        },
        shell::{ModeId, RevealTarget, Shell},
    },
};
use serde_json::Map;
use std::rc::Rc;

/// Height of one dashboard grid row in pixels; spans add the row gap between rows.
pub const ROW_HEIGHT: f32 = 120.0;

/// A form mode as stored (`list`, `detail`, ...).
pub fn mode_name(mode: FormMode) -> String {
    serde_json::to_value(mode).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

pub fn parse_mode(name: &str) -> Option<FormMode> {
    serde_json::from_value(serde_json::Value::String(name.into())).ok()
}

/// Registers the dashboard page of Runtime navigation.
pub fn register(cx: &mut App) {
    let builder: PageBuilder = Rc::new(|page, env, back, shell, window, cx| {
        let (shell, env, id, params) = (shell.clone(), env.clone(), page.id.clone(), page.params.clone());
        let view = cx.new(|cx| DashboardView::new(shell, env, &id, params, back, cx));
        let _ = window;
        view.into()
    });
    let mut builders = cx.try_global::<PageBuilders>().cloned().unwrap_or_default();
    builders.dashboard = Some(builder);
    cx.set_global(builders);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Design,
    View,
}

const TABS: [(&str, &str); 2] = [("design", "Design"), ("view", "View")];

/// "Dashboard 1", "Dashboard 2", ... the first not taken.
fn unique_name(base: &str, taken: &[String]) -> String {
    (1..).map(|n| format!("{base} {n}")).find(|name| !taken.contains(name)).unwrap_or_default()
}

pub struct DashboardsMode {
    shell: Entity<Shell>,
    pub selected: Option<String>,
    pub tab: Tab,
    focus: Option<String>,
    focus_seq: u64,
    editor: Option<(String, u64, Entity<DashboardEditor>)>,
    live: Option<(String, Entity<DashboardView>)>,
    inputs: Inputs<DashboardsMode, String>,
    _subscription: Subscription,
}

impl DashboardsMode {
    pub fn new(shell: Entity<Shell>, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
        Self {
            shell,
            selected: None,
            tab: Tab::Design,
            focus: None,
            focus_seq: 0,
            editor: None,
            live: None,
            inputs: Inputs::new(Self::on_edit),
            _subscription: subscription,
        }
    }

    pub fn editor(&self) -> Option<Entity<DashboardEditor>> {
        self.editor.as_ref().map(|(_, _, e)| e.clone())
    }

    pub fn live(&self) -> Option<Entity<DashboardView>> {
        self.live.as_ref().map(|(_, v)| v.clone())
    }

    /// Opens the dashboard another mode asked to reveal.
    fn take_reveal(&mut self, cx: &mut Context<Self>) {
        let request = self.shell.update(cx, |s, _| s.take_reveal(ModeId::Dashboards));
        if let Some(RevealTarget::Dashboards { object_id, element_id }) = request {
            self.selected = Some(object_id);
            self.tab = Tab::Design;
            self.focus = element_id;
            self.focus_seq += 1;
            cx.notify();
        }
    }

    /// The dashboard shown: the selected one, else the first.
    fn current(&self, cx: &App) -> Option<Dashboard> {
        let config = self.shell.read(cx).config(cx);
        config.dashboards.iter().find(|d| Some(&d.id) == self.selected.as_ref()).or(config.dashboards.first()).cloned()
    }

    fn edit_config(&mut self, label: &str, mutate: impl FnOnce(&mut crate::archive::DocumentConfig) + 'static, cx: &mut Context<Self>) {
        let store = self.shell.read(cx).store.clone();
        if let Err(e) = store.update(cx, |s, cx| s.update(label, mutate, cx)) {
            self.shell.update(cx, |s, cx| s.set_error(Some(e), cx));
        }
        cx.notify();
    }

    fn on_edit(this: &mut Self, key: &String, text: String, cx: &mut Context<Self>) {
        if key != "dashboard-name" {
            return;
        }
        let Some(current) = this.current(cx) else { return };
        if current.name == text {
            return;
        }
        let id = current.id;
        this.edit_config("Rename dashboard", move |c| {
            if let Some(d) = c.dashboards.iter_mut().find(|d| d.id == id) {
                d.name = text;
            }
        }, cx);
    }

    pub fn create(&mut self, cx: &mut Context<Self>) {
        let taken: Vec<String> = self.shell.read(cx).config(cx).dashboards.iter().map(|d| d.name.clone()).collect();
        let dashboard = new_dashboard(&unique_name("Dashboard", &taken));
        self.selected = Some(dashboard.id.clone());
        self.tab = Tab::Design;
        self.edit_config("New dashboard", move |c| c.dashboards.push(dashboard), cx);
    }

    pub fn duplicate(&mut self, cx: &mut Context<Self>) {
        let Some(dashboard) = self.current(cx) else { return };
        let copy = duplicate_dashboard(&dashboard, &format!("{} copy", dashboard.name));
        self.selected = Some(copy.id.clone());
        self.edit_config("Duplicate dashboard", move |c| c.dashboards.push(copy), cx);
    }

    pub fn delete(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.current(cx).map(|d| d.id) else { return };
        self.selected = None;
        self.edit_config("Delete dashboard", move |c| c.dashboards.retain(|d| d.id != id), cx);
    }

    fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        self.selected = Some(id.to_string());
        cx.notify();
    }

    fn set_tab(&mut self, key: &str, cx: &mut Context<Self>) {
        self.tab = if key == "view" { Tab::View } else { Tab::Design };
        // The live view is built afresh each time the tab opens, like a remounted component.
        self.live = None;
        cx.notify();
    }

    fn editor_for(&mut self, dashboard: &Dashboard, cx: &mut Context<Self>) -> Entity<DashboardEditor> {
        if let Some((id, seq, editor)) = &self.editor {
            if *id == dashboard.id && *seq == self.focus_seq {
                return editor.clone();
            }
        }
        let (shell, id, focus) = (self.shell.clone(), dashboard.id.clone(), self.focus.take());
        let editor = cx.new(|_| DashboardEditor::new(shell, &id, focus));
        self.editor = Some((dashboard.id.clone(), self.focus_seq, editor.clone()));
        editor
    }

    fn live_for(&mut self, dashboard: &Dashboard, cx: &mut Context<Self>) -> Entity<DashboardView> {
        if let Some((id, view)) = &self.live {
            if *id == dashboard.id {
                return view.clone();
            }
        }
        let shell = self.shell.clone();
        let (label, store, bus) = {
            let s = shell.read(cx);
            (s.label().to_string(), s.store.clone(), s.bus.clone())
        };
        let env = RuntimeEnv { label: label.into(), store, bus, runtime: None };
        let id = dashboard.id.clone();
        let view = cx.new(|cx| DashboardView::new(shell, env, &id, Map::new(), None, cx));
        self.live = Some((dashboard.id.clone(), view.clone()));
        view
    }

    fn render_list(&self, dashboards: &[Dashboard], current: Option<&str>, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        v_flex()
            .id("dashboard-list")
            .aria_label("Dashboard list")
            .w(px(220.))
            .h_full()
            .flex_none()
            .gap_1()
            .p_3()
            .border_r_1()
            .border_color(border)
            .child(div().text_xs().text_color(muted).child("DASHBOARDS"))
            .child(
                Button::new("new-dashboard")
                    .outline()
                    .small()
                    .icon(IconName::Plus)
                    .label("New dashboard")
                    .on_click(cx.listener(|this, _, _, cx| this.create(cx))),
            )
            .children(dashboards.iter().enumerate().map(|(i, d)| {
                let id = d.id.clone();
                Button::new(SharedString::from(format!("dashboard-item-{}", i + 1)))
                    .ghost()
                    .small()
                    .justify_start()
                    .icon(IconName::LayoutDashboard)
                    .label(if d.name.is_empty() { "Untitled dashboard".to_string() } else { d.name.clone() })
                    .selected(current == Some(d.id.as_str()))
                    .on_click(cx.listener(move |this, _, _, cx| this.select(&id, cx)))
            }))
            .when(dashboards.is_empty(), |el| el.child(widgets::text("no-dashboards", "No dashboards yet.")))
    }

    fn render_toolbar(&mut self, dashboard: &Dashboard, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self.inputs.get("dashboard-name".to_string(), &dashboard.name, "", window, cx);
        let tab = if self.tab == Tab::View { "view" } else { "design" };
        let entity = cx.entity();
        h_flex()
            .gap_3()
            .items_end()
            .px_3()
            .py_2()
            .child(widgets::field("Dashboard name", input(&name, "dashboard-name", "Dashboard name", false), cx).w(px(220.)))
            .child(
                Button::new("duplicate-dashboard")
                    .outline()
                    .small()
                    .icon(IconName::Copy)
                    .label("Duplicate dashboard")
                    .on_click(cx.listener(|this, _, _, cx| this.duplicate(cx))),
            )
            .child(
                Button::new("delete-dashboard")
                    .outline()
                    .small()
                    .icon(IconName::Delete)
                    .label("Delete dashboard")
                    .on_click(cx.listener(|this, _, _, cx| this.delete(cx))),
            )
            .child(widgets::tab_strip(
                "dashboard-view-tabs",
                &TABS,
                tab,
                move |key, _, cx| entity.update(cx, |this, cx| this.set_tab(key, cx)),
                cx,
            ))
    }
}

impl Render for DashboardsMode {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The shell is being updated while it builds this view, so reveals are taken here.
        self.take_reveal(cx);
        let dashboards = self.shell.read(cx).config(cx).dashboards.clone();
        let dashboard = self.current(cx);
        let title = dashboard.as_ref().map_or("Dashboards".to_string(), |d| d.name.clone());
        let body = match &dashboard {
            Some(dashboard) => {
                let body = match self.tab {
                    Tab::Design => self.editor_for(dashboard, cx).into_any_element(),
                    Tab::View => self.live_for(dashboard, cx).into_any_element(),
                };
                v_flex()
                    .size_full()
                    .child(self.render_toolbar(dashboard, window, cx))
                    .child(div().flex_1().min_h_0().child(body))
                    .into_any_element()
            }
            None => widgets::empty_state("no-dashboard", "Create a dashboard to start designing.", "Use New dashboard in the list.", cx)
                .into_any_element(),
        };
        v_flex()
            .size_full()
            .child(widgets::titlebar("Project / Dashboards", &title, cx))
            .child(
                h_flex()
                    .id("dashboards-mode")
                    .flex_1()
                    .min_h_0()
                    .child(self.render_list(&dashboards, dashboard.as_ref().map(|d| d.id.as_str()), cx))
                    .child(div().flex_1().min_w_0().h_full().child(body)),
            )
    }
}
