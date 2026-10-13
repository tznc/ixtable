//! The `app` mode: application settings as a tab strip over `TABS`. Each tab's view
//! lives in its owning feature module and is registered in `build_tab`.

use super::{Shell, placeholder::TabPlaceholder};
use crate::ui::widgets;
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

/// Tabs in display order: (key, label). Test ids are `tab-<key>`.
pub const TABS: &[(&str, &str)] = &[
    ("assets", "Assets"),
    ("release", "Release"),
    ("cloud", "Cloud"),
    ("datasource", "Datasource"),
    ("entities", "Entities"),
    ("files", "File sources"),
    ("migrations", "Migrations"),
    ("roles", "Roles"),
    ("yaml", "YAML"),
    ("problems", "Problems"),
    ("logs", "Logs"),
    ("access", "Access migration"),
];

/// Whether a tab applies to this document (the Access tab only after an Access import).
fn visible(key: &str, shell: &Shell, cx: &App) -> bool {
    match key {
        "access" => shell
            .config(cx)
            .settings
            .get("accessImport")
            .and_then(|a| a.get("report"))
            .is_some_and(|r| r.is_array()),
        _ => true,
    }
}

fn build_tab(key: &str, shell: &Entity<Shell>, window: &mut Window, cx: &mut App) -> AnyView {
    match key {
        "assets" => cx
            .new(|cx| crate::ui::persistence::assets_tab::AssetsTab::new(shell.clone(), cx))
            .into(),
        "release" => cx
            .new(|cx| crate::ui::release::release_tab::ReleaseTab::new(shell.clone(), window, cx))
            .into(),
        "cloud" => cx
            .new(|cx| crate::ui::cloud::cloud_tab::CloudTab::new(shell.clone(), cx))
            .into(),
        "logs" => {
            let label = shell.read(cx).label().to_string();
            cx.new(|cx| crate::ui::persistence::logs_tab::LogsTab::new(label, cx))
                .into()
        }
        "access" => cx
            .new(|cx| crate::ui::access::migration_tab::AccessMigrationTab::new(shell.clone(), window, cx))
            .into(),
        "migrations" => cx
            .new(|cx| crate::ui::migrations::tab::MigrationsTab::new(shell.clone(), cx))
            .into(),
        "yaml" => cx
            .new(|cx| super::yaml::YamlTab::new(shell.clone(), window, cx))
            .into(),
        "problems" => cx
            .new(|cx| super::problems::ProblemsTab::new(shell.clone(), cx))
            .into(),
        _ => {
            let label = TABS
                .iter()
                .find(|(k, _)| *k == key)
                .map_or("Settings", |(_, l)| *l);
            cx.new(|_| TabPlaceholder { title: label }).into()
        }
    }
}

pub struct AppSettings {
    shell: Entity<Shell>,
    active: &'static str,
    view: Option<(&'static str, AnyView)>,
    runtime: String,
    _subscription: Subscription,
}

impl AppSettings {
    pub fn new(shell: Entity<Shell>, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let info = crate::app_info();
        let runtime = format!(
            "{} · {} runtime",
            info["name"].as_str().unwrap_or("ixtable"),
            info["runtime"].as_str().unwrap_or("tauri")
        );
        let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
        Self {
            shell,
            active: "assets",
            view: None,
            runtime,
            _subscription: subscription,
        }
    }

    /// The open tab's view (tests downcast it).
    pub fn tab_view(&self) -> Option<AnyView> {
        self.view.as_ref().map(|(_, v)| v.clone())
    }

    pub fn select(&mut self, key: &'static str, cx: &mut Context<Self>) {
        self.active = key;
        cx.notify();
    }
}

impl Render for AppSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shell = self.shell.read(cx);
        let tabs: Vec<(&'static str, &'static str)> = TABS
            .iter()
            .copied()
            .filter(|(k, _)| visible(k, shell, cx))
            .collect();
        if !tabs.iter().any(|(k, _)| *k == self.active) {
            self.active = tabs[0].0;
        }
        if self.view.as_ref().is_none_or(|(k, _)| *k != self.active) {
            let view = build_tab(self.active, &self.shell, window, cx);
            self.view = Some((self.active, view));
        }
        let body = self.view.as_ref().map(|(_, v)| v.clone());
        let entity = cx.entity();
        v_flex()
            .size_full()
            .child(
                widgets::titlebar("Project / Settings", "Application settings", cx).child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(self.runtime.clone()),
                ),
            )
            .child(widgets::tab_strip(
                "settings-tabs",
                &tabs,
                self.active,
                move |key, _, cx| entity.update(cx, |this, cx| this.select(key, cx)),
                cx,
            ))
            .child(
                div()
                    .id("settings-panel")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .when_some(body, |el, b| el.child(b))
                    .aria_label("Settings panel")
                    .test_support(),
            )
    }
}
