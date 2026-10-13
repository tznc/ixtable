//! Connects the Runtime's action seam (`forms::actions`) to the Automation runner
//! (`automation_runner`). The Runtime builds a `forms::actions::ActionContext` whose host is
//! the Run mode's `UiHost`; this adapter turns it into the runner's context, so buttons and
//! form events run the real declarative actions (navigate, setState, popups and confirms go
//! to the Runtime through that host).

use crate::automation_runner as runner;
use crate::forms::actions::{
    self as seam, ActionContext, ActionHost, ActionResult, ActionRunner, NavigationTarget,
    NoticeTone, StateScope,
};
use serde_json::{Map, Value};
use std::sync::Arc;

/// The runner the Runtime installs when nothing else did.
pub struct AutomationRunner;

/// Installs [`AutomationRunner`] unless a runner is already set (tests install doubles).
pub fn ensure_runner() {
    if seam::action_runner().is_none() {
        seam::set_action_runner(Some(Arc::new(AutomationRunner)));
    }
}

impl ActionRunner for AutomationRunner {
    fn run_action(&self, action_id: &str, ctx: &ActionContext) -> ActionResult {
        let host: Arc<dyn runner::ActionHost> = Arc::new(HostAdapter(ctx.host.clone()));
        let mut run = runner::ActionContext::new(&ctx.window_label, ctx.config.clone(), host);
        run.record = ctx.record.clone();
        run.form = ctx.form.clone();
        run.app = ctx.app.clone();
        run.params = ctx.params.clone();
        run.old = ctx.old.clone();
        run.snapshot = ctx.snapshot.clone();
        let authorizer = ctx.host.clone();
        run.authorize = Some(Arc::new(move |kind, id, op| authorizer.authorize(kind, id, op)));
        let result = runner::run_action_by_id(action_id, run);
        ActionResult {
            ok: result.ok,
            error: result.error,
            cancelled: result.cancelled,
            aborted: result.aborted,
            results: result.results,
        }
    }
}

/// The Runtime's host seen as the runner's host.
struct HostAdapter(Arc<dyn ActionHost>);

fn target(t: &runner::NavigationTarget) -> NavigationTarget {
    NavigationTarget {
        kind: t.kind.clone(),
        id: t.id.clone(),
        mode: t.mode.clone(),
        record_id: t.record_id.clone(),
        params: t.params.clone().unwrap_or_else(Map::new),
    }
}

impl runner::ActionHost for HostAdapter {
    fn navigate(&self, to: &runner::NavigationTarget) -> Result<(), String> {
        self.0.navigate(target(to));
        Ok(())
    }
    fn set_state(&self, scope: &str, key: &str, value: &Value) -> Result<(), String> {
        let scope = if scope == "app" { StateScope::App } else { StateScope::Form };
        self.0.set_state(scope, key, value.clone());
        Ok(())
    }
    fn confirm(&self, message: &str) -> Result<bool, String> {
        Ok(self.0.confirm(message))
    }
    fn notify(&self, message: &str, tone: runner::Tone) {
        let tone = if tone == runner::Tone::Error { NoticeTone::Error } else { NoticeTone::Info };
        self.0.notify(message, tone);
    }
    fn refresh(&self) {
        self.0.refresh();
    }
    fn open_popup(&self, to: &runner::NavigationTarget) -> Result<Option<Value>, String> {
        Ok(self.0.open_popup(&target(to)))
    }
    fn close_form(&self, value: &Value) {
        self.0.close_form(value.clone());
    }
}
