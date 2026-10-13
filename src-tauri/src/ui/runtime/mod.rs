//! Run mode: the application as its users see it (port of `src/runtime/`). The form
//! renderer (`FormRenderer`) is also what the Design studio previews forms with and what
//! dashboards embed, so it works detached from Run mode (`RuntimeEnv::runtime` is `None`).
//!
//! Test ids: `rt-nav` ("Application navigation"), `rt-page` ("Application page"),
//! `nav:<item id>` (navigation buttons, selected = current page), `role:developer` /
//! `role:<id>` (the "Preview as role" picker), `rt-notice`, `back-crumb` ("Back to X"),
//! `list:<form name>` (a list region), `row:<n>` (list rows, "Open X"), `form:<title>`
//! (a record view region), and `field:<label>` inputs (see `field.rs`).

pub mod automation_bridge;
pub mod back;
pub mod body;
pub mod continuous;
pub mod export;
pub mod field;
pub mod handlers;
pub mod host;
pub mod list;
pub mod navbar;
pub mod popup;
pub mod record;
pub mod related;
pub mod roles_tab;
pub mod run_mode;
pub mod split;
pub mod stack;

#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod tests_events;
#[cfg(test)]
mod tests_forms;
#[cfg(test)]
mod tests_guards;
#[cfg(test)]
pub(crate) mod tests_misc;
#[cfg(test)]
mod tests_rbac;
#[cfg(test)]
mod tests_template;
#[cfg(test)]
mod tests_views;
#[cfg(test)]
mod tests_writes;
#[cfg(test)]
mod tests_keyboard;
#[cfg(test)]
mod tests_preview;

use crate::archive::DocumentConfig;
use crate::forms::actions::{NavigationTarget, NoticeTone};
use crate::forms::navigation::{RuntimePage, page_title, start_page};
use crate::forms::rbac::{self, find_role};
use crate::ui::{events::DocBus, store::ConfigStore};
use crate::ui::prelude::*;
use serde_json::{Map, Value, json};
use std::sync::{Arc, Mutex, mpsc::Sender};

pub use back::BackLink;
pub use run_mode::{RunBody, RunMode};
pub use stack::{FormRenderer, FormRendererProps};

/// Prefix of the element ids of a repeated body (empty for a single form).
pub type BodyKey = SharedString;

/// A message shown on the page or beside a form.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub message: String,
    pub tone: NoticeTone,
}

/// A popup form an action opened; it resolves with the form's returned value.
pub struct Popup {
    pub id: u64,
    pub target: NavigationTarget,
    reply: Option<Sender<Option<Value>>>,
}

pub enum RuntimeEvent {
    /// The page, role, state or notice changed.
    Changed,
}

/// Run mode's navigation, previewed role, app state and notice (port of `useRuntimeState`).
pub struct Runtime {
    label: SharedString,
    history: Vec<RuntimePage>,
    visit: u64,
    preview_role: Option<String>,
    pub notice: Option<Notice>,
    state: Arc<Mutex<Map<String, Value>>>,
    pub popups: Vec<Popup>,
    next_popup: u64,
}
impl EventEmitter<RuntimeEvent> for Runtime {}

impl Runtime {
    pub fn new(label: &str) -> Self {
        Self {
            label: label.to_string().into(),
            history: vec![],
            visit: 0,
            preview_role: None,
            notice: None,
            state: Arc::default(),
            popups: vec![],
            next_popup: 1,
        }
    }

    /// The page trail's current page; None before anything was chosen.
    pub fn page(&self) -> Option<&RuntimePage> {
        self.history.last()
    }

    /// Bumped on every navigation, so choosing the open page again starts it fresh.
    pub fn visit(&self) -> u64 {
        self.visit
    }

    /// The role in force: the one ixtable Cloud assigned, else the previewed one. None is the
    /// developer (full access).
    pub fn role_id(&self) -> Option<String> {
        rbac::assigned_runtime_role()
            .map(|assigned| assigned.role.id)
            .or_else(|| self.preview_role.clone())
    }

