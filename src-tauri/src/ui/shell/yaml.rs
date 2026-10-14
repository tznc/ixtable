//! Code-first view of DocumentConfig: applying YAML replaces the config (PRD §8).
//! Port of src/shell/YamlTab.tsx. Test ids: `config-yaml` (editor), `apply-yaml`,
//! `discard-edits`, `yaml-error`.

use super::Shell;
use crate::{
    manager::AppError,
    ui::{store::StoreEvent, task, toast::Toast, widgets},
};
use gpui_kit::component::{
    Disableable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Editor, EditorState, InputEvent},
    v_flex,
};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

pub struct YamlTab {
    shell: Entity<Shell>,
    pub editor: Entity<EditorState>,
    edited: bool,
    /// The YAML last loaded into the editor; any other text is an edit.
    loaded: String,
    loading: bool,
    busy: bool,
    error: Option<AppError>,
    _subscriptions: Vec<Subscription>,
}

impl YamlTab {
    pub fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| EditorState::new(window, cx).language("yaml"));
        let store = shell.read(cx).store.clone();
        let subscriptions = vec![
            cx.subscribe(&editor, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) && !this.loading {
                    this.edited = true;
                    cx.notify();
                }
            }),
            cx.subscribe_in(&store, window, |this, _, e: &StoreEvent, window, cx| {
                if matches!(e, StoreEvent::Changed) && !this.edited {
                    this.load(window, cx);
                }
            }),
        ];
        let mut this = Self {
            shell,
            editor,
            edited: false,
            loaded: String::new(),
            loading: false,
            busy: false,
            error: None,
            _subscriptions: subscriptions,
        };
        this.load(window, cx);
        this
    }

    fn label(&self, cx: &App) -> String {
        self.shell.read(cx).label().to_string()
    }

    /// Replaces the editor text with the current config as YAML.
    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let label = self.label(cx);
        task::spawn_call_in(
            window,
            cx,
            move || crate::read_document_config_yaml(label),
            |this, result, window, cx| {
                match result {
                    Ok(text) => {
                        this.loading = true;
                        this.loaded = text.clone();
                        this.editor.update(cx, |e, cx| e.set_value(text, window, cx));
                        this.loading = false;
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
    }

    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        let yaml = self.editor.read(cx).value().to_string();
        let label = self.label(cx);
        task::spawn_call_in(
            window,
            cx,
            move || crate::apply_document_config_yaml(label, yaml),
            |this, result, window, cx| {
                this.busy = false;
                match result {
                    Ok(state) => {
                        this.edited = false;
                        let shell = this.shell.clone();
                        shell.update(cx, |s, cx| s.apply_session(&state, cx));
                        let store = shell.read(cx).store.clone();
                        let reloaded = store.update(cx, |s, cx| s.reload(Some("Apply YAML"), cx));
                        match reloaded {
                            Ok(_) => {
                                this.edited = false;
                                shell.update(cx, |s, cx| s.toast(Toast::success("YAML applied."), cx));
                                this.load(window, cx);
                            }
                            Err(e) => this.error = Some(e),
                        }
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.edited = false;
        self.error = None;
        self.load(window, cx);
    }
}

impl Render for YamlTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let edited = self.edited || *self.editor.read(cx).value() != *self.loaded;
        let idle = !self.busy && edited;
        v_flex()
            .gap_3()
            .size_full()
            .child(widgets::section_heading("yaml-heading", "Configuration YAML"))
            .child(div().text_sm().child(
                "Every definition in this application as YAML. Applying replaces the whole \
                 configuration; Undo restores the previous one.",
            ))
            .child(
                div().h(px(420.)).child(
                    Editor::new(&self.editor)
                        .h_full()
                        .aria_label("Configuration YAML"),
                ),
            )
            .when_some(self.error.clone(), |el, e| {
                el.child(widgets::error_banner("yaml-error", &e, None, cx))
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("apply-yaml")
                            .primary()
                            .label("Apply YAML")
                            .disabled(!idle)
                            .on_click(cx.listener(|this, _, window, cx| this.apply(window, cx))),
                    )
                    .child(
                        Button::new("discard-edits")
                            .outline()
                            .label("Discard edits")
                            .disabled(!idle)
                            .on_click(cx.listener(|this, _, window, cx| this.discard(window, cx))),
                    ),
            )
    }
}
