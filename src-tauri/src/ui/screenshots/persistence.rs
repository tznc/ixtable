//! Persistence flows: autosave, recent documents, crash recovery, assets with archive
//! size and checkpoints, and the log tab (`persistence-*`, `assets-01`, `settings-06`).
//! Port of scripts/screenshot/specs/persistence.spec.tsx.

use crate::ui::persistence::tests::{archive_path, exclusive_recovery, seed_crashed_session, unique};
use crate::ui::screenshot::context;
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use gpui_kit::{ScrollDelta, SharedString, point, px, test::TestWindowExt as _};
use std::time::Duration;

/// Tall enough that the settings cards need no scrolling.
const TALL: (f32, f32) = (1280.0, 1700.0);

fn id(text: &str) -> SharedString {
    SharedString::from(text.to_string())
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn persistence_autosave_and_recent() {
    let mut cx = context();
    let mut h = Harness::new_document(&mut cx);
    h.create_table("orders", &[("id", "INTEGER", 1), ("total", "REAL", 0)]);
    let path = archive_path("orders-app");
    h.dialogs.queue_save(Some(path.clone()));
    h.click("save-project");
    assert!(h.label("save-status").starts_with("Saved at"));

    h.create_table("returns", &[("id", "INTEGER", 1)]);
    assert_eq!(h.label("save-status"), "Unsaved changes");
    h.advance(Duration::from_millis(1600));
    assert!(h.label("save-status").starts_with("Saved at"), "{}", h.label("save-status"));
    assert!(!h.state().dirty);
    assert!(h.exists("table:returns"));
    h.capture(
        "persistence-01-autosaved",
        &[
            "The sidebar shows the archive name and a 'Saved at' status after the autosave.",
            "The new returns table is listed in the object browser.",
        ],
    );

    h.click("close-project");
    assert!(!h.has_shell());
    let recent = format!("recent:{}", path.file_name().unwrap().to_string_lossy());
    assert!(h.exists(id(&recent)), "{recent}");
    h.capture(
        "persistence-02-recent",
        &["The start screen lists the saved document under Recent documents with its path."],
    );
    h.click(id(&recent));
    assert_eq!(h.label("document-status"), "Saved archive");
    assert_eq!(h.state().path.as_deref(), Some(path.to_str().unwrap()));
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn persistence_recovery_prompt() {
    let _guard = exclusive_recovery();
    let mut cx = context();
    let name = unique("Field notes");
    seed_crashed_session(&unique("crashy"), &name, Some(&archive_path("field-notes")));
    let mut h = Harness::start(&mut cx);
    assert!(h.exists("recover-unsaved-work"));
    assert!(h.exists(id(&format!("recover:{name}"))));
    h.capture(
        "persistence-03-recovery-prompt",
        &[
            "The start screen shows a Recover unsaved work section naming the crashed document.",
            "Recover and Discard actions are offered for the crashed session.",
        ],
    );
    h.click(id(&format!("recover:{name}")));
    assert_eq!(h.label("document-status"), "Saved archive");
    assert_eq!(h.read_page("notes").total, 1);
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn assets_size_checkpoints_and_logs() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, TALL);
    h.click("new-document");
    assert!(h.has_shell());
    h.create_table("orders", &[("id", "INTEGER", 1)]);
    h.switch_mode(ModeId::App);
    h.click("tab-assets");
    let dir = h.temp_path(&format!("assets-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    let logo = dir.join("logo.png");
    std::fs::write(&logo, [0x89, 0x50, 0x4e, 0x47, 1, 2, 3, 4]).unwrap();
    let manual = dir.join("handbook.pdf");
    std::fs::write(&manual, vec![7u8; 180_000]).unwrap();
    for file in [logo, manual] {
        h.dialogs.queue_open(Some(file));
        h.click("import-asset");
    }
    assert!(h.exists("asset-row:logo.png") && h.exists("asset-row:handbook.pdf"));
    h.click("measure-archive");
    let largest: Vec<String> = (0..5)
        .filter_map(|i| h.try_label(id(&format!("size-largest:{i}"))))
        .collect();
    assert!(largest.iter().any(|l| l.contains("handbook.pdf")), "{largest:?}");
    h.click("create-checkpoint");
    assert_eq!(h.label("checkpoint-status"), "Checkpoint created.");
    assert!(h.label("checkpoint:0").contains("manual"));
    h.capture(
        "assets-01-assets-and-size",
        &[
            "The Assets table lists logo.png and handbook.pdf with MIME type, size, and actions.",
            "Archive size breaks the archive into Records, Configuration, Assets, and Other against the 500 MB limit.",
            "The Checkpoints panel lists one manual checkpoint with a restore action.",
        ],
    );

    h.click("tab-logs");
    assert!(h.exists("log:0"));
    h.update(|w, cx| w.scroll("settings-panel", ScrollDelta::Pixels(point(px(0.), px(5000.))), cx));
    h.capture(
        "settings-06-logs",
        &["The Logs tab lists timestamped, levelled diagnostic lines with a Copy action."],
    );
}