    /// A trail rooted at a navigation item goes back no further than that item.
    pub fn can_go_back(&self) -> bool {
        self.history.len() > 1 || (self.history.len() == 1 && self.history[0].nav_id.is_none())
    }

    /// The page `back` returns to (for its label).
    pub fn previous(&self, config: &DocumentConfig) -> Option<RuntimePage> {
        if !self.can_go_back() {
            return None;
        }
        if self.history.len() > 1 {
            return self.history.get(self.history.len() - 2).cloned();
        }
        start_page(config, self.role_id().as_deref())
    }

    pub fn previous_title(&self, config: &DocumentConfig) -> Option<String> {
        self.previous(config).map(|page| page_title(config, &page))
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(RuntimeEvent::Changed);
        cx.notify();
    }

    /// Choosing a navigation item starts a new trail; pages opened from content stack on it.
    pub fn navigate(&mut self, page: RuntimePage, cx: &mut Context<Self>) {
        self.notice = None;
        self.visit += 1;
        if page.nav_id.is_some() {
            self.history = vec![page];
        } else {
            let keep = self.history.len().saturating_sub(49);
            self.history.drain(..keep);
            self.history.push(page);
        }
        self.changed(cx);
    }

    pub fn back(&mut self, cx: &mut Context<Self>) {
        self.visit += 1;
        self.history.pop();
        self.changed(cx);
    }

    /// Previews a role (None is the developer). Rust enforces the previewed role too, so a
    /// preview behaves like the installed runtime; if Rust refuses, the preview falls back to
    /// developer access and says why.
    pub fn set_role_id(&mut self, role_id: Option<String>, cx: &mut Context<Self>) {
        if rbac::assigned_runtime_role().is_some() {
            return;
        }
        self.notice = None;
        self.visit += 1;
        self.history.clear();
        rbac::set_previewed_role(&self.label, role_id.as_deref());
        self.preview_role = role_id.clone();
        if let Err(e) =
            crate::authz::set_runtime_role_preview(self.label.to_string(), role_id.clone())
        {
            self.notice = Some(Notice {
                message: format!("Could not preview this role: {}", e.message),
                tone: NoticeTone::Error,
            });
            if role_id.is_some() {
                self.preview_role = None;
                rbac::set_previewed_role(&self.label, None);
            }
        }
        self.changed(cx);
    }

    /// The app state, shared with the automation handlers that set it from other threads.
    pub fn shared_state(&self) -> Arc<Mutex<Map<String, Value>>> {
        self.state.clone()
    }

    pub fn set_app_state(&mut self, key: &str, value: Value, cx: &mut Context<Self>) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).insert(key.to_string(), value);
        self.changed(cx);
    }

    pub fn notify(&mut self, message: impl Into<String>, tone: NoticeTone, cx: &mut Context<Self>) {
        self.notice = Some(Notice { message: message.into(), tone });
        self.changed(cx);
    }

    pub fn clear_notice(&mut self, cx: &mut Context<Self>) {
        self.notice = None;
        self.changed(cx);
    }

    /// The name of the role in force.
    pub fn role_name(&self, config: &DocumentConfig) -> Option<String> {
        find_role(config, self.role_id().as_deref()).ok().flatten().map(|r| r.name)
    }

    /// The `app` expression scope: app state plus `user` and `role`.
    pub fn app(&self, config: &DocumentConfig) -> Map<String, Value> {
        let mut app = self.state.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let role_name = self.role_name(config);
        let user = match rbac::assigned_runtime_role() {
            Some(assigned) => {
                let mut user = Map::new();
                user.insert("name".into(), json!(assigned.user_name));
                if let Some(email) = assigned.user_email {
                    user.insert("email".into(), json!(email));
                }
                if let Some(id) = assigned.user_id {
                    user.insert("id".into(), json!(id));
                }
                Value::Object(user)
            }
            None => json!({
                "name": role_name.as_ref().map_or("Developer".to_string(), |n| format!("{n} preview")),
            }),
        };
        app.insert("user".into(), user);
        app.insert("role".into(), role_name.map_or(Value::Null, Value::String));
        app
    }

    /// Opens a popup form; `reply` receives its value when it closes.
    pub fn open_popup(
        &mut self,
        target: NavigationTarget,
        reply: Option<Sender<Option<Value>>>,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_popup;
        self.next_popup += 1;
        self.popups.push(Popup { id, target, reply });
        self.changed(cx);
    }

    /// Closes the popup `id` (the first close wins); false when it is already closed.
    pub fn close_popup_by_id(&mut self, id: u64, value: Option<Value>, cx: &mut Context<Self>) -> bool {
        let Some(index) = self.popups.iter().position(|p| p.id == id) else {
            return false;
        };
        let popup = self.popups.remove(index);
        if let Some(reply) = popup.reply {
            let _ = reply.send(value);
        }
        self.changed(cx);
        true
    }

    /// Closes the topmost popup with `value`; false when none is open.
    pub fn close_popup(&mut self, value: Option<Value>, cx: &mut Context<Self>) -> bool {
        let Some(popup) = self.popups.pop() else {
            return false;
        };
        if let Some(reply) = popup.reply {
            let _ = reply.send(value);
        }
        self.changed(cx);
        true
    }
}

