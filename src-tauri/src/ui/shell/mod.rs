//! The open-document frame: sidebar (save/close, mode switch, mode sidebar, save
//! status), ribbon (view menu, undo/redo), banners and the active mode's workspace.
//! `Shell` is also the document-level state every mode reads (port of `ShellApi`).

pub mod expression_issues;
pub mod modes;
pub mod object_browser;
pub mod problems;
pub mod settings;
pub mod yaml;
#[cfg(test)]
mod tests;

use crate::{
    archive::DocumentConfig,
    data::DbObject,
    manager::{AppError, SessionState},
    ui::{
        autosave::{Autosave, AutosaveEvent, SaveStatus},
        dialogs,
        events::{DocBus, DocEvent},
        store::{ConfigStore, StoreEvent},
        task, widgets,
    },
};
use gpui_kit::assets::IconName;
use crate::ui::widgets::StartAligned as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
pub use modes::ModeId;

gpui_kit::actions!(ixtable, [Undo, Redo]);

/// Key bindings for the shell; registered once per App by `ui::init`.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-z", Undo, Some("Shell")),
        KeyBinding::new("secondary-shift-z", Redo, Some("Shell")),
        KeyBinding::new("ctrl-y", Redo, Some("Shell")),
    ]);
}

/// What the open document is (port of the React `Doc`).
#[derive(Clone, Debug)]
pub struct Doc {
    pub name: String,
    pub path: Option<String>,
    pub dirty: bool,
    pub mode: ModeId,
    pub session_id: String,
    pub document_id: String,
    /// Runtime-only bundle session (Studio modes hidden).
    pub runtime_only: bool,
    pub bundle_version: Option<String>,
}

impl Doc {
    fn from_session(state: &SessionState) -> Self {
        Self {
            name: state.name.clone(),
            path: state.path.clone(),
            dirty: state.dirty,
            mode: if state.runtime_only {
                ModeId::Run
            } else {
                ModeId::from_key(&state.active_mode)
            },
            session_id: state.session_id.clone(),
            document_id: state.document_id.clone(),
            runtime_only: state.runtime_only,
            bundle_version: state.bundle_version.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionKind {
    Table,
    View,
    Query,
    /// A new query; `id` is the unsaved draft's id, or empty to start one.
    NewQuery,
    NewTable,
}

/// The object chosen in the object browser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub kind: SelectionKind,
    pub id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutomationTab {
    Actions,
    Triggers,
}

/// An object (and optionally one of its elements) to open in a Studio mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RevealTarget {
    Design {
        object_id: String,
        element_id: Option<String>,
    },
    Reports {
        object_id: String,
        element_id: Option<String>,
    },
    Page {
        object_id: String,
        element_id: Option<String>,
    },
    Automation {
        tab: AutomationTab,
        object_id: String,
    },
}

impl RevealTarget {
    pub fn mode(&self) -> ModeId {
        match self {
            RevealTarget::Design { .. } => ModeId::Design,
            RevealTarget::Reports { .. } | RevealTarget::Page { .. } => ModeId::Design,
            RevealTarget::Automation { .. } => ModeId::Automation,
        }
    }
}

pub enum ShellEvent {
    /// The document closed; the app returns to the start screen.
    Closed,
    /// Another document opened in this window (an OS open request).
    Opened(SessionState),
}

pub struct Shell {
    label: SharedString,
    pub doc: Doc,
    pub store: Entity<ConfigStore>,
    pub bus: Entity<DocBus>,
    pub autosave: Entity<Autosave>,
    pub pending: Option<String>,
    pub error: Option<AppError>,
    dismissed_store_error: Option<String>,
    pub notice: Option<String>,
    pub objects: Vec<DbObject>,
    pub metadata_loading: bool,
    pub metadata_error: Option<String>,
    pub selection: Option<Selection>,
    /// `(id, name)` of the new query being edited on the Data page and not saved yet, so
    /// the object browser can list it.
    pub unsaved_query: Option<(String, String)>,
    pub reveal: Option<RevealTarget>,
    view: Option<(ModeId, AnyView, Option<AnyView>)>,
    focus: FocusHandle,
    /// Triggers, custom actions and the job worker of this document; dropped on close.
    _automation: crate::ui::automation::AutomationHost,
    _subscriptions: Vec<Subscription>,
}
impl EventEmitter<ShellEvent> for Shell {}

impl Shell {
    /// `store` is `ConfigStore::load(label)`, loaded by the caller so a failure can be
    /// shown where the open started.
    pub fn new(
        label: &str,
        initial: SessionState,
        store: ConfigStore,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = cx.new(|_| store);
        let bus = cx.new(|_| DocBus);
        let autosave = cx.new(|cx| Autosave::new(label, &initial, cx));
        let subscriptions = vec![
            cx.subscribe(&store, |this, _, event: &StoreEvent, cx| match event {
                StoreEvent::State(state) => this.adopt(state, cx),
                StoreEvent::Failed(_) => {
                    this.dismissed_store_error = None;
                    cx.notify();
                }
                StoreEvent::Changed => cx.notify(),
            }),
            cx.subscribe(&autosave, |this, _, event: &AutosaveEvent, cx| match event {
                AutosaveEvent::Saved(state) => {
                    this.doc = Doc::from_session(state);
                    let state = state.clone();
                    this.store.update(cx, |s, _| s.observe(&state));
                    cx.notify();
                }
            }),
            cx.subscribe(&bus, |this, _, event: &DocEvent, cx| match event {
                DocEvent::DatabaseChanged => {
                    this.reload_metadata(cx);
                    this.resync_state(cx);
                }
                DocEvent::RecordsChanged => this.resync_state(cx),
            }),
            cx.observe(&autosave, |_, _, cx| cx.notify()),
        ];
        expression_issues::register_gate(label);
        let mut this = Self {
            label: label.to_string().into(),
            doc: Doc::from_session(&initial),
            store,
            bus,
            autosave,
            pending: None,
            error: None,
            dismissed_store_error: None,
            notice: None,
            objects: vec![],
            metadata_loading: true,
            metadata_error: None,
            selection: None,
            unsaved_query: None,
            reveal: None,
            view: None,
            focus: cx.focus_handle(),
            _automation: crate::ui::automation::AutomationHost::new(label, cx),
            _subscriptions: subscriptions,
        };
        this.reload_metadata(cx);
        window.focus(&this.focus, cx);
        this
    }

    /// The window label every backend command for this document takes.
    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn config<'a>(&self, cx: &'a App) -> &'a DocumentConfig {
        self.store.read(cx).config()
    }

