//! Pages (PRD §13, §15, §16; `docs/decisions/pages.md`): a page is a grid of components
//! (forms, reports, charts, KPIs, tables, filters, buttons and text). The Design mode
//! (`DesignMode`, `design_mode.rs`) lists pages and opens a page, or a form or report shown
//! on one, in its designer. The non-UI logic is `crate::page_engine`.
//!
//! - `PagePanel` shows one page: the toolbar (name, duplicate, delete, Design/View tabs) and
//!   the designer or the live view.
//! - `PageEditor` (`editor.rs`, `props.rs`, `filters.rs`, `settings.rs`) edits one page.
//! - `PageView` (`view.rs`, `bodies.rs`, `chart.rs`, `embedded.rs`, `data.rs`) shows it
//!   live, in the Studio's View tab and, through `register`, as a Runtime page.
//!
//! Test ids: `page-name`, `duplicate-page`, `delete-page`, `tab-design`, `tab-view`.

mod chart;
mod design_mode;
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
#[cfg(test)]
mod tests_design_mode;

pub use data::{QueryRunner, QueryState, SLOW_QUERY_MS};
pub use design_mode::{DesignMode, Focus};
pub use editor::PageEditor;
pub use embedded::{Binding, EmbeddedForm};
pub use view::{PageView, WidgetState};

use crate::{
    page_engine::model::{duplicate_dashboard, new_dashboard},
    pages::Page,
    design::FormMode,
    ui::{
        prelude::*,
        controls::{input, Inputs},
        runtime::{
            run_mode::{PageBuilders, PageBuilder},
            RuntimeEnv,
        },
        shell::Shell,
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
        let view = cx.new(|cx| PageView::new(shell, env, &id, params, back, cx));
        let _ = window;
        view.into()
    });
    let mut builders = cx.try_global::<PageBuilders>().cloned().unwrap_or_default();
    builders.page = Some(builder);
    cx.set_global(builders);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Design,
    View,
}

const TABS: [(&str, &str); 2] = [("design", "Design"), ("view", "View")];

/// "Page 1", "Page 2", ... the first not taken.
fn unique_name(base: &str, taken: &[String]) -> String {
    (1..).map(|n| format!("{base} {n}")).find(|name| !taken.contains(name)).unwrap_or_default()
}

pub struct PagePanel {
    shell: Entity<Shell>,
    pub selected: Option<String>,
    pub tab: Tab,
    focus: Option<String>,
    focus_seq: u64,
    editor: Option<(String, u64, Entity<PageEditor>)>,
    live: Option<(String, Entity<PageView>)>,
    inputs: Inputs<PagePanel, String>,
    _subscription: Subscription,
}

impl PagePanel {
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

    pub fn editor(&self) -> Option<Entity<PageEditor>> {
        self.editor.as_ref().map(|(_, _, e)| e.clone())
    }

    pub fn live(&self) -> Option<Entity<PageView>> {
        self.live.as_ref().map(|(_, v)| v.clone())
    }

    /// Shows page `id` in the designer, with component `element` selected.
    pub fn open(&mut self, id: &str, element: Option<String>, cx: &mut Context<Self>) {
        self.selected = Some(id.to_string());
        self.tab = Tab::Design;
        self.focus = element;
        self.focus_seq += 1;
        cx.notify();
    }

    /// The page shown: the selected one, else the first.
    pub fn current(&self, cx: &App) -> Option<Page> {
        let config = self.shell.read(cx).config(cx);
        config.pages.iter().find(|d| Some(&d.id) == self.selected.as_ref()).or(config.pages.first()).cloned()
    }

    fn edit_config(&mut self, label: &str, mutate: impl FnOnce(&mut crate::archive::DocumentConfig) + 'static, cx: &mut Context<Self>) {
        Shell::edit_config(&self.shell, label, mutate, cx);
        cx.notify();
    }