impl Drop for Runtime {
    // Outside Run mode Rust must not keep enforcing a role preview: Studio modes run with
    // full access. A cloud session keeps its assigned role.
    fn drop(&mut self) {
        for popup in self.popups.drain(..) {
            if let Some(reply) = popup.reply {
                let _ = reply.send(None);
            }
        }
        if rbac::assigned_runtime_role().is_none() {
            rbac::set_previewed_role(&self.label, None);
            let _ = crate::authz::set_runtime_role_preview(self.label.to_string(), None);
        }
    }
}

/// A status or error line (`role="status"` / `role="alert"` in the web UI).
pub fn message(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    error: bool,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let color = if error { theme.danger } else { theme.muted_foreground };
    widgets::styled_text(id, text, move |d| d.text_sm().text_color(color))
}

/// What every runtime view needs: the window label, the config store, the document bus and,
/// in Run mode, the `Runtime` (None when a renderer is used outside Run mode).
#[derive(Clone)]
pub struct RuntimeEnv {
    pub label: SharedString,
    pub store: Entity<ConfigStore>,
    pub bus: Entity<DocBus>,
    pub runtime: Option<Entity<Runtime>>,
}

impl RuntimeEnv {
    pub fn config<'a>(&self, cx: &'a App) -> &'a DocumentConfig {
        self.store.read(cx).config()
    }

    /// True inside Run mode: navigation goes to the Runtime instead of stacking in the renderer.
    pub fn attached(&self) -> bool {
        self.runtime.is_some()
    }

    pub fn role_id(&self, cx: &App) -> Option<String> {
        match &self.runtime {
            Some(runtime) => runtime.read(cx).role_id(),
            None => None,
        }
    }

    /// The `app` expression scope (a detached developer outside Run mode).
    pub fn app(&self, cx: &App) -> Map<String, Value> {
        match &self.runtime {
            Some(runtime) => runtime.read(cx).app(self.config(cx)),
            None => {
                let mut app = Map::new();
                app.insert("user".into(), json!({"name": "Developer"}));
                app.insert("role".into(), Value::Null);
                app
            }
        }
    }

    pub fn can(&self, kind: &str, id: &str, op: rbac::Operation, cx: &App) -> bool {
        rbac::can(self.config(cx), self.role_id(cx).as_deref(), kind, id, op)
    }

    /// Announces committed record writes to the rest of the document.
    pub fn records_changed(&self, cx: &mut App) {
        DocBus::records_changed(&self.bus, cx);
    }

    /// A page-level message (Run mode only).
    pub fn notify(&self, message: impl Into<String>, tone: NoticeTone, cx: &mut App) {
        if let Some(runtime) = &self.runtime {
            let message = message.into();
            runtime.update(cx, |r, cx| r.notify(message, tone, cx));
        }
    }

    pub fn set_app_state(&self, key: &str, value: Value, cx: &mut App) {
        if let Some(runtime) = &self.runtime {
            runtime.update(cx, |r, cx| r.set_app_state(key, value, cx));
        }
    }
}
