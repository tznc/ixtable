//! Run App (port of `RunMode.tsx`): the application as its users see it, with navigation,
//! start page and role preview. `RunBody` is the Run App window's content (`window.rs`) and,
//! through `RunMode`, the whole window of a runtime bundle. Pages are forms (the `FormRenderer`), tables (generated CRUD
//! forms built in memory), and reports and dashboards, which other features provide through
//! `PageBuilders` (a placeholder shows until they register).
//!
//! Test ids: `mode-title` ("Run App"), `rt-role` ("Role: X", cloud-assigned role), the
//! "Run as" role select `run-as` ("Owner (default)" or a role name),
//! `rt-nav` ("Application navigation") with `nav:<item id>` buttons (selected = current
//! page) and `nav-group:<item id>` labels, `rt-nav-empty`, `rt-page` ("Application page"),
//! `rt-notice` / `rt-notice-error`, `rt-empty` ("Build your app") with `generate-app`,
//! `page-denied`, `page-placeholder`, `table-loading` / `table-error`.

use super::{
    BackLink, Runtime, RuntimeEnv,
    back::back_crumb,
    popup::PopupHost,
    stack::{FormRenderer, FormRendererProps},
};
use crate::archive::DocumentConfig;
use crate::design::{FormMode, NavKind, NavigationItem};
use crate::forms::actions::NoticeTone;
use crate::forms::data::{table_context, table_schema};
use crate::forms::generate::GenerateOptions;
use crate::forms::generate_app::add_generated_app;
use crate::forms::navigation::{PageKind, RuntimePage, can_open, page_for, solo_page, start_page, visible_navigation};
use crate::forms::rbac::assigned_runtime_role;
use crate::forms::registry::table_forms;
use crate::forms::schema::{flatten_navigation, form_table};
use crate::ui::{prelude::*, shell::Shell, store::StoreEvent};
use std::rc::Rc;

/// Builds the view of a report or dashboard page; registered by the features that own them.
/// Builders get the shell too: a dashboard's report widgets need it.
pub type PageBuilder =
    Rc<dyn Fn(&RuntimePage, &RuntimeEnv, Option<BackLink>, &Entity<Shell>, &mut Window, &mut App) -> AnyView>;

#[derive(Clone, Default)]
pub struct PageBuilders {
    pub report: Option<PageBuilder>,
    pub page: Option<PageBuilder>,
}
impl Global for PageBuilders {}

/// True when the application has a navigation item of any kind other than a new document's
/// blank form (no source, no controls).
pub fn has_pages(config: &DocumentConfig) -> bool {
    flatten_navigation(&config.design.navigation).iter().any(|item| {
        let Some(page) = page_for(item) else {
            return item.kind != NavKind::Form;
        };
        let page = solo_page(config, &page);
        if page.kind != PageKind::Form {
            return true;
        }
        config
            .design
            .forms
            .iter()
            .find(|f| f.id == page.id)
            .is_some_and(|f| form_table(f).is_some() || !f.controls.is_empty())
    })
}

struct Placeholder {
    back: Option<BackLink>,
    text: String,
}

impl Render for Placeholder {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let color = cx.theme().muted_foreground;
        v_flex()
            .gap_2()
            .p_4()
            .children(self.back.as_ref().map(back_crumb))
            .child(widgets::styled_text("page-placeholder", self.text.clone(), move |d| d.text_sm().text_color(color)))
    }
}

/// A table navigation item: the generated CRUD forms (with lookups and related lists).
struct TablePage {
    state: Result<Option<Entity<FormRenderer>>, String>,
}

impl TablePage {
    fn new(env: RuntimeEnv, table: String, back: Option<BackLink>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let label = env.label.to_string();
        task::spawn_call_in(
            window,
            cx,
            move || table_context(&label, &table).map_err(|e| e.message),
            move |this, context, window, cx| {
                this.state = context.map(|context| {
                    let children: Vec<&crate::data::TableSchema> = context.children.iter().map(|c| &**c).collect();
                    let targets = context.targets.iter().map(|(k, v)| (k.clone(), &**v)).collect();
                    let entities = env.config(cx).entities.clone();
                    let forms = table_forms(
                        &context.schema,
                        &GenerateOptions { targets, children, child_forms: Default::default(), entities: &entities },
                    );
                    let mut props = FormRendererProps::new(env.clone(), forms.list.id.clone());
                    props.mode = Some(FormMode::List);
                    props.back = back;
                    Some(cx.new(|cx| FormRenderer::new(props, window, cx)))
                });
                cx.notify();
            },
        );
        Self { state: Ok(None) }
    }
}

