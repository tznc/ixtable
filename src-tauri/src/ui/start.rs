//! The start screen: new/open document, templates and recent documents.
//! Port of src/shell/StartScreen.tsx. Feature modules add more entry points (Access
//! import, runtime bundles, cloud apps, recovery) through `sections`.
//!
//! Test ids: `new-document`, `open-document`, `start-heading`, `recent:<file name>`,
//! `template:<id>`, `progress`, `error`, `notice`.

use crate::{
    manager::{AppError, SessionState},
    storage::RecentFile,
    templates::TemplateInfo,
    ui::{dialogs, task, widgets},
};
use gpui_kit::assets::IconName;
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
    notice: Option<String>,
    recents: Vec<RecentFile>,
    templates: Vec<TemplateInfo>,
    show_all: bool,
    panels: Vec<AnyView>,
}
impl EventEmitter<StartEvent> for StartScreen {}

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
        let panels = super::start_sections::panels(&cx.entity(), label, window, cx);
        let mut this = Self {
            label: label.to_string().into(),
            pending: None,
            error: None,
            notice,
            recents: vec![],
            templates: vec![],
            show_all: false,
            panels,
        };
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
                this.recents = recents;
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

    pub fn set_notice(&mut self, notice: impl Into<String>, cx: &mut Context<Self>) {
        self.notice = Some(notice.into());
        cx.notify();
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
                this.notice = Some("Open canceled.".into());
                cx.notify();
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
            .icon(Icon::new(icon))
            .label(title)
            .tooltip(detail)
            .disabled(disabled)
            .w(px(220.));
        if primary { button.primary() } else { button.outline() }
    }
}

impl Render for StartScreen {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.is_busy();
        let actions = super::start_sections::actions(&cx.entity(), busy, cx);
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let visible: Vec<RecentFile> = if self.show_all {
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
                    .py_6()
                    .gap_4()
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
                            .children(actions),
                    )
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
                        el.child(widgets::notice_banner(
                            &n,
                            move |_, cx| {
                                entity.update(cx, |this, cx| {
                                    this.notice = None;
                                    cx.notify();
                                })
                            },
                            cx,
                        ))
                    })
                    .children(self.panels.clone())
                    .when(!self.templates.is_empty(), |el| {
                        el.child(
                            v_flex()
                                .gap_2()
                                .child(widgets::section_heading("templates-heading", "Start from a template"))
                                .children(self.templates.iter().map(|t| {
                                    let id = t.id.clone();
                                    let name = t.name.clone();
                                    Button::new(SharedString::from(format!("template:{}", t.id)))
                                        .outline()
                                        .justify_start()
                                        .w_full()
                                        .icon(Icon::new(IconName::LayoutDashboard))
                                        .label(format!("{} — {} (v{})", t.name, t.description, t.version))
                                        .disabled(busy)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let id = id.clone();
                                            this.run(&format!("Creating {name}…"), move |label| {
                                                crate::templates::create_from_template(label, id)
                                            }, cx);
                                        }))
                                })),
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
                                let name = file_name(&recent.path);
                                let path = recent.path.clone();
                                Button::new(SharedString::from(format!("recent:{name}")))
                                    .ghost()
                                    .justify_start()
                                    .w_full()
                                    .icon(Icon::new(IconName::FileText))
                                    .label(format!("{name} — {}", recent.path))
                                    .tooltip(format!("Open recent {name}"))
                                    .disabled(busy)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.open_path(path.clone(), cx)
                                    }))
                            })),
                    ),
            )
    }
}