    fn on_edit(this: &mut Self, key: &String, text: String, cx: &mut Context<Self>) {
        if key != "page-name" {
            return;
        }
        let Some(current) = this.current(cx) else { return };
        if current.name == text {
            return;
        }
        let id = current.id;
        this.edit_config("Rename page", move |c| {
            if let Some(d) = c.pages.iter_mut().find(|d| d.id == id) {
                d.name = text;
            }
        }, cx);
    }

    pub fn create(&mut self, cx: &mut Context<Self>) {
        let taken: Vec<String> = self.shell.read(cx).config(cx).pages.iter().map(|d| d.name.clone()).collect();
        let dashboard = new_dashboard(&unique_name("Page", &taken));
        self.selected = Some(dashboard.id.clone());
        self.tab = Tab::Design;
        self.edit_config("New page", move |c| c.pages.push(dashboard), cx);
    }

    pub fn duplicate(&mut self, cx: &mut Context<Self>) {
        let Some(dashboard) = self.current(cx) else { return };
        let copy = duplicate_dashboard(&dashboard, &format!("{} copy", dashboard.name));
        self.selected = Some(copy.id.clone());
        self.edit_config("Duplicate page", move |c| c.pages.push(copy), cx);
    }

    pub fn delete(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.current(cx).map(|d| d.id) else { return };
        self.selected = None;
        self.edit_config("Delete page", move |c| c.pages.retain(|d| d.id != id), cx);
    }

    pub fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        self.selected = Some(id.to_string());
        cx.notify();
    }

    fn set_tab(&mut self, key: &str, cx: &mut Context<Self>) {
        self.tab = if key == "view" { Tab::View } else { Tab::Design };
        // The live view is built afresh each time the tab opens, like a remounted component.
        self.live = None;
        cx.notify();
    }

    fn editor_for(&mut self, dashboard: &Page, cx: &mut Context<Self>) -> Entity<PageEditor> {
        if let Some((id, seq, editor)) = &self.editor {
            if *id == dashboard.id && *seq == self.focus_seq {
                return editor.clone();
            }
        }
        let (shell, id, focus) = (self.shell.clone(), dashboard.id.clone(), self.focus.take());
        let editor = cx.new(|_| PageEditor::new(shell, &id, focus));
        self.editor = Some((dashboard.id.clone(), self.focus_seq, editor.clone()));
        editor
    }

    fn live_for(&mut self, dashboard: &Page, cx: &mut Context<Self>) -> Entity<PageView> {
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
        let view = cx.new(|cx| PageView::new(shell, env, &id, Map::new(), None, cx));
        self.live = Some((dashboard.id.clone(), view.clone()));
        view
    }

    fn render_toolbar(&mut self, dashboard: &Page, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self.inputs.get("page-name".to_string(), &dashboard.name, "", window, cx);
        let tab = if self.tab == Tab::View { "view" } else { "design" };
        let entity = cx.entity();
        h_flex()
            .gap_3()
            .items_end()
            .px_3()
            .py_2()
            .child(widgets::field("Page name", input(&name, "page-name", "Page name", false), cx).w(px(220.)))
            .child(
                Button::new("duplicate-page")
                    .outline()
                    .small()
                    .icon(IconName::Copy)
                    .label("Duplicate page")
                    .on_click(cx.listener(|this, _, _, cx| this.duplicate(cx))),
            )
            .child(
                Button::new("delete-page")
                    .outline()
                    .small()
                    .icon(IconName::Delete)
                    .label("Delete page")
                    .on_click(cx.listener(|this, _, _, cx| this.delete(cx))),
            )
            .child(widgets::tab_strip(
                "page-view-tabs",
                &TABS,
                tab,
                move |key, _, cx| entity.update(cx, |this, cx| this.set_tab(key, cx)),
                cx,
            ))
    }
}

impl Render for PagePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(page) = self.current(cx) else {
            return widgets::empty_state("no-page", "Create a page to start designing.", "Use New page in the list.", cx)
                .into_any_element();
        };
        let body = match self.tab {
            Tab::Design => self.editor_for(&page, cx).into_any_element(),
            Tab::View => self.live_for(&page, cx).into_any_element(),
        };
        v_flex()
            .size_full()
            .child(self.render_toolbar(&page, window, cx))
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }
}