impl Render for TablePage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.state {
            Err(message) => super::message("table-error", message.clone(), true, cx).into_any_element(),
            Ok(None) => super::message("table-loading", "Loading…", false, cx).into_any_element(),
            Ok(Some(renderer)) => renderer.clone().into_any_element(),
        }
    }
}

/// The Run mode view. The shell builds a mode's view while it is itself being updated, so
/// the body (which reads the shell) is created by the first render.
pub struct RunMode {
    shell: Entity<Shell>,
    body: Option<Entity<RunBody>>,
}

impl RunMode {
    pub fn new(shell: Entity<Shell>, _: &mut Window, _: &mut Context<Self>) -> Self {
        Self { shell, body: None }
    }

    /// The running body, once rendered.
    pub fn body(&self) -> Option<&Entity<RunBody>> {
        self.body.as_ref()
    }
}

impl Render for RunMode {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.body {
            Some(body) => body.clone(),
            None => {
                let shell = self.shell.clone();
                let body = cx.new(|cx| RunBody::new(shell, window, cx));
                self.body = Some(body.clone());
                body
            }
        };
        body
    }
}

pub struct RunBody {
    shell: Entity<Shell>,
    env: RuntimeEnv,
    runtime: Entity<Runtime>,
    popups: Entity<PopupHost>,
    _handlers: crate::automation_runner::host::HandlersRegistration,
    _relay: Task<()>,
    page: Option<(String, AnyView)>,
    _subscriptions: Vec<Subscription>,
}

impl RunBody {
    pub fn new(shell: Entity<Shell>, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let (label, store, bus) = {
            let s = shell.read(cx);
            (s.label().to_string(), s.store.clone(), s.bus.clone())
        };
        // Buttons and form events run through the Automation runner unless a runner is set.
        super::automation_bridge::ensure_runner();
        let runtime = cx.new(|_| Runtime::new(&label));
        let env = RuntimeEnv { label: label.into(), store: store.clone(), bus, runtime: Some(runtime.clone()) };
        let popups = cx.new(|cx| PopupHost::new(env.clone(), runtime.clone(), cx));
        let subscriptions = vec![
            cx.observe(&runtime, |_, _, cx| cx.notify()),
            cx.observe(&shell, |_, _, cx| cx.notify()),
            cx.subscribe(&store, |_, _, _: &StoreEvent, cx| cx.notify()),
        ];
        // Triggers and actions on background threads navigate and set state through these.
        let handlers = super::handlers::RunHandlers::new(runtime.read(cx).shared_state());
        let registration = crate::automation_runner::host::install_handlers(&env.label, handlers.clone());
        let relay = handlers.relay(runtime.clone(), env.clone(), cx);
        Self { shell, env, runtime, popups, _handlers: registration, _relay: relay, page: None, _subscriptions: subscriptions }
    }

    /// The view of the page shown (tests downcast it to a `FormRenderer`).
    pub fn page_view_entity(&self) -> Option<AnyView> {
        self.page.as_ref().map(|(_, view)| view.clone())
    }

    pub fn runtime(&self) -> &Entity<Runtime> {
        &self.runtime
    }

    pub fn env(&self) -> &RuntimeEnv {
        &self.env
    }

    fn runtime_only(&self, cx: &App) -> bool {
        self.shell.read(cx).doc.runtime_only
    }

    /// The page shown: the one chosen, else the start page (none for an empty app).
    pub fn current_page(&self, cx: &App) -> Option<RuntimePage> {
        let config = self.env.config(cx);
        let empty = !self.runtime_only(cx) && !has_pages(config);
        let runtime = self.runtime.read(cx);
        runtime.page().cloned().or_else(|| if empty { None } else { start_page(config, runtime.role_id().as_deref()) })
    }

    fn back_link(&self, cx: &mut Context<Self>) -> Option<BackLink> {
        let runtime = self.runtime.read(cx);
        if !runtime.can_go_back() {
            return None;
        }
        let title = runtime.previous_title(self.env.config(cx))?;
        let target = self.runtime.clone();
        Some(BackLink::new(title, move |cx| {
            target.update(cx, |r, cx| r.back(cx));
        }))
    }

