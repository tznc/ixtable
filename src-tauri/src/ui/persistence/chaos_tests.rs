//! Chaos tests for the save UI (exploratory): each asserts what the user should see.
use super::tests::{archive_path, unique};
use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;
use std::{path::PathBuf, time::Duration};

fn saved_document(h: &mut Harness, name: &str) -> PathBuf {
    let path = archive_path(name);
    h.dialogs.queue_save(Some(path.clone()));
    h.click("save-project");
    assert_eq!(h.label("document-status"), "Saved");
    path
}

fn rename(h: &mut Harness, name: &str) {
    let shell = h.shell();
    let name = name.to_string();
    h.update(|_, cx| {
        shell.update(cx, |s, cx| {
            s.store
                .update(cx, |st, cx| st.update(&format!("Rename to {name}"), |c| c.name = name, cx))
                .unwrap();
        })
    });
    h.settle();
}

fn archived_name(path: &PathBuf) -> String {
    let w = unique("verify");
    crate::open_document(w.clone(), path.to_string_lossy().into()).unwrap();
    let name = crate::manager().unwrap().config(&w).unwrap().name;
    crate::close_document(w, true).unwrap();
    name
}

#[gpui_kit::test]
fn chaos_ctrl_s_saves_the_document(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let _ = saved_document(&mut h, "ctrl-s");
    h.create_table("t", &[("id", "INTEGER", 1)]);
    assert!(h.state().dirty);
    h.click("shell");
    h.press("secondary-s");
    assert!(!h.state().dirty, "Ctrl+S did not save");
}

#[gpui_kit::test]
fn chaos_rapid_ctrl_s_and_autosave_end_clean(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let _ = saved_document(&mut h, "rapid-ui");
    for i in 0..10 {
        h.create_table(&format!("t{i}"), &[("id", "INTEGER", 1)]);
        h.click("save-project");
        h.advance(Duration::from_millis(700));
    }
    h.advance(Duration::from_millis(1600));
    assert!(!h.state().dirty);
    assert_eq!(h.label("document-status"), "Saved");
    assert!(h.error().is_none(), "{:?}", h.error());
}

/// A clean document whose file another program replaced: Save shows a conflict, then
/// closing must either close or offer to discard, never dead-end on an error.
#[gpui_kit::test]
fn chaos_close_a_clean_conflicted_document(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let path = saved_document(&mut h, "conflict-close");
    let other = unique("other");
    crate::new_document(other.clone()).unwrap();
    crate::save_document_as(other.clone(), path.to_string_lossy().into()).unwrap();
    crate::close_document(other, true).unwrap();
    h.click("save-project");
    assert!(h.state().conflict);
    h.click("close-project");
    assert!(
        !h.has_shell() || h.dialogs.count("confirm") > 0,
        "close dead-ends: error={:?}",
        h.error()
    );
}

/// Closing while the save cannot be written: the user should learn why.
#[gpui_kit::test]
fn chaos_close_when_the_flush_fails_explains_the_failure(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let path = saved_document(&mut h, "close-fail");
    std::fs::remove_file(&path).unwrap();
    h.create_table("late", &[("id", "INTEGER", 1)]);
    h.dialogs.queue_confirm(false);
    h.click("close-project");
    let calls = h.dialogs.calls.lock().unwrap().clone();
    let confirm = calls.iter().find(|c| c.starts_with("confirm")).cloned().unwrap_or_default();
    eprintln!("close prompt: {confirm}");
    assert!(
        confirm.to_lowercase().contains("could not") || confirm.to_lowercase().contains("fail"),
        "discard prompt hides the save failure: {confirm}"
    );
}

/// A failed manual save must not leave the chip saying "Saved".
#[gpui_kit::test]
fn chaos_failed_manual_save_shows_in_the_status_chip(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let path = saved_document(&mut h, "manual-fail");
    std::fs::remove_file(&path).unwrap();
    h.click("save-project");
    eprintln!("error={:?} chip={}", h.error(), h.label("document-status"));
    assert!(h.error().is_some());
    assert_ne!(h.label("document-status"), "Saved", "chip says Saved after a failed save");
}

#[gpui_kit::test]
fn chaos_undo_across_saves_reaches_the_archive(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let path = saved_document(&mut h, "undo-save");
    rename(&mut h, "First");
    h.click("save-project");
    rename(&mut h, "Second");
    h.advance(Duration::from_millis(1600));
    assert_eq!(archived_name(&path), "Second");
    h.click("shell");
    h.press("secondary-z");
    assert_eq!(h.config().name, "First");
    assert!(h.state().dirty);
    h.advance(Duration::from_millis(1600));
    assert!(!h.state().dirty);
    assert_eq!(archived_name(&path), "First");
    h.press("secondary-shift-z");
    h.advance(Duration::from_millis(1600));
    assert_eq!(archived_name(&path), "Second");
}

/// An edit made while an autosave is in flight is saved by the next one.
#[gpui_kit::test]
fn chaos_edit_during_an_autosave_is_saved_next(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let path = saved_document(&mut h, "mid-save");
    rename(&mut h, "A");
    h.advance(Duration::from_millis(1500));
    rename(&mut h, "B");
    h.advance(Duration::from_millis(3500));
    assert!(!h.state().dirty);
    assert_eq!(h.label("document-status"), "Saved");
    assert_eq!(archived_name(&path), "B");
}
