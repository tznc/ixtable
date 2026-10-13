//! Runs declarative actions and record triggers (PRD §17), the Rust port of the former
//! `src/automation/*.ts` logic. `automation.rs` stores and validates the definitions; this
//! module executes them:
//!
//! - `runner`: the action runner (steps, failure modes, transactions).
//! - `triggers`: the `records::RecordHook` that fires before-change, sync and async triggers.
//! - `custom`: the `records::RecordRouter` of `customAction` entities.
//! - `worker`: the background thread that drains the durable job queue.
//! - `context` and `host`: the environment of a run and the hosts that handle UI effects.
//!   Steps that need a UI (navigate, setState, popups) go through `host::UiHandlers`, which
//!   the Runtime installs per window; confirmations use `ui::dialogs`.
//! - `install`: installs the hook, router and worker for a window while its document is open.

pub mod context;
pub mod custom;
pub mod host;
pub mod install;
pub mod rows;
pub mod runner;
pub mod triggers;
pub mod values;
pub mod worker;

#[cfg(test)]
mod tests_actions;
#[cfg(test)]
mod tests_rbac;
#[cfg(test)]
mod tests_support;
#[cfg(test)]
mod tests_triggers;
#[cfg(test)]
mod tests_before_change;
#[cfg(test)]
mod tests_write_path;

pub use context::{
    ActionContext, ActionHost, ContextBase, NavigationTarget, SessionEnv, Tone, TriggerEnv,
};
pub use runner::{run_action, run_action_by_id, ActionResult, StepLog, MAX_ACTION_DEPTH};
pub use triggers::{install_triggers, MAX_TRIGGER_DEPTH};