    /// Adopts a SessionState returned by any command (name, path, dirty, mode).
    pub fn apply_session(&mut self, state: &SessionState, cx: &mut Context<Self>) {
        self.adopt(state, cx);
        let state = state.clone();
        self.store.update(cx, |s, _| s.observe(&state));
    }

    fn adopt(&mut self, state: &SessionState, cx: &mut Context<Self>) {
        let mode = self.doc.mode;
        self.doc = Doc::from_session(state);
        // The mode only changes through `change_mode`, which rebuilds the view.
        self.doc.mode = mode;
        let state = state.clone();
        self.autosave.update(cx, |a, cx| a.sync(&state, cx));
        cx.notify();
    }

    /// Re-reads the session state (after record writes or DDL) for the dirty flag.
    fn resync_state(&mut self, cx: &mut Context<Self>) {
        let label = self.label.to_string();
        task::spawn_call(
            cx,
            move || crate::document_state(label),
            |this, result, cx| {
                if let Ok(state) = result {
                    this.apply_session(&state, cx);
                }
            },
        );
    }

    /// Records an edit made outside the config store (shows dirty, schedules autosave).
    pub fn mark_dirty(&mut self, cx: &mut Context<Self>) {
        self.doc.dirty = true;
        self.autosave.update(cx, |a, cx| a.touch(cx));
        cx.notify();
    }

    /// Applies a definition edit through the config store; a failure becomes the shell's
    /// error banner.
    pub fn edit_config(
        shell: &Entity<Shell>,
        label: &str,
        mutate: impl FnOnce(&mut crate::archive::DocumentConfig),
        cx: &mut App,
    ) {
        let store = shell.read(cx).store.clone();
        if let Err(e) = store.update(cx, |s, cx| s.update(label, mutate, cx)) {
            shell.update(cx, |s, cx| s.set_error(Some(e), cx));
        }
    }

    pub fn set_notice(&mut self, notice: impl Into<String>, cx: &mut Context<Self>) {
        self.notice = Some(notice.into());
        cx.notify();
    }

