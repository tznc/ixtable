//! The environment an action runs in: the record it acts on, the host that handles
//! UI effects, the role check and the trigger identity. Port of the `ActionContext`
//! of `src/automation/runner.ts` and of `src/automation/context.ts`.

use crate::archive::DocumentConfig;
use crate::authz::Op;
use crate::data::{DataValue, NamedValue};
use crate::trigger_auth::TriggerWrite;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Map, Value};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Info,
    Error,
}

impl Tone {
    pub fn from_key(key: &str) -> Self {
        if key == "error" {
            Tone::Error
        } else {
            Tone::Info
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Tone::Info => "info",
            Tone::Error => "error",
        }
    }
}

/// Where a navigation step wants to go.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigationTarget {
    /// `form`, `report`, `dashboard` or `table`.
    pub kind: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Map<String, Value>>,
}

impl NavigationTarget {
    pub fn new(kind: &str, id: &str) -> Self {
        Self {
            kind: kind.into(),
            id: id.into(),
            mode: None,
            record_id: None,
            params: None,
        }
    }
}

/// Handles the effects of steps that touch the UI. Methods block the calling thread
/// (a background worker), so an implementation that needs the UI thread hands the work
/// over and waits. Navigation and state changes fail only for hosts that cannot do them
/// (background jobs).
pub trait ActionHost: Send + Sync {
    fn navigate(&self, target: &NavigationTarget) -> Result<(), String>;
    fn set_state(&self, scope: &str, key: &str, value: &Value) -> Result<(), String>;
    fn confirm(&self, message: &str) -> Result<bool, String>;
    fn notify(&self, message: &str, tone: Tone);
    /// Called once an action that wrote records finished.
    fn refresh(&self) {}
    /// Shows a form in a popup and waits for it to close. `Ok(None)` means no host
    /// handled it (the runner navigates to the form instead); `Some(Null)` is a dismissed popup.
    fn open_popup(&self, _target: &NavigationTarget) -> Result<Option<Value>, String> {
        Ok(None)
    }
    /// Closes the topmost popup form with `value`.
    fn close_form(&self, _value: &Value) {}
}

/// Answers whether the user's role may do `op` (`create`, `update`, `delete`, `read`,
/// `execute`) on an object (`table`, `query`, `action`, ...).
pub type Authorizer = Arc<dyn Fn(&str, &str, &str) -> bool + Send + Sync>;

/// The row a custom concurrency action was called for.
#[derive(Debug, Clone)]
pub struct CurrentRow {
    pub table: String,
    pub identity: Vec<DataValue>,
    pub expected: Vec<NamedValue>,
}

#[derive(Clone)]
pub struct ActionContext {
    /// The window label every backend call takes.
    pub label: String,
    pub config: Arc<DocumentConfig>,
    pub record: Option<Map<String, Value>>,
    pub form: Option<Map<String, Value>>,
    pub app: Map<String, Value>,
    pub params: Option<Map<String, Value>>,
    /// Previous values of `record` (update triggers).
    pub old: Option<Map<String, Value>>,
    pub host: Arc<dyn ActionHost>,
    pub authorize: Option<Authorizer>,
    /// Set when an app-mode trigger runs this action: record steps present it to Rust
    /// (with their step id) instead of the user's role.
    pub trigger_auth: Option<TriggerWrite>,
    /// Trigger nesting depth of this run; writes carry it so triggers can stop recursion.
    pub trigger_depth: u32,
    /// Clock for `today()` and `now()`.
    pub now: Option<DateTime<Utc>>,
    /// Set while a custom concurrency action runs: its record steps write to the store directly.
    pub direct_writes: bool,
    /// The current record as loaded (before unsaved edits); `expected` for a keyless current row.
    pub snapshot: Option<Map<String, Value>>,
    /// Original values a write of this row must match (custom actions: what the caller edited).
    pub current: Option<CurrentRow>,
    /// Set while a before-change trigger runs: `setField` steps collect the record's new
    /// field values here, and steps that write or touch the UI are refused.
    pub before_change: Option<Arc<Mutex<Map<String, Value>>>>,
}

