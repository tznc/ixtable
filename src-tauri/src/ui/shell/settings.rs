//! The `app` mode: application settings as a grouped left sub-nav over `GROUPS`. Each tab's view
//! lives in its owning feature module and is registered in `build_tab`.

use super::Shell;
use crate::ui::{
    display::{Tone, status_badge},
    widgets::StartAligned as _,
    store::StoreEvent,
    task, widgets,
};
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use crate::ui::widgets::{Type, TypeScale as _};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

/// Settings tabs under small headings in the left sub-nav: (heading, [(key, label)]).
/// Test ids are `tab-<key>` and are kept stable when a label changes.
pub const GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "Data",
        &[
            ("datasource", "Data source"),
            ("entities", "Record conflicts"),
            ("files", "File sources"),
            ("migrations", "Migrations"),
        ],
    ),
    ("App", &[("assets", "Assets"), ("roles", "Roles"), ("yaml", "YAML")]),
    ("Release", &[("release", "Release"), ("cloud", "Cloud")]),
    (
        "Diagnostics",
        &[("problems", "Problems"), ("logs", "Logs"), ("access", "Access migration")],
    ),
];

/// Whether a tab applies to this document (the Access tab only after an Access import).
pub(super) fn visible(key: &str, shell: &Shell, cx: &App) -> bool {
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
            cx.new(|cx| crate::ui::persistence::logs_tab::LogsTab::new(label, shell.clone(), cx))
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
        "roles" => cx
            .new(|cx| crate::ui::runtime::roles_tab::RolesTab::new(shell.clone(), window, cx))
            .into(),
        "datasource" => cx
            .new(|cx| crate::ui::schema::datasource::DatasourceTab::new(shell.clone(), window, cx))
            .into(),
        "entities" => cx
            .new(|cx| crate::ui::schema::entities::EntitiesTab::new(shell.clone(), cx))
            .into(),
        "files" => cx
            .new(|cx| crate::ui::import::sources_tab::FileSourcesTab::new(shell.clone(), window, cx))
            .into(),
        "problems" => cx
            .new(|cx| super::problems::ProblemsTab::new(shell.clone(), cx))
            .into(),
        _ => unreachable!("settings tab {key} is not in TABS"),
    }
}

pub struct AppSettings {
    shell: Entity<Shell>,
    active: &'static str,
    view: Option<(&'static str, AnyView)>,
    runtime: String,
    /// (errors, all problems) for the Problems badge; None until the first check.
    problems: Option<(usize, usize)>,
    counting: bool,
    _subscriptions: Vec<Subscription>,
}

impl AppSettings {
    pub fn new(shell: Entity<Shell>, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let info = crate::app_info();
        let runtime = format!(
            "{} · {} runtime",
            info["name"].as_str().unwrap_or("ixtable"),
            info["runtime"].as_str().unwrap_or("tauri")
        );
        let subscriptions = vec![cx.observe(&shell, |_, _, cx| cx.notify())];
        Self {
            shell,
            active: "assets",
            view: None,
            runtime,
            problems: None,
            counting: false,
            _subscriptions: subscriptions,
        }
    }

    /// Recounts problems in the background for the sub-nav badge.
    fn count_problems(&mut self, cx: &mut Context<Self>) {
        let label = self.shell.read(cx).label().to_string();
        let config = self.shell.read(cx).config(cx).clone();
        task::spawn_call(
            cx,
            move || {
                let expressions = super::expression_issues::load_expression_issues(&label, &config).len();
                crate::validation::validate_document(label).map(|plain| {
                    let errors = plain
                        .iter()
                        .filter(|i| matches!(i.severity, crate::validation::Severity::Error))
                        .count();
                    (expressions + errors, expressions + plain.len())
                })
            },
            |this, result, cx| {
                if let Ok(counts) = result {
                    this.problems = Some(counts);
                    cx.notify();
                }
            },
        );
    }

    /// The open tab's view (tests downcast it).
    pub fn tab_view(&self) -> Option<AnyView> {
        self.view.as_ref().map(|(_, v)| v.clone())
    }

    /// The key of the open tab.
    pub fn selected(&self) -> &'static str {
        self.active
    }

    pub fn select(&mut self, key: &'static str, cx: &mut Context<Self>) {
        self.active = key;
        cx.notify();
    }
}

impl Render for AppSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.counting {
            // The shell is being updated while this view is built, so wait for the first render.
            self.counting = true;
            let store = self.shell.read(cx).store.clone();
            self._subscriptions.push(cx.subscribe(&store, |this, _, e: &StoreEvent, cx| {
                if matches!(e, StoreEvent::Changed) {
                    this.count_problems(cx);
                }
            }));
            self.count_problems(cx);
        }
        let shell = self.shell.read(cx);
        let groups: Vec<(&'static str, Vec<(&'static str, &'static str)>)> = GROUPS
            .iter()
            .map(|&(heading, tabs)| {
                (heading, tabs.iter().copied().filter(|(k, _)| visible(k, shell, cx)).collect())
            })
            .filter(|(_, tabs): &(_, Vec<_>)| !tabs.is_empty())
            .collect();
        if !groups.iter().flat_map(|(_, t)| t).any(|(k, _)| *k == self.active) {
            self.active = groups[0].1[0].0;
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
                        .ty(Type::Caption)
                        .text_color(cx.theme().muted_foreground)
                        .child(self.runtime.clone()),
                ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.nav(&groups, &entity, cx))
                    .child(
                div()
                    .id("settings-panel")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .p_4()
                    .when_some(body, |el, b| el.child(b))
                    .aria_label("Settings panel")
                    .test_support(),
                    ),
            )
    }
}

impl AppSettings {
    /// The grouped left sub-nav; the selected item has an accent tint and medium weight.
    fn nav(
        &self,
        groups: &[(&'static str, Vec<(&'static str, &'static str)>)],
        entity: &Entity<Self>,
        cx: &App,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border, accent) = (theme.muted_foreground, theme.border, theme.primary);
        v_flex()
            .id("settings-nav")
            .w(px(200.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .gap_3()
            .p_2()
            .border_r_1()
            .border_color(border)
            .children(groups.iter().map(|(heading, tabs)| {
                v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .px_2()
                            .ty(Type::Caption)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(muted)
                            .child(*heading),
                    )
                    .children(tabs.iter().map(|&(key, label)| {
                        let entity = entity.clone();
                        let selected = key == self.active;
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                div().flex_1().min_w_0().child(
                                    Button::new(SharedString::from(format!("tab-{key}")))
                                        .ghost()
                                        .small()
                                        .w_full()
                                        .start_label(None, label)
                                        .when(selected, |b| {
                                            b.font_weight(FontWeight::MEDIUM).bg(accent.opacity(0.12)).text_color(accent)
                                        })
                                        .on_click(move |_, _, cx| entity.update(cx, |this, cx| this.select(key, cx))),
                                ),
                            )
                            .when_some(self.problems.filter(|&(_, all)| key == "problems" && all > 0), |row, (errors, all)| {
                                let tone = if errors > 0 { Tone::Danger } else { Tone::Warning };
                                row.child(status_badge("problems-count", &all.to_string(), tone, cx))
                            })
                    }))
            }))
    }
}
