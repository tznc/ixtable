//! Installs everything automation needs for one open document: the trigger hook, the
//! custom-action router and the job worker. Dropping the value removes all three.

use super::context::{ActionHost, SessionEnv, TriggerEnv};
use super::custom::install_custom_actions;
use super::host::app_state;
use super::triggers::install_triggers;
use super::worker::{start_worker, Polling, WorkerHandle};
use crate::jobs::Job;
use crate::records::Registration;
use std::sync::Arc;

/// The environment of a window: definitions from its session, UI effects to `host`, the
/// `app` scope from the installed `UiHandlers`.
pub fn session_env(label: &str, host: Arc<dyn ActionHost>) -> Arc<SessionEnv> {
    let key = label.to_string();
    Arc::new(SessionEnv::new(label, host).with_app(Arc::new(move || app_state(&key))))
}

pub struct Automation {
    _hook: Registration,
    _router: Registration,
    _worker: WorkerHandle,
}

/// Installs triggers, custom actions and the job worker for `label`. `on_job` runs after
/// each finished job (to refresh data views).
pub fn install(
    label: &str,
    host: Arc<dyn ActionHost>,
    on_job: Option<Arc<dyn Fn(&Job, bool) + Send + Sync>>,
    polling: Polling,
) -> Automation {
    let env: Arc<dyn TriggerEnv> = session_env(label, host);
    Automation {
        _hook: install_triggers(env.clone()),
        _router: install_custom_actions(env.clone()),
        _worker: start_worker(env, on_job, polling),
    }
}
