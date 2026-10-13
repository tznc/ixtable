//! The session's definition store: every DocumentConfig edit goes through
//! `ConfigStore::update`, which writes it to Rust and records undo history.
//! Port of the React `DocumentConfigProvider` (src/lib/config-store.tsx).

use crate::{
    archive::DocumentConfig,
    manager::{AppError, SessionState},
};
use gpui_kit::*;
use serde_json::{Map, Value};
use std::time::{Duration, Instant};

const COALESCE: Duration = Duration::from_millis(1000);
const HISTORY_LIMIT: usize = 200;

#[derive(Clone)]
struct Entry {
    config: DocumentConfig,
    label: String,
}

#[derive(Clone, Debug)]
pub enum StoreEvent {
    /// The config shown changed (edit, undo, redo or reload).
    Changed,
    /// A write returned this session state (dirty flag, revision, name).
    State(SessionState),
    /// A write or reload failed.
    Failed(AppError),
}

pub struct ConfigStore {
    label: SharedString,
    config: DocumentConfig,
    past: Vec<Entry>,
    future: Vec<Entry>,
    last_edit: Option<(String, Instant)>,
    // configRevision the store last wrote or loaded, and the newest one seen anywhere.
    revision: u64,
    seen: u64,
    pub error: Option<AppError>,
}
impl EventEmitter<StoreEvent> for ConfigStore {}

