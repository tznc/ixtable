//! The UI side of the Automation seam: a `UiHost` implements `forms::actions::ActionHost` for
//! an action that runs on a background thread. Requests that need the UI are queued and
//! handled on the UI thread; the ones that wait for an answer (confirm, popups, close form)
//! block the action thread on a channel until the UI replies.
//!
//! `run_action_job` runs one action: the work on the background executor, a poller that
//! hands queued requests to the owning view while it runs, and a completion callback.

use super::{Notice, Runtime};
use crate::forms::actions::{
    ActionContext, ActionHost, ActionResult, NavigationTarget, NoticeTone, StateScope,
};
use crate::forms::rbac::{self, Operation};
use crate::ui::{dialogs, prelude::*};
use serde_json::Value;
use std::sync::{
    Arc, Mutex,
    mpsc::{Sender, channel},
};
use std::time::Duration;

/// A request an action made of the UI.
pub enum HostEvent {
    Navigate(NavigationTarget),
    SetState(StateScope, String, Value),
    Notify(String, NoticeTone),
    Refresh,
    Confirm(String, Sender<bool>),
    Popup(NavigationTarget, Sender<Option<Value>>),
    Close(Value, Sender<bool>),
}

type Authorizer = Arc<dyn Fn(&str, &str, &str) -> bool + Send + Sync>;

#[derive(Clone)]
pub struct UiHost {
    events: Arc<Mutex<Vec<HostEvent>>>,
    authorizer: Authorizer,
    popups: bool,
}

impl UiHost {
    /// `popups`: whether this window can show popup forms (Run mode).
    pub fn new(config: crate::archive::DocumentConfig, role_id: Option<String>, popups: bool) -> Self {
        let authorizer: Authorizer = Arc::new(move |kind, id, op| {
            Operation::parse(op).is_some_and(|op| rbac::can(&config, role_id.as_deref(), kind, id, op))
        });
        Self { events: Arc::default(), authorizer, popups }
    }

    fn push(&self, event: HostEvent) {
        self.events.lock().unwrap_or_else(|e| e.into_inner()).push(event);
    }

    fn take(&self) -> Vec<HostEvent> {
        std::mem::take(&mut *self.events.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

impl ActionHost for UiHost {
    fn navigate(&self, target: NavigationTarget) {
        self.push(HostEvent::Navigate(target));
    }
    fn set_state(&self, scope: StateScope, key: &str, value: Value) {
        self.push(HostEvent::SetState(scope, key.to_string(), value));
    }
    fn confirm(&self, message: &str) -> bool {
        let (tx, rx) = channel();
        self.push(HostEvent::Confirm(message.to_string(), tx));
        rx.recv().unwrap_or(false)
    }
    fn notify(&self, message: &str, tone: NoticeTone) {
        self.push(HostEvent::Notify(message.to_string(), tone));
    }
    fn authorize(&self, kind: &str, id: &str, op: &str) -> bool {
        (self.authorizer)(kind, id, op)
    }
    fn refresh(&self) {
        self.push(HostEvent::Refresh);
    }
    fn open_popup(&self, target: &NavigationTarget) -> Option<Value> {
        if !self.popups {
            return None;
        }
        let (tx, rx) = channel();
        self.push(HostEvent::Popup(target.clone(), tx));
        Some(rx.recv().ok().flatten().unwrap_or(Value::Null))
    }
    fn close_form(&self, value: Value) -> bool {
        if !self.popups {
            return false;
        }
        let (tx, rx) = channel();
        self.push(HostEvent::Close(value, tx));
        rx.recv().unwrap_or(false)
    }
}

/// Handles the requests every owner treats alike (confirm, popups, close form); returns the
/// ones the owning view handles itself.
fn common<V: 'static>(
    events: Vec<HostEvent>,
    runtime: Option<&Entity<Runtime>>,
    cx: &mut Context<V>,
) -> Vec<HostEvent> {
    let mut rest = vec![];
    for event in events {
        match event {
            HostEvent::Confirm(message, tx) => {
                let answer = dialogs::confirm(cx, "Confirm", &message);
                cx.spawn(async move |_, _| {
                    let _ = tx.send(answer.await);
                })
                .detach();
            }
            HostEvent::Popup(target, tx) => match runtime {
                Some(runtime) => runtime.update(cx, |r, cx| r.open_popup(target, Some(tx), cx)),
                None => {
                    let _ = tx.send(None);
                }
            },
            HostEvent::Close(value, tx) => {
                let closed = match runtime {
                    Some(runtime) => runtime.update(cx, |r, cx| {
                        if !r.close_popup(Some(value), cx) && r.can_go_back() {
                            r.back(cx);
                        }
                        true
                    }),
                    None => false,
                };
                let _ = tx.send(closed);
            }
            other => rest.push(other),
        }
    }
    rest
}

/// Runs one action on its own thread (it may wait for a confirmation or a popup, which the UI
/// thread answers). `work` gets the host; `on_event` receives navigation, state, notice and
/// refresh requests on the UI thread, and `done` the result (after the last request was
/// handled). A poller hands requests over every few milliseconds while the action runs.
pub fn run_action_job<V: 'static>(
    cx: &mut Context<V>,
    host: UiHost,
    runtime: Option<Entity<Runtime>>,
    work: impl FnOnce(Arc<dyn ActionHost>) -> ActionResult + Send + 'static,
    on_event: fn(&mut V, HostEvent, &mut Context<V>),
    done: impl FnOnce(&mut V, ActionResult, &mut Context<V>) + 'static,
) {
    let slot: Arc<Mutex<Option<ActionResult>>> = Arc::default();
    {
        let (host, slot) = (host.clone(), slot.clone());
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(Arc::new(host))))
                .unwrap_or_else(|_| ActionResult::failure("The action stopped unexpectedly."));
            *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
        });
    }
    let mut done = Some(done);
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(Duration::from_millis(10)).await;
            // The result is read before the requests, so none made before it is missed.
            let result = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
            let events = host.take();
            let finished = result.is_some();
            let (runtime, finish) = (runtime.clone(), if finished { done.take() } else { None });
            let alive = this.update(cx, |view, cx| {
                for event in common(events, runtime.as_ref(), cx) {
                    on_event(view, event, cx);
                }
                if let (Some(result), Some(done)) = (result, finish) {
                    done(view, result, cx);
                }
            });
            if alive.is_err() || finished {
                return;
            }
        }
    })
    .detach();
}

/// The `ActionContext` of a run: `record` and `form` are the values the action sees.
pub fn context(
    config: crate::archive::DocumentConfig,
    label: &str,
    host: Arc<dyn ActionHost>,
    record: Option<serde_json::Map<String, Value>>,
    form: serde_json::Map<String, Value>,
    app: serde_json::Map<String, Value>,
) -> ActionContext {
    ActionContext {
        config: Arc::new(config),
        window_label: label.to_string(),
        record,
        form: Some(form),
        app,
        params: None,
        old: None,
        snapshot: None,
        host,
    }
}

/// The notice for an action's failure.
pub fn failure_notice(result: &ActionResult) -> Option<Notice> {
    (!result.ok)
        .then(|| result.error.clone().filter(|e| !e.is_empty()))
        .flatten()
        .map(|message| Notice { message, tone: NoticeTone::Error })
}
