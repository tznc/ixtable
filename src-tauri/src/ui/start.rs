//! The start screen: new/open document, templates and recent documents.
//! Port of src/shell/StartScreen.tsx. Feature modules add more entry points (Access
//! import, runtime bundles, cloud apps, recovery) through `sections`.
//!
//! Test ids: `new-document`, `open-document`, `open-bundle` (link), `import-access`,
//! `start-heading`, `recent:<file name>`, `template:<id>`, `progress`, `error`, `notice`.

use crate::{
    manager::{AppError, SessionState},
    storage::RecentFile,
    templates::TemplateInfo,
    ui::{dialogs, task, toast::{self, Toast}, widgets},
};
use gpui_kit::assets::IconName;
use crate::ui::widgets::StartAligned as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use std::path::PathBuf;

const RECENT_PREVIEW: usize = 5;

pub enum StartEvent {
    Opened(SessionState),
}

pub struct StartScreen {
    label: SharedString,
    pending: Option<String>,
    error: Option<AppError>,
    notice: Option<Toast>,
    recents: Vec<RecentRow>,
    recovery: AnyView,
    templates: Vec<TemplateInfo>,
    show_all: bool,
    panels: Vec<AnyView>,
}
impl EventEmitter<StartEvent> for StartScreen {}

/// A recent document as listed: its name, folder and last-modified date.
#[derive(Clone)]
struct RecentRow {
    path: String,
    name: String,
    folder: String,
    modified: Option<String>,
}

impl RecentRow {
    fn new(recent: RecentFile) -> Self {
        let path = PathBuf::from(&recent.path);
        let folder = path
            .parent()
            .map(|p| tilde(&p.to_string_lossy()))
            .unwrap_or_default();
        let modified = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%-d %b %Y").to_string());
        Self { name: file_name(&recent.path), path: recent.path, folder, modified }
    }
}

/// Shortens the home directory to "~".
fn tilde(folder: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && folder.starts_with(&home) => format!("~{}", &folder[home.len()..]),
        _ => folder.to_string(),
    }
}

/// The icon a template is listed with.
fn template_icon(id: &str) -> IconName {
    match id.split('@').next().unwrap_or(id) {
        "crm" => IconName::Shapes,
        "inventory" => IconName::PackageOpen,
        "work-orders" => IconName::Settings,
        _ => IconName::LayoutDashboard,
    }
}

/// A full-width list row: icon, bold title, muted detail (up to `lines` lines) and an
/// optional muted trailing note. `name` is the accessibility label.
fn list_row(
    id: SharedString,
    icon: IconName,
    title: String,
    detail: String,
    lines: usize,
    trailing: Option<String>,
    cx: &App,
) -> Button {
    let muted = cx.theme().muted_foreground;
    let name = title.clone();
    Button::new(id).w_full().h_auto().py_2().accessibility_label(name).child(
        h_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .items_center()
            .child(Icon::new(icon))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .items_start()
                    .text_left()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                    // Buttons don't wrap by default; the description wraps up to `lines` lines.
                    .child(div().w_full().whitespace_normal().text_sm().text_color(muted).line_clamp(lines).child(detail)),
            )
            .when_some(trailing, |row, note| {
                row.child(div().text_sm().text_color(muted).flex_none().child(note))
            }),
    )
}

