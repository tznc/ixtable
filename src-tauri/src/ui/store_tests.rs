//! Port of tests/unit/config-store.test.tsx. The Rust store writes synchronously, so the cases
//! about interleaved in-flight writes (coalescing queued writes, `onState` throwing, waiting
//! for queued writes before a reload) have no counterpart; the rest run against a real session.
//! Imports are explicit.

use crate::archive::DocumentConfig;
use crate::ui::store::ConfigStore;
use crate::ui::testing::Harness;
use gpui_kit::{Entity, TestAppContext};
use serde_json::json;

fn store(h: &mut Harness) -> Entity<ConfigStore> {
    h.read_shell(|s, _| s.store.clone())
}

fn rename(h: &mut Harness, store: &Entity<ConfigStore>, label: &str, name: &str) {
    let name = name.to_string();
    h.cx.update(|cx| {
        store.update(cx, |s, cx| s.update(label, move |c| c.name = name, cx)).expect("update");
    });
}

fn backend(h: &Harness) -> DocumentConfig {
    crate::manager().expect("manager").config(&h.label).expect("config")
}

fn can_undo(h: &mut Harness, store: &Entity<ConfigStore>) -> bool {
    h.cx.read(|cx| store.read(cx).can_undo())
}

fn undo(h: &mut Harness, store: &Entity<ConfigStore>) {
    h.cx.update(|cx| store.update(cx, |s, cx| s.undo(cx)).expect("undo"));
}

#[gpui_kit::test]
fn persists_updates_through_the_backend_and_reports_session_state(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let store = store(&mut h);
    let state = h.cx.update(|cx| {
        store.update(cx, |s, cx| s.update("Rename", |c| c.name = "Alpha".into(), cx)).expect("update")
    });
    assert_eq!(state.name, "Alpha");
    assert_eq!(backend(&h).name, "Alpha");
    assert_eq!(h.cx.read(|cx| store.read(cx).config().name.clone()), "Alpha");
}

#[gpui_kit::test]
fn undoes_and_redoes_definition_edits_but_keeps_the_active_mode(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let store = store(&mut h);
    rename(&mut h, &store, "Rename", "Beta");
    h.cx.update(|cx| {
        store.update(cx, |s, cx| s.update_with("Mode", false, |c| c.active_mode = "reports".into(), cx)).expect("mode");
    });
    undo(&mut h, &store);
    let config = h.cx.read(|cx| store.read(cx).config().clone());
    assert_eq!((config.name.as_str(), config.active_mode.as_str()), ("Untitled", "reports"));
    assert!(!can_undo(&mut h, &store));
    h.cx.update(|cx| store.update(cx, |s, cx| s.redo(cx)).expect("redo"));
    let backend = backend(&h);
    assert_eq!((backend.name.as_str(), backend.active_mode.as_str()), ("Beta", "reports"));
}

#[gpui_kit::test]
fn coalesces_rapid_edits_with_the_same_label_into_one_undo_step(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let store = store(&mut h);
    rename(&mut h, &store, "Rename", "Alpha");
    rename(&mut h, &store, "Rename", "Alpha");
    rename(&mut h, &store, "Other", "Beta");
    undo(&mut h, &store);
    assert_eq!(h.cx.read(|cx| store.read(cx).config().name.clone()), "Alpha");
    undo(&mut h, &store);
    assert_eq!(h.cx.read(|cx| store.read(cx).config().name.clone()), "Untitled");
    assert!(!can_undo(&mut h, &store));
}

#[gpui_kit::test]
fn records_an_external_change_as_an_undo_step_when_reloading_with_a_label(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let store = store(&mut h);
    let mut external = backend(&h);
    external.name = "From YAML".into();
    crate::manager().unwrap().update_config(&h.label, external).expect("backend edit");
    h.cx.update(|cx| store.update(cx, |s, cx| s.reload(Some("Apply YAML"), cx)).expect("reload"));
    assert_eq!(h.cx.read(|cx| store.read(cx).config().name.clone()), "From YAML");
    assert_eq!(h.cx.read(|cx| store.read(cx).undo_label().map(str::to_string)), Some("Apply YAML".into()));
    undo(&mut h, &store);
    assert_eq!(backend(&h).name, "Untitled");
}

#[gpui_kit::test]
fn reloads_backend_owned_changes_without_an_undo_step_and_keeps_them_on_undo(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let store = store(&mut h);
    rename(&mut h, &store, "Rename", "Alpha");
    let mut external = backend(&h);
    external.settings = json!({"owned": "by the backend"});
    crate::manager().unwrap().update_config(&h.label, external).expect("backend edit");
    h.cx.update(|cx| store.update(cx, |s, cx| s.reload(None, cx)).expect("reload"));
    assert_eq!(h.cx.read(|cx| store.read(cx).undo_label().map(str::to_string)), Some("Rename".into()));
    undo(&mut h, &store);
    let after = backend(&h);
    assert_eq!(after.name, "Untitled");
    assert_eq!(after.settings, json!({"owned": "by the backend"}));
    assert!(!can_undo(&mut h, &store));
}

#[gpui_kit::test]
fn reloads_before_editing_when_a_session_state_shows_a_newer_config_revision(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let store = store(&mut h);
    rename(&mut h, &store, "Rename", "Alpha");
    let mut external = backend(&h);
    external.settings = json!({"owned": "elsewhere"});
    let state = crate::manager().unwrap().update_config(&h.label, external).expect("backend edit");
    h.cx.update(|cx| store.update(cx, |s, _| s.observe(&state)));
    rename(&mut h, &store, "Other", "Beta");
    let after = backend(&h);
    assert_eq!(after.name, "Beta");
    assert_eq!(after.settings, json!({"owned": "elsewhere"}), "the edit applied on top of the newer config");
}

#[gpui_kit::test]
fn keeps_the_config_and_reports_the_error_when_a_write_is_refused(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let store = store(&mut h);
    let refused = h.cx.update(|cx| {
        store.update(cx, |s, cx| {
            s.update(
                "Break",
                |c| {
                    let form = c.design.forms[0].clone();
                    c.design.forms.push(form);
                },
                cx,
            )
        })
    });
    assert_eq!(refused.map(|_| ()).unwrap_err().code, "INVALID_DESIGN_SCHEMA");
    assert_eq!(h.cx.read(|cx| store.read(cx).error.as_ref().map(|e| e.code.clone())), Some("INVALID_DESIGN_SCHEMA".into()));
    assert_eq!(h.cx.read(|cx| store.read(cx).config().design.forms.len()), 1);
    assert!(!can_undo(&mut h, &store));
    // The next valid edit clears the error.
    rename(&mut h, &store, "Rename", "Fine");
    assert!(h.cx.read(|cx| store.read(cx).error.is_none()));
}