    fn page_view(&mut self, page: &RuntimePage, key: String, window: &mut Window, cx: &mut Context<Self>) -> AnyView {
        if let Some((shown, view)) = &self.page {
            if *shown == key {
                return view.clone();
            }
        }
        let back = self.back_link(cx);
        let env = self.env.clone();
        let role = self.runtime.read(cx).role_id();
        let view: AnyView = if !can_open(self.env.config(cx), role.as_deref(), page) {
            cx.new(|_| Placeholder { back: None, text: "You do not have access to this page.".into() }).into()
        } else {
            let page = &solo_page(self.env.config(cx), page);
            match page.kind {
                PageKind::Form => {
                    let mut props = FormRendererProps::new(env, page.id.clone());
                    props.mode = page.mode.as_deref().and_then(|m| match m {
                        "list" => Some(FormMode::List),
                        "detail" => Some(FormMode::Detail),
                        "create" => Some(FormMode::Create),
                        "edit" => Some(FormMode::Edit),
                        "continuous" => Some(FormMode::Continuous),
                        "split" => Some(FormMode::Split),
                        _ => None,
                    });
                    props.record_id = page.record_id.clone();
                    props.params = page.params.clone();
                    props.back = back;
                    cx.new(|cx| FormRenderer::new(props, window, cx)).into()
                }
                PageKind::Table => {
                    let table = page.id.clone();
                    cx.new(|cx| TablePage::new(env, table, back, window, cx)).into()
                }
                PageKind::Report | PageKind::Page => {
                    let builders = cx.try_global::<PageBuilders>().cloned().unwrap_or_default();
                    let (builder, noun) = if page.kind == PageKind::Report {
                        (builders.report, "Reports")
                    } else {
                        (builders.page, "Pages")
                    };
                    match builder {
                        Some(build) => {
                            let shell = self.shell.clone();
                            build(page, &env, back, &shell, window, cx)
                        }
                        None => cx
                            .new(|_| Placeholder { back, text: format!("{noun} are not available in this build.") })
                            .into(),
                    }
                }
            }
        };
        self.page = Some((key, view.clone()));
        view
    }

    fn generate(&mut self, cx: &mut Context<Self>) {
        let label = self.env.label.to_string();
        task::spawn_call(
            cx,
            move || -> Result<Vec<crate::data::TableSchema>, crate::manager::AppError> {
                let objects = crate::list_database_objects(label.clone())?;
                objects
                    .into_iter()
                    .filter(|o| o.object_type == "table")
                    .map(|o| table_schema(&label, &o.name).map(|s| (*s).clone()))
                    .collect()
            },
            |this, schemas, cx| {
                let Ok(schemas) = schemas else { return };
                let (store, env) = (this.env.store.clone(), this.env.clone());
                let entities = env.config(cx).entities.clone();
                let (design, pages) = (env.config(cx).design.clone(), env.config(cx).pages.clone());
                match add_generated_app(&design, &pages, &schemas, None, &entities) {
                    Some(next) => {
                        let _ = store.update(cx, |s, cx| s.update("Generate app", move |c| {
                            c.design = next;
                            crate::pages::upgrade::sync(c);
                        }, cx));
                    }
                    None => env.notify("There are no tables to generate from.", NoticeTone::Error, cx),
                }
                cx.notify();
            },
        );
    }

    fn nav_items(&self, items: &[NavigationItem], active: Option<&str>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        items
            .iter()
            .map(|item| {
                if item.kind == NavKind::Group {
                    return v_flex()
                        .gap_0p5()
                        .child(widgets::styled_text(
                            SharedString::from(format!("nav-group:{}", item.id)),
                            item.label.clone(),
                            |d| d.text_xs().font_weight(FontWeight::SEMIBOLD).px_2().pt_2(),
                        ))
                        .child(v_flex().pl_2().children(self.nav_items(&item.children, active, cx)))
                        .into_any_element();
                }
                let page = page_for(item);
                let runtime = self.runtime.clone();
                Button::new(SharedString::from(format!("nav:{}", item.id)))
                    .ghost()
                    .small()
                    .w_full()
                    .start_label(None, item.label.clone())
                    .selected(active == Some(item.id.as_str()))
                    .on_click(move |_, _, cx| {
                        if let Some(page) = page.clone() {
                            runtime.update(cx, |r, cx| r.navigate(page, cx));
                        }
                    })
                    .into_any_element()
            })
            .collect()
    }
}