    pub fn set_error(&mut self, error: Option<AppError>, cx: &mut Context<Self>) {
        self.error = error;
        cx.notify();
    }

    pub fn select(&mut self, selection: Option<Selection>, cx: &mut Context<Self>) {
        self.selection = selection;
        cx.notify();
    }

    pub fn set_unsaved_query(&mut self, draft: Option<(String, String)>, cx: &mut Context<Self>) {
        if self.unsaved_query != draft {
            self.unsaved_query = draft;
            cx.notify();
        }
    }

    /// Announces a DDL/data change: metadata reloads and every subscriber refreshes.
    pub fn database_changed(&mut self, cx: &mut Context<Self>) {
        let bus = self.bus.clone();
        DocBus::database_changed(&bus, cx);
    }

    /// Announces committed record writes.
    pub fn records_changed(&mut self, cx: &mut Context<Self>) {
        let bus = self.bus.clone();
        DocBus::records_changed(&bus, cx);
    }

    pub fn reload_metadata(&mut self, cx: &mut Context<Self>) {
        self.metadata_loading = true;
        self.metadata_error = None;
        cx.notify();
        let label = self.label.to_string();
        task::spawn_call(
            cx,
            move || crate::list_database_objects(label),
            |this, result, cx| {
                this.metadata_loading = false;
                match result {
                    Ok(objects) => this.objects = objects,
                    Err(e) => this.metadata_error = Some(e.message),
                }
                cx.notify();
            },
        );
    }

