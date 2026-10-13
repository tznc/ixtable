//! Form events (PRD §17.4, docs/decisions/form-events.md; port of `src/runtime/formEvents.ts`).
//! Each event runs the declarative action bound to it in `Form.events`; nothing is a script.
//!
//! - on load: once when a record view of the form opens (detail, edit or create).
//! - on current: each time the view moves to a record (a new record counts).
//! - before update: before a create or edit is written; a failing action vetoes the save.
//! - after update: after the write committed; a failure is reported, the save stays.

use super::actions::{ActionContext, ActionResult, run_action};
use crate::design::Form;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormEvent {
    OnLoad,
    OnCurrent,
    BeforeUpdate,
    AfterUpdate,
}

impl FormEvent {
    pub const ALL: [FormEvent; 4] = [
        FormEvent::OnLoad,
        FormEvent::OnCurrent,
        FormEvent::BeforeUpdate,
        FormEvent::AfterUpdate,
    ];
    pub fn key(self) -> &'static str {
        match self {
            FormEvent::OnLoad => "onLoad",
            FormEvent::OnCurrent => "onCurrent",
            FormEvent::BeforeUpdate => "beforeUpdate",
            FormEvent::AfterUpdate => "afterUpdate",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            FormEvent::OnLoad => "On load",
            FormEvent::OnCurrent => "On current",
            FormEvent::BeforeUpdate => "Before update",
            FormEvent::AfterUpdate => "After update",
        }
    }
}

/// The action bound to `event`, if any.
pub fn event_action(form: &Form, event: FormEvent) -> Option<String> {
    let id = match event {
        FormEvent::OnLoad => &form.events.on_load,
        FormEvent::OnCurrent => &form.events.on_current,
        FormEvent::BeforeUpdate => &form.events.before_update,
        FormEvent::AfterUpdate => &form.events.after_update,
    };
    id.clone().filter(|id| !id.is_empty())
}

/// Runs the action bound to `event`; None when none is bound. Never panics.
pub fn run_form_event(form: &Form, event: FormEvent, ctx: &ActionContext) -> Option<ActionResult> {
    let action_id = event_action(form, event)?;
    Some(run_action(&action_id, ctx))
}

/// Why a before update action stopped the save, or None when it allows it.
pub fn veto_message(result: Option<&ActionResult>) -> Option<String> {
    let result = result.filter(|r| !r.ok)?;
    if result.cancelled {
        return Some("Save cancelled.".into());
    }
    Some(
        result
            .error
            .clone()
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| "The record was not saved.".into()),
    )
}

/// The problem to show when an after update action failed, or None.
pub fn after_update_problem(result: Option<&ActionResult>) -> Option<String> {
    let result = result.filter(|r| !r.ok && !r.cancelled)?;
    Some(format!(
        "Saved, but the after update action failed: {}",
        result.error.as_deref().unwrap_or("unknown error")
    ))
}

/// Where a view is in its form events: opened, and the record it last raised on-current for.
#[derive(Clone, Debug, Default)]
pub struct EventState {
    pub opened: bool,
    pub current: Option<String>,
}

/// Events to raise when a view shows `record_key`: on load once per view, on current per record.
pub fn events_to_raise(state: &mut EventState, record_key: &str) -> Vec<FormEvent> {
    let mut events = vec![];
    if !state.opened {
        events.push(FormEvent::OnLoad);
    }
    if state.current.as_deref() != Some(record_key) {
        events.push(FormEvent::OnCurrent);
    }
    state.opened = true;
    state.current = Some(record_key.to_string());
    events
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
