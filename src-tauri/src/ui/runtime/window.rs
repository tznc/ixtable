//! The Run App window: the application as its users see it, in an OS window of its own
//! beside Studio. Studio opens it from the "Run App" split button (`run_app_button`): the
//! button opens the window as the owner (full access), or brings it to the front and
//! reloads its page with the latest definitions, and its
//! menu runs the app as one of the document's roles. One window per document; it closes
//! with the document.
//!
//! The window shares the document's session with Studio, so Rust enforces the role it
//! previews only while it is the active window. Studio keeps full access when it is in
//! front.
//!
//! Test ids: `run-app` (the split button) with `run-app-open` (its action half) and
//! `popup` (its menu caret), `run-app-window` (the window's root).

use super::{RunBody, Runtime};
use crate::archive::DocumentConfig;
use crate::ui::{
    prelude::*,
    shell::{Shell, ShellEvent},
    windows::{self, WindowSpec},
};
use gpui_kit::component::{button::DropdownButton, menu::PopupMenuItem};
use std::collections::HashMap;

/// Label of the owner entry in role pickers (no role: full access).
pub const OWNER: &str = "Owner (default)";

/// The open Run App windows, by document label.
#[derive(Default)]
struct RunApps(HashMap<String, (AnyWindowHandle, WeakEntity<RunAppWindow>)>);
impl Global for RunApps {}

/// The role picker's options: the owner (value "") and then every role, by id.
pub fn role_options(config: &DocumentConfig) -> Vec<(String, String)> {
    std::iter::once((String::new(), OWNER.to_string()))
        .chain(config.roles.iter().map(|r| (r.id.clone(), r.name.clone())))
        .collect()
}

/// The Run App window of document `label`, when open.
pub fn find(label: &str, cx: &App) -> Option<(AnyWindowHandle, Entity<RunAppWindow>)> {
    let (handle, view) = cx.try_global::<RunApps>()?.0.get(label)?;
    Some((*handle, view.upgrade()?))
}

/// The runtime of the open Run App window of `label`.
pub fn runtime(label: &str, cx: &App) -> Option<Entity<Runtime>> {
    find(label, cx).map(|(_, view)| view.read(cx).body.read(cx).runtime().clone())
}

/// Brings the Run App window to the front, opening it as the owner when it is closed.
pub fn open(shell: &Entity<Shell>, cx: &mut App) {
    show(shell, None, cx);
}

/// Runs the app as `role_id` (None: the owner) in the Run App window, opening it if needed.
pub fn run_as(shell: &Entity<Shell>, role_id: Option<String>, cx: &mut App) {
    show(shell, Some(role_id), cx);
}

// Deferred: the window's views read the shell, which the caller may be updating.
fn show(shell: &Entity<Shell>, role: Option<Option<String>>, cx: &mut App) {
    let shell = shell.clone();
    cx.defer(move |cx| {
        let (label, name) = {
            let s = shell.read(cx);
            (s.label().to_string(), s.doc.name.clone())
        };
        if let Some((handle, view)) = find(&label, cx) {
            // Running again shows the latest definitions.
            let runtime = view.read(cx).body.read(cx).runtime().clone();
            match role {
                Some(role) => runtime.update(cx, |r, cx| r.set_role_id(role, cx)),
                None => runtime.update(cx, |r, cx| r.reload(cx)),
            }
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            return;
        }
        let spec = WindowSpec { key: format!("run-app-{label}"), title: title(&name), size: (1100.0, 760.0) };
        let opener = shell.clone();
        let opened = windows::open(
            spec,
            move |window, cx| cx.new(|cx| RunAppWindow::new(opener, role.flatten(), window, cx)),
            cx,
        );
        if let Err(e) = opened {
            shell.update(cx, |s, cx| {
                s.set_error(Some(crate::manager::AppError::new("WINDOW", format!("Could not open Run App: {e}"))), cx)
            });
        }
    });
}

fn title(name: &str) -> String {
    format!("{name} — Run App")
}

/// The Run App window's root view.
pub struct RunAppWindow {
    shell: Entity<Shell>,
    body: Entity<RunBody>,
    _subscriptions: Vec<Subscription>,
}

impl RunAppWindow {
    pub fn new(shell: Entity<Shell>, role_id: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let label = shell.read(cx).label().to_string();
        let body = cx.new(|cx| RunBody::new(shell.clone(), window, cx));
        if role_id.is_some() {
            let runtime = body.read(cx).runtime().clone();
            runtime.update(cx, |r, cx| r.set_role_id(role_id, cx));
        }
        let handle = window.window_handle();
        let weak = cx.entity().downgrade();
        cx.default_global::<RunApps>().0.insert(label.clone(), (handle, weak));
        cx.on_release(move |_, cx| {
            let apps = cx.default_global::<RunApps>();
            if apps.0.get(&label).is_some_and(|(h, _)| *h == handle) {
                apps.0.remove(&label);
            }
        })
        .detach();
        let subscriptions = vec![
            // The window goes with its document.
            cx.subscribe_in(&shell, window, |_, _, _: &ShellEvent, window, _| window.remove_window()),
            cx.observe(&shell, |_, _, cx| cx.notify()),
            cx.observe_window_activation(window, |this, window, cx| {
                let active = window.is_window_active();
                let runtime = this.body.read(cx).runtime().clone();
                runtime.update(cx, |r, _| r.set_enforced(active));
            }),
        ];
        Self { shell, body, _subscriptions: subscriptions }
    }

    pub fn body(&self) -> &Entity<RunBody> {
        &self.body
    }
}

impl Render for RunAppWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(&title(&self.shell.read(cx).doc.name));
        let theme = cx.theme();
        div()
            .id("run-app-window")
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(self.body.clone())
    }
}

/// Studio's "Run App" split button: the action opens the Run App window (or brings it
/// to the front); the menu runs the app as the owner or as a role. The shell renders it, so
/// it gets the shell's label and config rather than reading the shell.
pub fn run_app_button(
    shell: Entity<Shell>,
    label: &str,
    config: &DocumentConfig,
    disabled: bool,
) -> impl IntoElement {
    let options = role_options(config);
    let label = label.to_string();
    let (open_shell, menu_shell) = (shell.clone(), shell);
    DropdownButton::new("run-app")
        .w_full()
        .button(
            Button::new("run-app-open")
                .primary()
                .small()
                .flex_1()
                .icon(Icon::new(IconName::Play))
                .label("Run App")
                .tooltip("Open the app in its own window")
                .on_click(move |_, _, cx| open(&open_shell, cx)),
        )
        .menu_accessibility_label("Run as role")
        .disabled(disabled)
        .dropdown_menu(move |menu, _, cx| {
            // The role in force in the open window is checked.
            let current = runtime(&label, cx).map(|r| r.read(cx).role_id().unwrap_or_default());
            let mut menu = menu.label("Run as");
            for (value, name) in options.clone() {
                let shell = menu_shell.clone();
                let checked = current.as_deref() == Some(value.as_str());
                menu = menu.item(PopupMenuItem::new(name).checked(checked).on_click(move |_, _, cx| {
                    let role = (!value.is_empty()).then(|| value.clone());
                    run_as(&shell, role, cx);
                }));
            }
            menu
        })
}