    /// Switches the Studio mode (not an undo step) and builds its view.
    pub fn change_mode(&mut self, mode: ModeId, window: &mut Window, cx: &mut Context<Self>) {
        if self.doc.mode == mode && self.view.is_some() {
            return;
        }
        self.error = None;
        let key = mode.key().to_string();
        let result = self.store.update(cx, |s, cx| {
            s.update_with("Switch mode", false, |c| c.active_mode = key, cx)
        });
        match result {
            Ok(_) => {
                self.doc.mode = mode;
                // The Data page's query draft goes with its view.
                self.unsaved_query = None;
                self.view = None;
                self.ensure_view(window, cx);
                self.reload_metadata(cx);
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    /// Switches to the target's mode and asks it to open the object; the mode takes the
    /// request with `take_reveal`.
    pub fn request_reveal(
        &mut self,
        target: RevealTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mode = target.mode();
        self.reveal = Some(target);
        self.change_mode(mode, window, cx);
        cx.notify();
    }

    /// The active mode's workspace view (tests downcast it).
    pub fn mode_view(&self) -> Option<AnyView> {
        self.view.as_ref().map(|(_, v, _)| v.clone())
    }

    pub fn take_reveal(&mut self, mode: ModeId) -> Option<RevealTarget> {
        match &self.reveal {
            Some(t) if t.mode() == mode => self.reveal.take(),
            _ => None,
        }
    }

    fn ensure_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(&self.view, Some((m, ..)) if *m == self.doc.mode) {
            return;
        }
        let shell = cx.entity();
        let mode = self.doc.mode;
        let body = modes::build_view(mode, &shell, window, cx);
        let sidebar = modes::build_sidebar(mode, &shell, window, cx);
        self.view = Some((mode, body, sidebar));
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        let result = self.store.update(cx, |s, cx| s.undo(cx));
        if let Err(e) = result {
            self.error = Some(e);
        }
        cx.notify();
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        let result = self.store.update(cx, |s, cx| s.redo(cx));
        if let Err(e) = result {
            self.error = Some(e);
        }
        cx.notify();
    }

    /// Saves in place, or asks for a destination first (Save As, or never saved).
    pub fn save(&mut self, force_destination: bool, cx: &mut Context<Self>) {
        self.error = None;
        self.notice = None;
        let label = self.label.to_string();
        if force_destination || self.doc.path.is_none() {
            self.pending = Some("Saving document…".into());
            let name = format!("{}.ixt", self.doc.name);
            let pick = dialogs::save_file(cx, "Save document", &name, &[dialogs::IXT]);
            task::after(cx, pick, move |this, path, cx| match path {
                None => {
                    this.pending = None;
                    this.notice = Some("Save canceled.".into());
                    cx.notify();
                }
                Some(path) => this.run_save(move || {
                    crate::save_document_as(label, path.to_string_lossy().into_owned())
                }, cx),
            });
        } else {
            self.run_save(move || crate::save_document(label), cx);
        }
        cx.notify();
    }

    fn run_save(
        &mut self,
        work: impl FnOnce() -> Result<SessionState, AppError> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.pending = Some("Saving document…".into());
        task::spawn_call(cx, work, |this, result, cx| {
            this.pending = None;
            match result {
                Ok(state) => this.apply_session(&state, cx),
                Err(e) => this.error = Some(e),
            }
            cx.notify();
        });
    }

    /// Flushes autosave, asks before discarding unsaved changes, then closes.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        let label = self.label.to_string();
        let eligible = self.autosave.read(cx).eligible;
        let dirty = self.doc.dirty;
        self.pending = Some("Closing document…".into());
        cx.notify();
        task::spawn_call(
            cx,
            move || {
                if dirty && eligible {
                    crate::manager::autosave_document(label).ok()
                } else {
                    None
                }
            },
            move |this, flushed, cx| {
                let dirty = flushed.as_ref().map_or(dirty, |s| s.dirty);
                if let Some(state) = &flushed {
                    this.apply_session(state, cx);
                }
                if !dirty {
                    this.finish_close(false, cx);
                    return;
                }
                let ask = dialogs::confirm(
                    cx,
                    "Close document",
                    "Discard unsaved changes and close this document?",
                );
                task::after(cx, ask, |this, discard, cx| {
                    if discard {
                        this.finish_close(true, cx);
                    } else {
                        this.pending = None;
                        cx.notify();
                    }
                });
            },
        );
    }

    fn finish_close(&mut self, discard: bool, cx: &mut Context<Self>) {
        let label = self.label.to_string();
        task::spawn_call(
            cx,
            move || crate::close_document(label, discard),
            |this, result, cx| match result {
                Ok(()) => cx.emit(ShellEvent::Closed),
                Err(e) => {
                    this.pending = None;
                    this.error = Some(e);
                    cx.notify();
                }
            },
        );
    }

    /// The error banner shows a command error, else an undismissed store error.
    fn shown_error(&self, cx: &App) -> Option<AppError> {
        if let Some(e) = &self.error {
            return Some(e.clone());
        }
        let store_error = self.store.read(cx).error.clone()?;
        let key = format!("{}:{}", store_error.code, store_error.message);
        (self.dismissed_store_error.as_deref() != Some(key.as_str())).then_some(store_error)
    }

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let busy = self.pending.is_some();
        let doc = &self.doc;
        let brand = h_flex()
            .justify_between()
            .items_center()
            .px_3()
            .py_2()
            .child(
                div()
                    .px_2()
                    .rounded_md()
                    .bg(gpui_kit::white())
                    .text_color(gpui_kit::black())
                    .font_weight(FontWeight::BOLD)
                    .child("ix"),
            )
            .child(
                h_flex()
                    .gap_1()
                    .when(!doc.runtime_only, |el| {
                        el.child(
                            Button::new("save-project")
                                .ghost()
                                .small()
                                .icon(Icon::new(IconName::Save))
                                .tooltip("Save project")
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, _, cx| this.save(false, cx))),
                        )
                        .child(
                            Button::new("save-project-as")
                                .ghost()
                                .small()
                                .icon(Icon::new(IconName::SaveAll))
                                .tooltip("Save project as")
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, _, cx| this.save(true, cx))),
                        )
                        .child(
                            Button::new(ModeId::App.button_id())
                                .ghost()
                                .small()
                                .icon(Icon::new(ModeId::App.icon()))
                                .tooltip(ModeId::App.label())
                                .selected(doc.mode == ModeId::App)
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.change_mode(ModeId::App, window, cx)
                                })),
                        )
                    })
                    .child(
                        Button::new("close-project")
                            .ghost()
                            .small()
                            .icon(Icon::new(IconName::X))
                            .tooltip("Close project")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    ),
            );
        let label = v_flex()
            .px_3()
            .py_2()
            .gap_0p5()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("PROJECT"),
            )
            .child(widgets::styled_text("document-name", doc.name.clone(), |d| {
                d.font_weight(FontWeight::SEMIBOLD)
            }))
            .child(widgets::styled_text(
                "document-status",
                if doc.path.is_some() {
                    "Saved archive"
                } else {
                    "Not saved yet"
                },
                |d| d.text_xs().text_color(muted),
            ));
        // Settings is an icon button beside Save, so the list skips it.
        let modes = ModeId::ALL.into_iter().filter(|m| *m != ModeId::App);
        let mode_switch = v_flex().px_2().gap_0p5().children(modes.map(|mode| {
            let active = self.doc.mode == mode;
            Button::new(mode.button_id())
                .ghost()
                .small()
                .w_full()
                .start_label(Some(Icon::new(mode.icon())), mode.label())
                .selected(active)
                .disabled(busy)
                .on_click(cx.listener(move |this, _, window, cx| this.change_mode(mode, window, cx)))
        }));
        let sidebar = self.view.as_ref().and_then(|(_, _, s)| s.clone());
        v_flex()
            .w(px(250.))
            .h_full()
            .flex_none()
            .border_r_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(brand)
            .when(!self.doc.runtime_only, |el| el.child(label).child(mode_switch))
            .child(
                div()
                    .id("mode-sidebar")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when_some(sidebar, |el, s| el.child(s)),
            )
            .child(self.render_save_status(cx))
    }

    fn render_save_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let autosave = self.autosave.read(cx);
        let has_path = self.doc.path.is_some();
        let text = match autosave.status {
            SaveStatus::Saving => "Saving…".to_string(),
            SaveStatus::Dirty if has_path => "Unsaved changes".to_string(),
            SaveStatus::Dirty => "Unsaved changes. Save once to turn on autosave".to_string(),
            SaveStatus::Clean => "All changes saved".to_string(),
            SaveStatus::Saved => match &autosave.last_saved_at {
                Some(at) => format!("Saved at {}", saved_time(at)),
                None => "All changes saved".to_string(),
            },
            SaveStatus::Error => format!(
                "Save failed: {}",
                autosave
                    .error
                    .as_ref()
                    .map_or("unknown error", |e| e.message.as_str())
            ),
        };
        let retry = autosave.status == SaveStatus::Error && autosave.eligible;
        let stuck = autosave.status == SaveStatus::Error && !autosave.eligible;
        let theme = cx.theme();
        v_flex()
            .px_3()
            .py_2()
            .gap_1()
            .border_t_1()
            .border_color(theme.border)
            .text_xs()
            .child(widgets::text("save-status", text))
            .when(retry, |el| {
                el.child(
                    Button::new("retry-save")
                        .link()
                        .xsmall()
                        .label("Retry save")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.autosave.update(cx, |a, cx| a.flush(cx));
                        })),
                )
            })
            .when(stuck, |el| {
                el.child(div().child("Use Save As or reopen the file to continue."))
            })
    }

    fn render_ribbon(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let undo_tip = store
            .undo_label()
            .map_or("Undo (Ctrl+Z)".to_string(), |l| format!("Undo {l} (Ctrl+Z)"));
        let redo_tip = store
            .redo_label()
            .map_or("Redo (Ctrl+Shift+Z)".to_string(), |l| {
                format!("Redo {l} (Ctrl+Shift+Z)")
            });
        let (can_undo, can_redo) = (store.can_undo(), store.can_redo());
        let current = self.doc.mode;
        let shell = cx.entity();
        h_flex()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("view-menu")
                    .outline()
                    .small()
                    .label(format!("{} view", current.label()))
                    .dropdown_menu(move |menu, _, _| {
                        let mut menu = menu.label("Switch view");
                        for mode in ModeId::ALL {
                            let shell = shell.clone();
                            menu = menu.item(
                                PopupMenuItem::new(format!("{} view", mode.label()))
                                    .checked(mode == current)
                                    .on_click(move |_, window, cx| {
                                        shell.update(cx, |s, cx| s.change_mode(mode, window, cx));
                                    }),
                            );
                        }
                        menu
                    }),
            )
            .child(
                Button::new("undo")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Undo2))
                    .label("Undo")
                    .tooltip(undo_tip)
                    .disabled(!can_undo)
                    .on_click(cx.listener(|this, _, _, cx| this.undo(cx))),
            )
            .child(
                Button::new("redo")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Redo2))
                    .label("Redo")
                    .tooltip(redo_tip)
                    .disabled(!can_redo)
                    .on_click(cx.listener(|this, _, _, cx| this.redo(cx))),
            )
    }
}