impl Render for RunBody {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let config = self.env.config(cx).clone();
        let runtime_only = self.runtime_only(cx);
        let empty = !runtime_only && !has_pages(&config);
        let (role_id, visit, notice) = {
            let r = self.runtime.read(cx);
            (r.role_id(), r.visit(), r.notice.clone())
        };
        let page = self.current_page(cx);
        let navigation = visible_navigation(&config.design.navigation, &config, role_id.as_deref());
        let assigned = assigned_runtime_role();
        let active = page.as_ref().and_then(|p| p.nav_id.clone());
        let nav = self.nav_items(&navigation, active.as_deref(), cx);
        let nav_empty = nav.is_empty();

        let role_control: Option<AnyElement> = match (&assigned, runtime_only) {
            (Some(assigned), _) => Some(
                widgets::text("rt-role", format!("Role: {}", assigned.role.name)).into_any_element(),
            ),
            (None, false) => {
                let runtime = self.runtime.clone();
                let picker = crate::ui::controls::select(
                    "run-as",
                    role_id.as_deref().unwrap_or_default(),
                    super::window::role_options(&config),
                    super::window::OWNER,
                    move |value, _, cx| {
                        let role = (!value.is_empty()).then(|| value.to_string());
                        runtime.update(cx, |r, cx| r.set_role_id(role, cx));
                    },
                );
                Some(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().text_sm().child("Run as"))
                        .child(picker)
                        .into_any_element(),
                )
            }
            (None, true) => None,
        };

        let body: AnyElement = match &page {
            Some(page) => {
                let key = format!("{}:{}:{}", role_id.clone().unwrap_or_default(), visit, format_page(page));
                self.page_view(page, key, window, cx).into_any_element()
            }
            None => {
                self.page = None;
                if runtime_only {
                    widgets::text("rt-nopages", "This application has no pages yet.").into_any_element()
                } else {
                    div().into_any_element()
                }
            }
        };
        let kicker = format!("PROJECT / RUN APP · {}", config.name);
        let header = if runtime_only {
            widgets::titlebar("", &config.name, cx)
        } else {
            widgets::titlebar(&kicker, "Run App", cx)
        };
        v_flex()
            .size_full()
            .relative()
            .child(header.child(div().children(role_control)))
            .child(
                h_flex()
                    .flex_1()
                    .items_stretch()
                    .min_h_0()
                    .child(
                        v_flex()
                            .id("rt-nav")
                            .aria_label("Application navigation")
                            .w(px(220.))
                            .p_2()
                            .gap_0p5()
                            .border_r_1()
                            .border_color(theme.border)
                            .children(nav)
                            .when(nav_empty, |d| {
                                d.child(widgets::styled_text("rt-nav-empty", "Nothing to show for this role.", {
                                    let color = theme.muted_foreground;
                                    move |d| d.text_sm().text_color(color)
                                }))
                            })
                            .test_support(),
                    )
                    .child(
                        v_flex()
                            .id("rt-page")
                            .aria_label("Application page")
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .p_2()
                            .children(notice.map(|n| {
                                let error = n.tone == NoticeTone::Error;
                                super::message(if error { "rt-notice-error" } else { "rt-notice" }, n.message, error, cx)
                            }))
                            .when(empty, |d| {
                                d.child(
                                    v_flex()
                                        .id("rt-empty")
                                        .aria_label("Build your app")
                                        .gap_2()
                                        .p_4()
                                        .child(widgets::styled_text("rt-empty-title", "Build your app", |d| d.text_base().font_semibold()))
                                        .child(div().text_sm().text_color(theme.muted_foreground).child(
                                            "This application has no pages yet. Generate a list and a form for each table to start.",
                                        ))
                                        .child(
                                            Button::new("generate-app")
                                                .primary()
                                                .label("Generate app from tables")
                                                .on_click(cx.listener(|this, _, _, cx| this.generate(cx))),
                                        )
                                        .test_support(),
                                )
                            })
                            .child(div().flex_1().min_h_0().child(body))
                            .test_support(),
                    ),
            )
            .child(self.popups.clone())
    }
}

fn format_page(page: &RuntimePage) -> String {
    format!(
        "{}:{}:{:?}:{:?}:{}",
        page.kind.key(),
        page.id,
        page.mode,
        page.record_id,
        serde_json::Value::Object(page.params.clone())
    )
}