impl ActionContext {
    pub fn new(label: &str, config: Arc<DocumentConfig>, host: Arc<dyn ActionHost>) -> Self {
        Self {
            label: label.into(),
            config,
            record: None,
            form: None,
            app: Map::new(),
            params: None,
            old: None,
            host,
            authorize: None,
            trigger_auth: None,
            trigger_depth: 0,
            now: None,
            direct_writes: false,
            snapshot: None,
            current: None,
            before_change: None,
        }
    }
}

/// What a caller (a trigger, a custom action) sets on the context an environment builds.
#[derive(Clone, Default)]
pub struct ContextBase {
    pub record: Option<Map<String, Value>>,
    pub old: Option<Map<String, Value>>,
    pub params: Option<Map<String, Value>>,
    pub trigger_depth: u32,
    pub before_change: Option<Arc<Mutex<Map<String, Value>>>>,
    pub trigger_auth: Option<TriggerWrite>,
    pub direct_writes: bool,
    pub snapshot: Option<Map<String, Value>>,
    pub current: Option<CurrentRow>,
}

/// What triggers, custom actions and the job worker need from the host application.
pub trait TriggerEnv: Send + Sync {
    /// The window label the triggers belong to.
    fn label(&self) -> &str;
    /// The document's current definitions.
    fn config(&self) -> Arc<DocumentConfig>;
    /// The running application's `app` state.
    fn app(&self) -> Map<String, Value> {
        Map::new()
    }
    /// Builds the interactive context sync triggers and custom actions run with. App-mode
    /// triggers (`trigger_auth` set) skip the role checks: Rust verifies their grant.
    fn context(&self, base: ContextBase) -> ActionContext;
}

/// The role check of a window's session (`authz`), as an `Authorizer`.
pub fn session_authorizer(label: &str) -> Authorizer {
    let label = label.to_string();
    Arc::new(move |kind, id, op| {
        let op = match op {
            "read" | "view" => Op::Read,
            "create" => Op::Create,
            "update" => Op::Update,
            "delete" => Op::Delete,
            "execute" => Op::Execute,
            _ => return false,
        };
        crate::authz::check(&label, kind, id, op).is_ok()
    })
}

/// The standard environment: definitions come from the window's session, UI effects go to `host`.
pub struct SessionEnv {
    label: String,
    host: Arc<dyn ActionHost>,
    app: Arc<dyn Fn() -> Map<String, Value> + Send + Sync>,
}

impl SessionEnv {
    pub fn new(label: &str, host: Arc<dyn ActionHost>) -> Self {
        Self {
            label: label.into(),
            host,
            app: Arc::new(|| Map::new()),
        }
    }

    pub fn with_app(mut self, app: Arc<dyn Fn() -> Map<String, Value> + Send + Sync>) -> Self {
        self.app = app;
        self
    }
}

impl TriggerEnv for SessionEnv {
    fn label(&self) -> &str {
        &self.label
    }
    fn config(&self) -> Arc<DocumentConfig> {
        Arc::new(
            crate::manager()
                .and_then(|m| m.config(&self.label))
                .unwrap_or_default(),
        )
    }
    fn app(&self) -> Map<String, Value> {
        (self.app)()
    }
    fn context(&self, base: ContextBase) -> ActionContext {
        let mut ctx = ActionContext::new(&self.label, self.config(), self.host.clone());
        ctx.app = self.app();
        if base.trigger_auth.is_none() {
            ctx.authorize = Some(session_authorizer(&self.label));
        }
        ctx.record = base.record;
        ctx.old = base.old;
        ctx.params = base.params;
        ctx.trigger_depth = base.trigger_depth;
        ctx.before_change = base.before_change;
        ctx.trigger_auth = base.trigger_auth;
        ctx.direct_writes = base.direct_writes;
        ctx.snapshot = base.snapshot;
        ctx.current = base.current;
        ctx
    }
}
