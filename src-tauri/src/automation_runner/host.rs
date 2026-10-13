//! Hosts for the UI effects of steps. The Runtime plugs navigation, state and popup
//! handling in through `UiHandlers` (one set per window); everything else here works
//! without a UI: the interactive `SessionHost`, the `HeadlessHost` of background jobs and
//! the `RecordingHost` of test runs.

use super::context::{ActionHost, NavigationTarget, Tone};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// What the running application (the Runtime view) does for navigation steps. Every method
/// may block the calling thread, which is never the UI thread. The defaults decline.
pub trait UiHandlers: Send + Sync {
    /// Follows a navigation; false when nothing could (the user is told to open the Runtime).
    fn navigate(&self, _target: &NavigationTarget) -> bool {
        false
    }
    /// Applies a `setState` step; false when nothing handled it.
    fn set_state(&self, _scope: &str, _key: &str, _value: &Value) -> bool {
        false
    }
    /// Shows `target` as a popup form and waits for it to close; `None` when there is no
    /// popup host, `Some(Null)` when the user dismissed it.
    fn open_popup(&self, _target: &NavigationTarget) -> Option<Value> {
        None
    }
    /// Closes the topmost popup form with `value`; false when no popup is open.
    fn close_form(&self, _value: &Value) -> bool {
        false
    }
    /// The `app` scope of expressions.
    fn app_state(&self) -> Map<String, Value> {
        Map::new()
    }
}

type Handlers = HashMap<String, (u64, Arc<dyn UiHandlers>)>;

fn registry() -> std::sync::MutexGuard<'static, (u64, Handlers)> {
    static REGISTRY: OnceLock<Mutex<(u64, Handlers)>> = OnceLock::new();
    REGISTRY
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Removes the handlers when dropped (if they are still the installed ones).
#[must_use = "dropping the registration removes the handlers"]
pub struct HandlersRegistration {
    label: String,
    id: u64,
}

impl Drop for HandlersRegistration {
    fn drop(&mut self) {
        let mut reg = registry();
        if reg.1.get(&self.label).is_some_and(|(id, _)| *id == self.id) {
            reg.1.remove(&self.label);
        }
    }
}

/// Installs the UI handlers of `label` (one set at a time; the latest wins).
pub fn install_handlers(label: &str, handlers: Arc<dyn UiHandlers>) -> HandlersRegistration {
    let mut reg = registry();
    reg.0 += 1;
    let id = reg.0;
    reg.1.insert(label.to_string(), (id, handlers));
    HandlersRegistration {
        label: label.to_string(),
        id,
    }
}

fn handlers_of(label: &str) -> Option<Arc<dyn UiHandlers>> {
    registry().1.get(label).map(|(_, h)| h.clone())
}

/// The `app` state the installed handlers report (empty when none are installed).
pub fn app_state(label: &str) -> Map<String, Value> {
    handlers_of(label).map(|h| h.app_state()).unwrap_or_default()
}

/// The interactive host: navigation and state go to the installed `UiHandlers`; an
/// unhandled navigation is reported through `notify`. Confirmations and messages go to
/// the functions the application supplies.
pub struct SessionHost {
    label: String,
    confirm: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    notify: Arc<dyn Fn(&str, Tone) + Send + Sync>,
    refresh: Arc<dyn Fn() + Send + Sync>,
}

impl SessionHost {
    pub fn new(
        label: &str,
        confirm: Arc<dyn Fn(&str) -> bool + Send + Sync>,
        notify: Arc<dyn Fn(&str, Tone) + Send + Sync>,
        refresh: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self {
            label: label.into(),
            confirm,
            notify,
            refresh,
        }
    }
}

impl ActionHost for SessionHost {
    fn navigate(&self, target: &NavigationTarget) -> Result<(), String> {
        let handled = handlers_of(&self.label).is_some_and(|h| h.navigate(target));
        if !handled {
            (self.notify)(
                &format!(
                    "This action opens a {}. Open it in the Runtime to follow navigation.",
                    target.kind
                ),
                Tone::Info,
            );
        }
        Ok(())
    }
    fn set_state(&self, scope: &str, key: &str, value: &Value) -> Result<(), String> {
        if let Some(h) = handlers_of(&self.label) {
            h.set_state(scope, key, value);
        }
        Ok(())
    }
    fn confirm(&self, message: &str) -> Result<bool, String> {
        Ok((self.confirm)(message))
    }
    fn notify(&self, message: &str, tone: Tone) {
        (self.notify)(message, tone)
    }
    fn refresh(&self) {
        (self.refresh)()
    }
    fn open_popup(&self, target: &NavigationTarget) -> Result<Option<Value>, String> {
        Ok(handlers_of(&self.label).and_then(|h| h.open_popup(target)))
    }
    fn close_form(&self, value: &Value) {
        if let Some(h) = handlers_of(&self.label) {
            h.close_form(value);
        }
    }
}

/// A message a background job's action showed.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct JobMessage {
    pub text: String,
    pub tone: String,
}

/// The host of background jobs: there is no user, so navigation, confirmation and
/// form state fail clearly. Messages are collected into the job's log.
#[derive(Default)]
pub struct HeadlessHost {
    pub messages: Mutex<Vec<JobMessage>>,
}

impl HeadlessHost {
    pub fn messages(&self) -> Vec<JobMessage> {
        self.messages.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl ActionHost for HeadlessHost {
    fn navigate(&self, _: &NavigationTarget) -> Result<(), String> {
        Err("Navigation is not available in background jobs".into())
    }
    fn set_state(&self, scope: &str, _: &str, _: &Value) -> Result<(), String> {
        if scope == "form" {
            return Err("Form state is not available in background jobs".into());
        }
        Ok(())
    }
    fn confirm(&self, _: &str) -> Result<bool, String> {
        Err("Confirmation is not available in background jobs".into())
    }
    fn notify(&self, message: &str, tone: Tone) {
        self.messages
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(JobMessage {
                text: message.into(),
                tone: tone.key().into(),
            });
    }
}

/// A host that records what the action did. Test runs use it with a confirm function;
/// unit tests use it with a fixed answer.
pub struct RecordingHost {
    pub events: Mutex<Vec<Value>>,
    confirm: Arc<dyn Fn(&str) -> bool + Send + Sync>,
}

impl RecordingHost {
    pub fn new(confirm: Arc<dyn Fn(&str) -> bool + Send + Sync>) -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            confirm,
        }
    }
    pub fn answering(answer: bool) -> Self {
        Self::new(Arc::new(move |_| answer))
    }
    pub fn events(&self) -> Vec<Value> {
        self.events.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn push(&self, event: Value) {
        self.events.lock().unwrap_or_else(|e| e.into_inner()).push(event);
    }
}

impl ActionHost for RecordingHost {
    fn navigate(&self, target: &NavigationTarget) -> Result<(), String> {
        self.push(serde_json::json!(["navigate", target]));
        Ok(())
    }
    fn set_state(&self, scope: &str, key: &str, value: &Value) -> Result<(), String> {
        self.push(serde_json::json!(["state", scope, key, value]));
        Ok(())
    }
    fn confirm(&self, message: &str) -> Result<bool, String> {
        self.push(serde_json::json!(["confirm", message]));
        Ok((self.confirm)(message))
    }
    fn notify(&self, message: &str, tone: Tone) {
        self.push(serde_json::json!(["notify", message, tone.key()]));
    }
    fn refresh(&self) {
        self.push(serde_json::json!(["refresh"]));
    }
}
