//! Chaos findings for save/autosave (`chaos-save-*`), exploratory.
use crate::ui::persistence::tests::{archive_path, unique};
use crate::ui::screenshot::context;
use crate::ui::testing::Harness;
use std::time::Duration;

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn chaos_save_findings() {
    let mut cx = context();
    let mut h = Harness::new_document(&mut cx);
    h.create_table("orders", &[("id", "INTEGER", 1)]);
    let path = archive_path("chaos-shots");
    h.dialogs.queue_save(Some(path.clone()));
    h.click("save-project");
    assert_eq!(h.label("document-status"), "Saved");

    // 1. Manual save after the file was deleted: banner error, chip still "Saved".
    std::fs::remove_file(&path).unwrap();
    h.click("save-project");
    assert!(h.error().unwrap().contains("MISSING_FILE"));
    assert_eq!(h.label("document-status"), "Saved");
    h.capture(
        "chaos-save-01-failed-save-shows-saved",
        &["An error banner says MISSING_FILE with a raw OS error.", "The header chip still says Saved."],
    );

    // 2. Restore a file another program wrote: Save marks a conflict, Close dead-ends.
    let other = unique("other");
    crate::new_document(other.clone()).unwrap();
    crate::save_document_as(other.clone(), path.to_string_lossy().into()).unwrap();
    crate::close_document(other, true).unwrap();
    h.click("save-project");
    h.advance(Duration::from_millis(100));
    assert!(h.state().conflict && !h.state().dirty);
    h.click("close-project");
    assert!(h.has_shell());
    assert!(h.error().unwrap().contains("CLOSE_BLOCKED"));
    h.capture(
        "chaos-save-02-close-blocked",
        &["Close leaves the document open with a CLOSE_BLOCKED error.", "No discard prompt was offered."],
    );
}