/// "Saved at" shows the local wall-clock time of an RFC 3339 timestamp.
fn saved_time(iso: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|t| t.with_timezone(&chrono::Local).format("%H:%M:%S").to_string())
        .unwrap_or_else(|_| iso.to_string())
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_view(window, cx);
        let title = format!(
            "{}{} — ixtable",
            self.doc.name,
            if self.doc.dirty { " •" } else { "" }
        );
        window.set_window_title(&title);
        let body = self.view.as_ref().map(|(_, b, _)| b.clone());
        let shown_error = self.shown_error(cx);
        let theme = cx.theme();
        h_flex()
            .id("shell")
            .key_context("Shell")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Undo, _, cx| this.undo(cx)))
            .on_action(cx.listener(|this, _: &Redo, _, cx| this.redo(cx)))
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(self.render_sidebar(cx))
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .when(!self.doc.runtime_only, |el| el.child(self.render_ribbon(cx)))
                    .when_some(self.pending.clone(), |el, p| {
                        el.child(widgets::progress_line(&p, cx))
                    })
                    .when_some(shown_error, |el, e| {
                        let entity = cx.entity();
                        el.child(div().p_2().child(widgets::error_banner(
                            "error",
                            &e,
                            Some(Box::new(move |_, cx| {
                                entity.update(cx, |this, cx| {
                                    this.error = None;
                                    if let Some(s) = this.store.read(cx).error.clone() {
                                        this.dismissed_store_error =
                                            Some(format!("{}:{}", s.code, s.message));
                                    }
                                    cx.notify();
                                })
                            })),
                            cx,
                        )))
                    })
                    .when_some(self.notice.clone(), |el, n| {
                        let entity = cx.entity();
                        el.child(div().p_2().child(widgets::notice_banner(
                            &n,
                            move |_, cx| {
                                entity.update(cx, |this, cx| {
                                    this.notice = None;
                                    cx.notify();
                                })
                            },
                            cx,
                        )))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .when_some(body, |el, b| el.child(b)),
                    ),
            )
            .test_support()
    }
}