fn to_map(config: &DocumentConfig) -> Map<String, Value> {
    match serde_json::to_value(config) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// Top-level fields whose values differ.
fn changed_keys(before: &DocumentConfig, after: &DocumentConfig) -> Vec<String> {
    let (a, b) = (to_map(before), to_map(after));
    let mut keys: Vec<String> = a.keys().chain(b.keys()).cloned().collect();
    keys.sort();
    keys.dedup();
    keys.retain(|k| a.get(k) != b.get(k));
    keys
}

/// Overwrites `keys` of a history snapshot with their values from `latest`.
fn carry(snapshot: &DocumentConfig, latest: &DocumentConfig, keys: &[String]) -> DocumentConfig {
    if keys.is_empty() {
        return snapshot.clone();
    }
    let mut out = to_map(snapshot);
    let latest = to_map(latest);
    for key in keys {
        match latest.get(key) {
            Some(v) => out.insert(key.clone(), v.clone()),
            None => out.remove(key),
        };
    }
    serde_json::from_value(Value::Object(out)).unwrap_or_else(|_| snapshot.clone())
}

/// Undo and redo restore definitions, never the session's place in the UI.
fn keep_session_fields(snapshot: &DocumentConfig, current: &DocumentConfig) -> DocumentConfig {
    let mut out = snapshot.clone();
    out.active_mode = current.active_mode.clone();
    out.navigation_state = current.navigation_state.clone();
    out
}

impl ConfigStore {
    pub fn load(label: &str) -> Result<Self, AppError> {
        let manager = crate::manager()?;
        let state = manager.state(label)?;
        let config = manager.config(label)?;
        Ok(Self {
            label: label.to_string().into(),
            config,
            past: vec![],
            future: vec![],
            last_edit: None,
            revision: state.config_revision,
            seen: state.config_revision,
            error: None,
        })
    }

    pub fn config(&self) -> &DocumentConfig {
        &self.config
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.past.last().map(|e| e.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.future.last().map(|e| e.label.as_str())
    }

    /// Notes a SessionState produced outside the store. When its revision is ahead of
    /// the one the store last wrote or loaded, the next edit reloads first.
    pub fn observe(&mut self, state: &SessionState) {
        self.seen = self.seen.max(state.config_revision);
    }

    fn stale(&self) -> bool {
        self.seen > self.revision
    }

    fn write(&mut self, next: DocumentConfig, cx: &mut Context<Self>) -> Result<SessionState, AppError> {
        let result = crate::manager().and_then(|m| m.update_config(&self.label, next.clone()));
        match result {
            Ok(state) => {
                self.config = next;
                self.error = None;
                self.revision = state.config_revision;
                self.seen = self.seen.max(state.config_revision);
                cx.emit(StoreEvent::State(state.clone()));
                cx.emit(StoreEvent::Changed);
                cx.notify();
                Ok(state)
            }
            Err(e) => {
                self.error = Some(e.clone());
                cx.emit(StoreEvent::Failed(e.clone()));
                cx.notify();
                Err(e)
            }
        }
    }

    /// Applies an undoable edit. Edits with the same label within a second coalesce into
    /// one undo step.
    pub fn update(
        &mut self,
        label: &str,
        mutate: impl FnOnce(&mut DocumentConfig),
        cx: &mut Context<Self>,
    ) -> Result<SessionState, AppError> {
        self.update_with(label, true, mutate, cx)
    }

    /// `undoable: false` for navigation-only changes (mode switches).
    pub fn update_with(
        &mut self,
        label: &str,
        undoable: bool,
        mutate: impl FnOnce(&mut DocumentConfig),
        cx: &mut Context<Self>,
    ) -> Result<SessionState, AppError> {
        // The backend changed the config since the store last wrote or loaded it:
        // load that first so this edit applies on top of it instead of overwriting it.
        if self.stale() {
            self.reload(None, cx)?;
        }
        let previous = self.config.clone();
        let mut next = previous.clone();
        mutate(&mut next);
        let state = self.write(next, cx)?;
        if undoable {
            let now = Instant::now();
            let coalesce = self
                .last_edit
                .as_ref()
                .is_some_and(|(l, at)| l == label && now.duration_since(*at) < COALESCE);
            if !coalesce {
                self.past.push(Entry {
                    config: previous,
                    label: label.to_string(),
                });
                if self.past.len() > HISTORY_LIMIT {
                    self.past.remove(0);
                }
            }
            self.future.clear();
            self.last_edit = Some((label.to_string(), now));
        }
        Ok(state)
    }

    /// Re-reads the config from Rust. With a label, records the external change as an
    /// undo step. Without one (backend-owned changes such as DDL entity updates), the
    /// changed top-level fields are carried into the undo history, so undoing an earlier
    /// edit does not revert the backend change.
    pub fn reload(
        &mut self,
        label: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Result<DocumentConfig, AppError> {
        let loaded_at = self.seen;
        let manager = crate::manager()?;
        let next = match manager.config(&self.label) {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(e.clone());
                cx.emit(StoreEvent::Failed(e.clone()));
                return Err(e);
            }
        };
        let revision = manager.state(&self.label).map(|s| s.config_revision).unwrap_or(0);
        self.revision = self.revision.max(loaded_at).max(revision);
        self.seen = self.seen.max(revision);
        let previous = std::mem::replace(&mut self.config, next.clone());
        match label {
            Some(label) => {
                self.past.push(Entry {
                    config: previous,
                    label: label.to_string(),
                });
                self.future.clear();
                self.last_edit = None;
            }
            None => {
                let keys = changed_keys(&previous, &next);
                if !keys.is_empty() {
                    for e in self.past.iter_mut().chain(self.future.iter_mut()) {
                        e.config = carry(&e.config, &next, &keys);
                    }
                    self.last_edit = None;
                }
            }
        }
        cx.emit(StoreEvent::Changed);
        cx.notify();
        Ok(next)
    }

    fn travel(&mut self, back: bool, cx: &mut Context<Self>) -> Result<(), AppError> {
        if self.stale() {
            self.reload(None, cx)?;
        }
        let Some(target) = (if back { self.past.pop() } else { self.future.pop() }) else {
            return Ok(());
        };
        let present = self.config.clone();
        self.last_edit = None;
        let next = keep_session_fields(&target.config, &present);
        let undo_entry = Entry {
            config: present,
            label: target.label.clone(),
        };
        match self.write(next, cx) {
            Ok(_) => {
                if back {
                    self.future.push(undo_entry);
                } else {
                    self.past.push(undo_entry);
                }
                Ok(())
            }
            Err(e) => {
                // Put the step back so the user can retry.
                if back {
                    self.past.push(target);
                } else {
                    self.future.push(target);
                }
                Err(e)
            }
        }
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) -> Result<(), AppError> {
        self.travel(true, cx)
    }
    pub fn redo(&mut self, cx: &mut Context<Self>) -> Result<(), AppError> {
        self.travel(false, cx)
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: with gpui-kit's test-support, its glob shadows `#[test]`.
    use super::{DocumentConfig, carry, changed_keys, keep_session_fields};

    #[test]
    fn carry_overwrites_only_the_named_keys() {
        let mut a = DocumentConfig::default();
        a.name = "A".into();
        let mut b = a.clone();
        b.name = "B".into();
        b.active_mode = "query".into();
        let keys = changed_keys(&a, &b);
        assert_eq!(keys, vec!["activeMode".to_string(), "name".to_string()]);
        let carried = carry(&a, &b, &["name".to_string()]);
        assert_eq!(carried.name, "B");
        assert_eq!(carried.active_mode, a.active_mode);
    }

    #[test]
    fn undo_keeps_the_mode_and_navigation() {
        let mut snapshot = DocumentConfig::default();
        snapshot.active_mode = "data".into();
        let mut current = DocumentConfig::default();
        current.active_mode = "reports".into();
        current.navigation_state = serde_json::json!({"x": 1});
        let out = keep_session_fields(&snapshot, &current);
        assert_eq!(out.active_mode, "reports");
        assert_eq!(out.navigation_state, serde_json::json!({"x": 1}));
    }
}
