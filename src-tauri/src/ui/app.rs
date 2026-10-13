//! The window's root view: the start screen until a document opens, then the shell.
//! Port of src/App.tsx.

use super::{
    shell::{Shell, ShellEvent},
    start::{StartEvent, StartScreen},
};
use crate::manager::SessionState;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

pub enum Screen {
    Start(Entity<StartScreen>),
    Shell(Entity<Shell>),
}

pub struct AppView {
    label: SharedString,
    pub screen: Screen,
    /// A file the OS asked to open that the start screen still has to take (a runtime
    /// bundle requested while a document was open).
    pending_request: Option<String>,
    _subscription: Option<Subscription>,
}

impl AppView {
    pub fn new(label: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            label: label.to_string().into(),
            screen: Screen::Start(cx.new(|cx| StartScreen::new(label, None, window, cx))),
            pending_request: None,
            _subscription: None,
        };
        this.show_start(None, window, cx);
        this
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn shell(&self) -> Option<&Entity<Shell>> {
        match &self.screen {
            Screen::Shell(s) => Some(s),
            Screen::Start(_) => None,
        }
    }

    pub fn start(&self) -> Option<&Entity<StartScreen>> {
        match &self.screen {
            Screen::Start(s) => Some(s),
            Screen::Shell(_) => None,
        }
    }

    fn show_start(&mut self, notice: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        window.set_window_title("ixtable");
        let label = self.label.to_string();
        let screen = cx.new(|cx| StartScreen::new(&label, notice, window, cx));
        self._subscription = Some(cx.subscribe_in(
            &screen,
            window,
            |this, _, event: &StartEvent, window, cx| match event {
                StartEvent::Opened(state) => this.show_shell(state.clone(), window, cx),
            },
        ));
        if let Some(path) = self.pending_request.take() {
            screen.update(cx, |s, cx| s.open_request(path, cx));
        }
        self.screen = Screen::Start(screen);
        cx.notify();
    }

    fn show_shell(&mut self, state: SessionState, window: &mut Window, cx: &mut Context<Self>) {
        let label = self.label.to_string();
        let store = match crate::ui::store::ConfigStore::load(&label) {
            Ok(store) => store,
            Err(e) => {
                if let Screen::Start(screen) = &self.screen {
                    screen.update(cx, |s, cx| s.set_error(e, cx));
                }
                return;
            }
        };
        let shell = cx.new(|cx| Shell::new(&label, state, store, window, cx));
        self._subscription = Some(cx.subscribe_in(
            &shell,
            window,
            |this, _, event: &ShellEvent, window, cx| match event {
                ShellEvent::Closed => this.show_start(None, window, cx),
                ShellEvent::Opened(state) => this.show_shell(state.clone(), window, cx),
            },
        ));
        self.screen = Screen::Shell(shell);
        cx.notify();
    }

    /// Opens files the OS asked to open (launch arguments, later launches).
    pub fn open_request(&mut self, path: String, cx: &mut Context<Self>) {
        match &self.screen {
            Screen::Start(screen) => screen.update(cx, |s, cx| s.open_request(path, cx)),
            Screen::Shell(shell) => {
                if super::is_runtime_bundle(&path) {
                    self.pending_request = Some(path.clone());
                }
                shell.update(cx, |s, cx| s.open_request(path, cx))
            }
        }
    }
}

impl Render for AppView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body: AnyView = match &self.screen {
            Screen::Start(s) => s.clone().into(),
            Screen::Shell(s) => s.clone().into(),
        };
        div()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(body)
    }
}
