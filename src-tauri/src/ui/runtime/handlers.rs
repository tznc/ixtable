//! The Runtime's `UiHandlers`: what a trigger or action running on a background thread does
//! when it navigates or sets app state. Requests are queued and applied to the `Runtime` on
//! the UI thread by a polling task; app state is shared with the `Runtime`, so a later step
//! of the same action already sees a `setState` step's value.

use super::{Runtime, RuntimeEnv};
use crate::automation_runner::{context::NavigationTarget, host::UiHandlers};
use crate::forms::actions::NavigationTarget as FormTarget;
use crate::forms::navigation::{PageKind, RuntimePage};
use crate::ui::prelude::*;
use serde_json::{Map, Value};
use std::sync::{Arc, Mutex, mpsc::{Sender, channel}};
use std::time::Duration;

const POLL: Duration = Duration::from_millis(50);

pub struct RunHandlers {
    state: Arc<Mutex<Map<String, Value>>>,
    // `user` and `role`, refreshed on the UI thread (they depend on the role in force).
    identity: Mutex<Map<String, Value>>,
    pages: Mutex<Vec<RuntimePage>>,
    popups: Mutex<Vec<Popup>>,
}

/// A popup request from an action thread, which waits on the sender's receiver.
enum Popup {
    Open(FormTarget, Sender<Option<Value>>),
    Close(Value, Sender<bool>),
}

// Drops unanswered requests when the Run body goes away, so waiting action threads wake.
struct Flush(Arc<RunHandlers>);

impl Drop for Flush {
    fn drop(&mut self) {
        lock(&self.0.popups).clear();
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl RunHandlers {
    pub fn new(state: Arc<Mutex<Map<String, Value>>>) -> Arc<Self> {
        Arc::new(Self { state, identity: Mutex::default(), pages: Mutex::default(), popups: Mutex::default() })
    }

    /// Applies queued navigation and refreshes the identity, until the body goes away.
    pub fn relay(self: &Arc<Self>, runtime: Entity<Runtime>, env: RuntimeEnv, cx: &mut Context<super::RunBody>) -> Task<()> {
        let handlers = Flush(self.clone());
        cx.spawn(async move |this, cx| loop {
            handlers.0.apply(&runtime, &env, cx);
            cx.background_executor().timer(POLL).await;
            if this.update(cx, |_, _| {}).is_err() {
                break;
            }
        })
    }

    fn apply(&self, runtime: &Entity<Runtime>, env: &RuntimeEnv, cx: &mut AsyncApp) {
        let pages = std::mem::take(&mut *lock(&self.pages));
        let popups = std::mem::take(&mut *lock(&self.popups));
        let _ = cx.update(|cx| {
            let app = runtime.read(cx).app(env.config(cx));
            let mut identity = lock(&self.identity);
            for key in ["user", "role"] {
                if let Some(value) = app.get(key) {
                    identity.insert(key.into(), value.clone());
                }
            }
            drop(identity);
            for page in pages {
                runtime.update(cx, |r, cx| r.navigate(page, cx));
            }
            for popup in popups {
                match popup {
                    Popup::Open(target, reply) => runtime.update(cx, |r, cx| r.open_popup(target, Some(reply), cx)),
                    Popup::Close(value, reply) => {
                        // Like the buttons' closeForm: the topmost popup, else back out of the page.
                        let closed = runtime.update(cx, |r, cx| {
                            if !r.close_popup(Some(value), cx) && r.can_go_back() {
                                r.back(cx);
                            }
                            true
                        });
                        let _ = reply.send(closed);
                    }
                }
            }
        });
    }
}

impl UiHandlers for RunHandlers {
    fn navigate(&self, target: &NavigationTarget) -> bool {
        let Some(kind) = PageKind::parse(&target.kind) else { return false };
        lock(&self.pages).push(RuntimePage {
            kind,
            id: target.id.clone(),
            mode: target.mode.clone(),
            record_id: target.record_id.clone(),
            params: target.params.clone().unwrap_or_default(),
            nav_id: None,
        });
        true
    }

    fn set_state(&self, scope: &str, key: &str, value: &Value) -> bool {
        if scope != "app" {
            return false;
        }
        lock(&self.state).insert(key.to_string(), value.clone());
        true
    }

    fn open_popup(&self, target: &NavigationTarget) -> Option<Value> {
        let target = FormTarget {
            kind: target.kind.clone(),
            id: target.id.clone(),
            mode: target.mode.clone(),
            record_id: target.record_id.clone(),
            params: target.params.clone().unwrap_or_default(),
        };
        let (tx, rx) = channel();
        lock(&self.popups).push(Popup::Open(target, tx));
        // Dismissed (or the body went away): null, like the TS runtime.
        Some(rx.recv().ok().flatten().unwrap_or(Value::Null))
    }

    fn close_form(&self, value: &Value) -> bool {
        let (tx, rx) = channel();
        lock(&self.popups).push(Popup::Close(value.clone(), tx));
        rx.recv().unwrap_or(false)
    }

    fn app_state(&self) -> Map<String, Value> {
        let mut app = lock(&self.state).clone();
        app.extend(lock(&self.identity).clone());
        app
    }
}

#[cfg(test)]
mod tests {
    use super::{Arc, NavigationTarget, RunHandlers, UiHandlers, lock};
    use serde_json::json;

    #[test]
    fn app_state_merges_state_with_identity_and_navigation_is_queued() {
        let handlers = RunHandlers::new(Arc::default());
        assert!(handlers.set_state("app", "n", &json!(1)));
        assert!(!handlers.set_state("form", "n", &json!(1)));
        lock(&handlers.identity).insert("user".into(), json!({"name": "Developer"}));
        let app = handlers.app_state();
        assert_eq!((app["n"].clone(), app["user"]["name"].clone()), (json!(1), json!("Developer")));
        assert!(handlers.navigate(&NavigationTarget::new("table", "audit")));
        assert!(!handlers.navigate(&NavigationTarget::new("bogus", "x")));
        assert_eq!(lock(&handlers.pages).len(), 1);
    }
}