impl Shell {
    /// Opens a file the OS asked to open over this document. Unsaved changes are flushed
    /// by autosave when possible, otherwise the user confirms discarding them. A runtime
    /// bundle closes the document; the start screen then takes the request.
    pub fn open_request(&mut self, path: String, cx: &mut Context<Self>) {
        let label = self.label.to_string();
        let eligible = self.autosave.read(cx).eligible;
        let dirty = self.doc.dirty;
        let name = self.doc.name.clone();
        let target = std::path::Path::new(&path)
            .file_name()
            .map_or(path.clone(), |n| n.to_string_lossy().into_owned());
        task::spawn_call(
            cx,
            move || {
                if dirty && eligible {
                    crate::manager::autosave_document(label).ok()
                } else {
                    None
                }
            },
            move |this, flushed, cx| {
                let dirty = flushed.as_ref().map_or(dirty, |s| s.dirty);
                if let Some(state) = &flushed {
                    this.apply_session(state, cx);
                }
                let target_name = target.clone();
                let proceed = move |this: &mut Self, cx: &mut Context<Self>| {
                    this.pending = Some(format!("Opening {target}…"));
                    this.error = None;
                    let label = this.label.to_string();
                    if crate::ui::is_runtime_bundle(&path) {
                        task::spawn_call(
                            cx,
                            move || crate::close_document(label, true),
                            move |_, _, cx| cx.emit(ShellEvent::Closed),
                        );
                        return;
                    }
                    task::spawn_call(
                        cx,
                        move || crate::open_document(label, path),
                        |this, result, cx| match result {
                            Ok(state) => cx.emit(ShellEvent::Opened(state)),
                            Err(e) => {
                                this.pending = None;
                                this.error = Some(e);
                                cx.notify();
                            }
                        },
                    );
                };
                if !dirty {
                    proceed(this, cx);
                    return;
                }
                let ask = dialogs::confirm(
                    cx,
                    "Open document",
                    &format!("Discard unsaved changes in {name} and open {target_name}?"),
                );
                task::after(cx, ask, move |this, discard, cx| {
                    if discard {
                        proceed(this, cx);
                    } else {
                        this.notice =
                            Some(format!("Kept {name} open; {target_name} was not opened."));
                        cx.notify();
                    }
                });
            },
        );
    }
}
