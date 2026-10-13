//! The seam between the Runtime and declarative actions (Automation).
//!
//! The Runtime runs actions for buttons and form events but does not interpret action
//! steps: the Automation port registers an [`ActionRunner`] at startup
//! ([`set_action_runner`]), and the Runtime builds an [`ActionContext`] whose
//! [`ActionHost`] carries what the TypeScript `ActionContext` callbacks did (navigate, set
//! state, confirm, notify, popups). Without a runner every action fails with a clear
//! message, so the Runtime still works for plain CRUD.

use crate::archive::DocumentConfig;
use serde_json::{Map, Value};
use std::sync::{Arc, RwLock};

/// A page an action asks the Runtime to open.
#[derive(Clone, Debug, PartialEq)]
pub struct NavigationTarget {
    /// `form`, `report`, `dashboard` or `table`.
    pub kind: String,
    pub id: String,
    pub mode: Option<String>,
    pub record_id: Option<Value>,
    pub params: Map<String, Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeTone {
    Info,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateScope {
    App,
    Form,
}

/// What an action run can ask of the UI. Methods run on the thread that runs the action
/// (a background thread), so implementations hand work to the UI thread themselves.
pub trait ActionHost: Send + Sync {
    fn navigate(&self, target: NavigationTarget);
    fn set_state(&self, scope: StateScope, key: &str, value: Value);
    /// Blocks until the user answers.
    fn confirm(&self, message: &str) -> bool;
    fn notify(&self, message: &str, tone: NoticeTone);
    /// Whether the active role may do `op` on `kind`/`id`; the default allows everything.
    fn authorize(&self, _kind: &str, _id: &str, _op: &str) -> bool {
        true
    }
    /// Announces that records changed.
    fn refresh(&self) {}
    /// Opens `target` as a popup form and waits for its result; None when no host shows
    /// popups (the caller then navigates instead).
    fn open_popup(&self, _target: &NavigationTarget) -> Option<Value> {
        None
    }
    /// Closes the topmost popup form with `value`; false when no popup is open.
    fn close_form(&self, _value: Value) -> bool {
        false
    }
}

/// Everything an action run reads. Mirrors the data fields of the TypeScript `ActionContext`;
/// the Automation port may add fields as it needs them.
#[derive(Clone)]
pub struct ActionContext {
    pub config: Arc<DocumentConfig>,
    /// The window label the action runs in (its backend session).
    pub window_label: String,
    pub record: Option<Map<String, Value>>,
    pub form: Option<Map<String, Value>>,
    pub app: Map<String, Value>,
    pub params: Option<Map<String, Value>>,
    /// Previous values of `record` (update triggers).
    pub old: Option<Map<String, Value>>,
    /// The current record as loaded (before unsaved edits).
    pub snapshot: Option<Map<String, Value>>,
    pub host: Arc<dyn ActionHost>,
}

/// How an action run ended.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActionResult {
    pub ok: bool,
    pub error: Option<String>,
    /// True when a confirm step was declined.
    pub cancelled: bool,
    /// True when a fail step ended the action (`error` is its message).
    pub aborted: bool,
    /// Values stored by steps with `storeAs`.
    pub results: Map<String, Value>,
}

impl ActionResult {
    pub fn success() -> Self {
        Self {
            ok: true,
            ..Default::default()
        }
    }
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(error.into()),
            ..Default::default()
        }
    }
}

/// Runs a declarative action by id. Implemented by the Automation port.
pub trait ActionRunner: Send + Sync {
    fn run_action(&self, action_id: &str, ctx: &ActionContext) -> ActionResult;
}

fn slot() -> &'static RwLock<Option<Arc<dyn ActionRunner>>> {
    static RUNNER: RwLock<Option<Arc<dyn ActionRunner>>> = RwLock::new(None);
    &RUNNER
}

/// Installs the runner the Runtime calls (the Automation port does this at startup).
pub fn set_action_runner(runner: Option<Arc<dyn ActionRunner>>) {
    *slot().write().unwrap_or_else(|e| e.into_inner()) = runner;
}

pub fn action_runner() -> Option<Arc<dyn ActionRunner>> {
    slot().read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Runs an action through the installed runner. Never panics: a missing runner is a failed
/// result.
pub fn run_action(action_id: &str, ctx: &ActionContext) -> ActionResult {
    match action_runner() {
        Some(runner) => runner.run_action(action_id, ctx),
        None => ActionResult::failure("Actions are not available in this build."),
    }
}

/// A host that records what an action asked for. Used by tests and by headless callers.
#[derive(Default)]
pub struct RecordingHost {
    pub navigated: std::sync::Mutex<Vec<NavigationTarget>>,
    pub states: std::sync::Mutex<Vec<(StateScope, String, Value)>>,
    pub notes: std::sync::Mutex<Vec<(String, NoticeTone)>>,
    pub answer: std::sync::atomic::AtomicBool,
    pub denied: std::sync::atomic::AtomicBool,
}

impl RecordingHost {
    pub fn new(confirm_answer: bool) -> Self {
        Self {
            answer: confirm_answer.into(),
            ..Default::default()
        }
    }
    pub fn notes(&self) -> Vec<String> {
        self.notes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(m, _)| m.clone())
            .collect()
    }
}

impl ActionHost for RecordingHost {
    fn navigate(&self, target: NavigationTarget) {
        self.navigated.lock().unwrap_or_else(|e| e.into_inner()).push(target);
    }
    fn set_state(&self, scope: StateScope, key: &str, value: Value) {
        self.states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((scope, key.to_string(), value));
    }
    fn confirm(&self, _message: &str) -> bool {
        self.answer.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn notify(&self, message: &str, tone: NoticeTone) {
        self.notes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((message.to_string(), tone));
    }
    fn authorize(&self, _kind: &str, _id: &str, _op: &str) -> bool {
        !self.denied.load(std::sync::atomic::Ordering::SeqCst)
    }
}