fn file_name(path: &str) -> String {
    PathBuf::from(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

impl StartScreen {
    pub fn new(
        label: &str,
        notice: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (panels, recovery) = super::start_sections::panels(&cx.entity(), label, window, cx);
        let mut this = Self {
            label: label.to_string().into(),
            pending: None,
            error: None,
            notice: None,
            recents: vec![],
            recovery,
            templates: vec![],
            show_all: false,
            panels,
        };
        if let Some(notice) = notice {
            this.set_notice(notice, cx);
        }
        this.refresh(cx);
        this
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        task::spawn_call(
            cx,
            || {
                (
                    crate::list_recent_files().unwrap_or_default(),
                    crate::templates::list_templates().unwrap_or_default(),
                )
            },
            |this, (recents, templates), cx| {
                this.recents = recents.into_iter().map(RecentRow::new).collect();
                this.templates = templates;
                cx.notify();
            },
        );
    }

    pub fn is_busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Runs an open action with progress and error display; emits `Opened` on success.
    pub fn run(
        &mut self,
        pending: &str,
        action: impl FnOnce(String) -> Result<SessionState, AppError> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.pending = Some(pending.to_string());
        self.error = None;
        self.notice = None;
        cx.notify();
        let label = self.label.to_string();
        task::spawn_call(
            cx,
            move || action(label),
            |this, result, cx| {
                this.pending = None;
                match result {
                    Ok(state) => cx.emit(StartEvent::Opened(state)),
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
    }

    /// A neutral toast, such as "Open canceled."
    pub fn set_notice(&mut self, notice: impl Into<String>, cx: &mut Context<Self>) {
        toast::show(self, |s| &mut s.notice, Toast::info(notice), cx);
    }

    pub fn set_error(&mut self, error: AppError, cx: &mut Context<Self>) {
        self.error = Some(error);
        cx.notify();
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    fn create(&mut self, cx: &mut Context<Self>) {
        self.run("Creating document…", crate::new_document, cx);
    }

    fn open(&mut self, cx: &mut Context<Self>) {
        self.pending = Some("Choosing document…".into());
        self.error = None;
        self.notice = None;
        cx.notify();
        let pick = dialogs::pick_file(cx, "Open document", &[dialogs::IXT]);
        task::after(cx, pick, |this, path, cx| match path {
            None => {
                this.pending = None;
                this.set_notice("Open canceled.", cx);
            }
            Some(path) => this.open_path(path.to_string_lossy().into_owned(), cx),
        });
    }

    /// A file the OS asked to open: documents open directly; runtime bundles go to the
    /// bundle flow (`release`).
    pub fn open_request(&mut self, path: String, cx: &mut Context<Self>) {
        if crate::ui::is_runtime_bundle(&path) {
            crate::ui::start_sections::open_bundle(self, path, cx);
        } else {
            self.open_path(path, cx);
        }
    }

    /// Opens a document by path (recent list, OS open request).
    pub fn open_path(&mut self, path: String, cx: &mut Context<Self>) {
        self.run(
            "Opening document…",
            move |label| crate::open_document(label, path),
            cx,
        );
    }

    fn render_action(
        id: &'static str,
        icon: IconName,
        title: &'static str,
        detail: &'static str,
        primary: bool,
        disabled: bool,
    ) -> Button {
        let button = Button::new(id)
            .start_label(Some(Icon::new(icon)), title)
            .tooltip(detail)
            .disabled(disabled)
            .w(px(220.));
        if primary { button.primary() } else { button.outline() }
    }
}

impl Render for StartScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.is_busy();
        let actions = super::start_sections::actions(&cx.entity(), busy, cx);
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let visible: Vec<RecentRow> = if self.show_all {
            self.recents.clone()
        } else {
            self.recents.iter().take(RECENT_PREVIEW).cloned().collect()
        };
        v_flex()
            .id("start-screen")
            .size_full()
            .overflow_y_scroll()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(
                h_flex()
                    .gap_2()
                    .p_4()
                    .items_center()
                    .child(
                        div()
                            .px_2()
                            .rounded_md()
                            .bg(gpui_kit::white())
                            .text_color(gpui_kit::black())
                            .font_weight(FontWeight::BOLD)
                            .child("ix"),
                    )
                    .child(div().font_weight(FontWeight::BOLD).child("ixtable")),
            )
            .child(
                v_flex()
                    .max_w(px(880.))
                    .w_full()
                    .mx_auto()
                    .px_8()
                    .py_4()
                    .gap_3()
                    .child(self.recovery.clone())
                    .child(div().text_xs().text_color(muted).child("DOCUMENT DATABASE"))
                    .child(widgets::styled_text(
                        "start-heading",
                        "Your data, in one portable file.",
                        |d| d.text_3xl().font_weight(FontWeight::BOLD),
                    ))
                    .child(div().text_color(muted).child(
                        "Create or open an ixtable document. Every database, setting, and \
                         attachment stays together inside its .ixt archive.",
                    ))
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_3()
                            .items_center()
                            .child(
                                Self::render_action(
                                    "new-document",
                                    IconName::FilePlus,
                                    "New document",
                                    "Start with an empty database",
                                    true,
                                    busy,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.create(cx))),
                            )
                            .child(
                                Self::render_action(
                                    "open-document",
                                    IconName::FolderOpen,
                                    "Open document",
                                    "Choose an .ixt file",
                                    false,
                                    busy,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.open(cx))),
                            )
                            .child(div().flex_1())
                            .children(actions),
                    )
                    .child(super::start_sections::import_card(&cx.entity(), busy, cx))
                    .when_some(self.pending.clone(), |el, p| el.child(widgets::progress_line(&p, cx)))
                    .when_some(self.error.clone(), |el, e| {
                        el.child(widgets::error_banner(
                            "error",
                            &e,
                            Some(Box::new({
                                let entity = cx.entity();
                                move |_, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.error = None;
                                        cx.notify();
                                    })
                                }
                            })),
                            cx,
                        ))
                    })
                    .when_some(self.notice.clone(), |el, n| {
                        let entity = cx.entity();
                        el.child(toast::render(
                            &n,
                            move |_, cx| {
                                entity.update(cx, |this, cx| {
                                    this.notice = None;
                                    cx.notify();
                                })
                            },
                            window,
                            cx,
                        ))
                    })
                    .children(self.panels.clone())
                    .when(!self.templates.is_empty(), |el| {
                        el.child(
                            v_flex()
                                .gap_2()
                                .child(widgets::section_heading("templates-heading", "Start from a template"))
                                .child(h_flex().flex_wrap().gap_2().children(self.templates.iter().map(|t| {
                                    let id = t.id.clone();
                                    let name = t.name.clone();
                                    let row = list_row(
                                        SharedString::from(format!("template:{}", t.id)),
                                        template_icon(&t.id),
                                        t.name.clone(),
                                        t.description.clone(),
                                        2,
                                        Some(format!("v{}", t.version)),
                                        cx,
                                    )
                                    .outline()
                                    .disabled(busy)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let id = id.clone();
                                        this.run(&format!("Creating {name}…"), move |label| {
                                            crate::templates::create_from_template(label, id)
                                        }, cx);
                                    }));
                                    // Two templates per row.
                                    div().w(relative(0.49)).child(row)
                                }))),
                        )
                    })
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                h_flex()
                                    .justify_between()
                                    .child(widgets::section_heading("recent-heading", "Recent documents"))
                                    .when(self.recents.len() > RECENT_PREVIEW, |el| {
                                        el.child(
                                            Button::new("toggle-recents")
                                                .link()
                                                .label(if self.show_all { "Show fewer" } else { "View all" })
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.show_all = !this.show_all;
                                                    cx.notify();
                                                })),
                                        )
                                    }),
                            )
                            .when(self.recents.is_empty(), |el| {
                                el.child(widgets::empty_state(
                                    "no-recents",
                                    "No recent documents",
                                    "Documents you open will appear here.",
                                    cx,
                                ))
                            })
                            .children(visible.into_iter().map(|recent| {
                                let path = recent.path.clone();
                                let detail = if recent.folder.is_empty() { recent.path.clone() } else { recent.folder.clone() };
                                list_row(
                                    SharedString::from(format!("recent:{}", recent.name)),
                                    IconName::FileText,
                                    recent.name.clone(),
                                    detail,
                                    1,
                                    recent.modified.clone(),
                                    cx,
                                )
                                .ghost()
                                .tooltip(format!("Open {}", recent.path))
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_path(path.clone(), cx)
                                }))
                            })),
                    ),
            )
    }
}

#[cfg(test)]
#[path = "start_tests.rs"]
mod tests;
