//! Persistence flows: assets, archive size, checkpoints, autosave, crash recovery,
//! forwarded files and newer-build archives. Ports of tests/integration/{assets,
//! persistence,open-files,newer-build-archives}.test.tsx and tests/unit/autosave.
//! Imports are explicit: `gpui_kit::*` would shadow `#[test]`.

use crate::{
    manager::SessionState,
    ui::{shell::ModeId, testing::Harness},
};
use gpui_kit::{ScrollDelta, SharedString, TestAppContext, point, px, test::TestWindowExt as _};
use serde_json::json;
use std::{path::PathBuf, time::Duration};

fn id(text: &str) -> SharedString {
    SharedString::from(text.to_string())
}

fn settings_tab(h: &mut Harness, tab: &str) {
    h.switch_mode(ModeId::App);
    h.click(id(&format!("tab-{tab}")));
}

/// Scrolls the settings panel to its end (lower cards start out of view).
fn scroll_to_end(h: &mut Harness) {
    h.update(|w, cx| {
        w.scroll(
            "settings-panel",
            ScrollDelta::Pixels(point(px(0.), px(-5000.))),
            cx,
        )
    });
    h.settle();
}

fn fixture(h: &Harness, name: &str, contents: &[u8]) -> PathBuf {
    let path = h.temp_path(&format!("{}-{name}", uuid::Uuid::new_v4().simple()));
    // The asset keeps the file's own name, so write it into a unique directory.
    let dir = path.with_extension("d");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(name);
    std::fs::write(&file, contents).unwrap();
    file
}

fn import(h: &mut Harness, path: PathBuf) {
    h.dialogs.queue_open(Some(path));
    h.click("import-asset");
}

pub(crate) fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4().simple())
}

pub(crate) fn archive_path(name: &str) -> PathBuf {
    crate::paths::state_dir().join(format!("{}.ixt", unique(name)))
}

// ---- Assets ------------------------------------------------------------------

#[gpui_kit::test]
fn imports_deduplicates_exports_and_removes_application_assets(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    settings_tab(&mut h, "assets");
    let png = [0x89, 0x50, 0x4e, 0x47, 1, 2, 3];
    let logo = fixture(&h, "logo.png", &png);
    import(&mut h, logo.clone());
    let row = h.label("asset-row:logo.png");
    assert!(row.contains("image/png"), "{row}");
    assert!(row.contains("7 B"), "{row}");
    assert_eq!(h.notice().unwrap_or_default(), "Imported logo.png.");

    let copy = fixture(&h, "copy-of-logo.png", &png);
    import(&mut h, copy);
    assert_eq!(
        h.notice().unwrap_or_default(),
        "Same content as logo.png; reused the existing asset."
    );
    assert_eq!(h.label("asset-count"), "1 asset");
    let state = h.state();
    assert_eq!(state.attachment_count, 1);
    assert!(state.dirty);

    let destination = h.temp_path("exported-logo.png");
    h.dialogs.queue_save(Some(destination.clone()));
    h.click("export-asset:logo.png");
    assert_eq!(h.notice().unwrap_or_default(), "Exported logo.png.");
    assert_eq!(std::fs::read(&destination).unwrap(), std::fs::read(&logo).unwrap());

    h.dialogs.queue_confirm(true);
    h.click("remove-asset:logo.png");
    assert_eq!(h.notice().unwrap_or_default(), "Removed logo.png.");
    assert!(!h.exists("asset-row:logo.png"));
    assert_eq!(h.label("asset-count"), "0 assets");
}

#[gpui_kit::test]
fn keeps_an_asset_when_the_removal_is_not_confirmed(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    settings_tab(&mut h, "assets");
    let file = fixture(&h, "keep.csv", b"a,b\n1,2\n");
    import(&mut h, file);
    h.dialogs.queue_confirm(false);
    h.click("remove-asset:keep.csv");
    assert!(h.exists("asset-row:keep.csv"));
    assert_eq!(h.state().attachment_count, 1);
}

#[gpui_kit::test]
fn reports_canceled_imports_and_exports(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    settings_tab(&mut h, "assets");
    h.dialogs.queue_open(None);
    h.click("import-asset");
    assert_eq!(h.notice().unwrap_or_default(), "Import canceled.");
    let file = fixture(&h, "note.txt", b"hello");
    import(&mut h, file);
    h.dialogs.queue_save(None);
    h.click("export-asset:note.txt");
    assert_eq!(h.notice().unwrap_or_default(), "Export canceled.");
}

#[gpui_kit::test]
fn stores_assets_with_safe_names_and_finds_unused_ones(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    settings_tab(&mut h, "assets");
    let reserved = fixture(&h, "CON.txt", b"reserved device name");
    import(&mut h, reserved);
    let used_file = fixture(&h, "used.csv", b"a,b\n1,2\n");
    import(&mut h, used_file);
    assert_eq!(h.label("asset-count"), "2 assets");
    assert!(h.label("asset-row:_CON.txt").contains("text/plain"));

    let assets = crate::list_attachments(h.label.clone()).unwrap();
    let used = assets.iter().find(|a| a.display_name == "used.csv").unwrap().id.clone();
    let store = h.read_shell(|s, _| s.store.clone());
    h.cx.update(|cx| {
        store.update(cx, |s, cx| {
            s.update("Use asset", move |c| c.settings = json!({ "importTemplate": used }), cx)
        })
    })
    .unwrap();
    h.settle();
    assert!(h.label("unused-assets-note").starts_with("No form, report, or setting refers to 1 asset."));
    assert_eq!((h.label("unused-asset:0"), h.exists("unused-asset:1")), ("_CON.txt".into(), false));
    assert_eq!(h.label("asset-unused:_CON.txt"), "Unused");
    assert!(!h.exists("asset-unused:used.csv"));
    h.click("remove-unused-assets");
    assert_eq!(h.notice().unwrap_or_default(), "Removed 1 unused asset.");
    assert_eq!(
        h.label("unused-assets-note"),
        "Every asset is referenced by the application."
    );

    let path = h.temp_path("with-assets.ixt");
    crate::save_document_as(h.label.clone(), path.to_string_lossy().into()).unwrap();
    let reopen = unique("reopen");
    crate::open_document(reopen.clone(), path.to_string_lossy().into()).unwrap();
    let reopened = crate::list_attachments(reopen.clone()).unwrap();
    crate::close_document(reopen, true).unwrap();
    let names: Vec<_> = reopened.iter().map(|a| a.display_name.as_str()).collect();
    assert_eq!(names, vec!["used.csv"]);
    assert_eq!(reopened[0].checksum.len(), 64);
}

#[gpui_kit::test]
fn views_a_text_asset_in_titled_sections(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    settings_tab(&mut h, "assets");
    let vba = "' ==== Module Helpers ====\nPublic Function Twice(x)\nEnd Function\n' ==== Form Customers ====\nPrivate Sub Form_Load()\nEnd Sub\n";
    let file = fixture(&h, "vba.txt", vba.as_bytes());
    import(&mut h, file);
    h.click("view-asset:vba.txt");
    assert_eq!(h.label("asset-preview"), "Preview of vba.txt");
    assert_eq!(h.label("preview-nav:0"), "Module Helpers");
    assert_eq!(h.label("preview-nav:1"), "Form Customers");
    assert!(h.label("preview-section:1").contains("Form_Load"));
    h.click("view-asset:vba.txt");
    assert!(!h.exists("asset-preview"));
    h.click("view-asset:vba.txt");
    h.click("close-preview");
    assert!(!h.exists("asset-preview"));
}

#[gpui_kit::test]
fn reports_archive_size_by_section_against_the_cloud_limit(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    settings_tab(&mut h, "assets");
    let photo = fixture(&h, "photo.bin", &vec![7u8; 200_000]);
    import(&mut h, photo);
    h.click("measure-archive");
    let largest: Vec<String> = (0..5)
        .filter_map(|i| h.try_label(id(&format!("size-largest:{i}"))))
        .collect();
    assert!(largest.iter().any(|l| l.contains("photo.bin")), "{largest:?}");
    assert_eq!(h.label("size-limit"), "Within the 500 MB cloud limit.");
    assert!(
        h.label("size-summary")
            .contains("Measured on a snapshot that includes unsaved changes.")
    );
    for section in ["Records", "Configuration", "Assets", "Other"] {
        assert!(h.exists(id(&format!("size-section:{section}"))), "{section}");
    }
}

#[gpui_kit::test]
fn creates_a_checkpoint_and_restores_it_as_a_separate_copy(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    settings_tab(&mut h, "assets");
    scroll_to_end(&mut h);
    h.click("create-checkpoint");
    assert_eq!(h.notice().as_deref(), Some("Checkpoint created."));
    assert!(h.label("checkpoint:0").contains("manual"));

    let copy = h.temp_path("restored-copy.ixt");
    h.dialogs.queue_save(Some(copy.clone()));
    h.click("restore-checkpoint:0");
    assert!(h.notice().unwrap_or_default().starts_with("Restored a copy to"));
    let current = h.state().document_id;
    let label = unique("copy");
    let restored = crate::open_document(label.clone(), copy.to_string_lossy().into()).unwrap();
    crate::close_document(label, true).unwrap();
    assert_ne!(restored.document_id, current);
}

#[gpui_kit::test]
fn shows_and_copies_the_diagnostic_log(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    crate::logging::info("ui-test", "a visible log line");
    settings_tab(&mut h, "logs");
    let first = h.label("log:0");
    assert!(!first.is_empty());
    h.click("logs-copy");
    let copied = h.notice().unwrap_or_default();
    assert!(copied.starts_with("Copied ") && copied.ends_with(" log lines."), "{copied}");
    let clipboard = h.cx.read(|cx| cx.read_from_clipboard()).and_then(|c| c.text());
    assert!(clipboard.unwrap().contains("a visible log line"));
    h.click("logs-refresh");
    assert!(h.label("logs-count").ends_with("recent lines"));
}

// ---- Autosave and recovery -----------------------------------------------------

fn saved_document(h: &mut Harness, name: &str) -> PathBuf {
    let path = archive_path(name);
    h.dialogs.queue_save(Some(path.clone()));
    h.click("save-project");
    assert_eq!(h.label("document-status"), "Saved");
    path
}

#[gpui_kit::test]
fn autosaves_edits_to_a_saved_document_and_shows_the_states(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let path = saved_document(&mut h, "autosave");
    assert!(h.label("save-status").starts_with("Saved at"));

    h.create_table("tasks", &[("id", "INTEGER", 1)]);
    assert_eq!(h.label("save-status"), "Unsaved changes");
    h.advance(Duration::from_millis(1600));
    assert!(h.label("save-status").starts_with("Saved at"), "{}", h.label("save-status"));
    let saved = h.state();
    assert!(!saved.dirty);
    assert!(saved.last_saved_at.is_some());

    let verify = unique("verify");
    crate::open_document(verify.clone(), path.to_string_lossy().into()).unwrap();
    let objects = crate::list_database_objects(verify.clone()).unwrap();
    crate::close_document(verify, true).unwrap();
    assert!(objects.iter().any(|o| o.name == "tasks"));
    let logs = crate::logging::read_logs(h.label.clone(), Some(200)).unwrap();
    let path_text = path.to_string_lossy().into_owned();
    assert!(
        logs.iter()
            .any(|l| l.area == "autosave" && l.message.contains(&path_text))
    );
}

#[gpui_kit::test]
fn surfaces_an_autosave_failure_and_retries_it_on_request(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let path = saved_document(&mut h, "retry");
    let moved = PathBuf::from(format!("{}.moved", path.display()));
    std::fs::rename(&path, &moved).unwrap();
    h.create_table("later", &[("id", "INTEGER", 1)]);
    h.advance(Duration::from_millis(1600));
    assert!(h.label("save-status").starts_with("Save failed"), "{}", h.label("save-status"));
    assert_eq!(h.state().last_error.unwrap().code, "MISSING_FILE");

    std::fs::rename(&moved, &path).unwrap();
    h.click("retry-save");
    assert!(h.label("save-status").starts_with("Saved at"), "{}", h.label("save-status"));
    assert!(!h.state().dirty);
}

#[gpui_kit::test]
fn never_autosaves_untitled_documents(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.create_table("loose", &[("id", "INTEGER", 1)]);
    h.advance(Duration::from_secs(20));
    assert_eq!(h.label("save-status"), "Unsaved changes. Save once to turn on autosave");
    assert!(h.state().dirty);
}

#[gpui_kit::test]
fn debounces_bursts_of_changes_into_one_save_after_the_quiet_period(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let _ = saved_document(&mut h, "burst");
    h.create_table("one", &[("id", "INTEGER", 1)]);
    h.advance(Duration::from_millis(1000));
    h.create_table("two", &[("id", "INTEGER", 1)]);
    h.advance(Duration::from_millis(1000));
    assert_eq!(h.label("save-status"), "Unsaved changes");
    h.advance(Duration::from_millis(600));
    assert!(h.label("save-status").starts_with("Saved at"));
}

static RECOVERY: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Recovery tests share one process-wide list of crashed sessions, so they run one at a
/// time on an emptied list (the start screen shows every record).
pub(crate) fn exclusive_recovery() -> std::sync::MutexGuard<'static, ()> {
    let guard = RECOVERY.lock().unwrap_or_else(|e| e.into_inner());
    for record in crate::list_recovery_sessions().unwrap_or_default() {
        let _ = crate::discard_recovery(record.session_id);
    }
    guard
}

pub(crate) fn seed_crashed_session(window: &str, name: &str, path: Option<&PathBuf>) -> SessionState {
    crate::new_document(window.into()).unwrap();
    let mut config = crate::read_document_config(window.into()).unwrap();
    config.name = name.into();
    crate::update_document_config(window.into(), config).unwrap();
    if let Some(path) = path {
        crate::save_document_as(window.into(), path.to_string_lossy().into()).unwrap();
    }
    let spec = json!({
        "name": "notes",
        "columns": [{
            "name": "id", "declaredType": "INTEGER", "nullable": false, "primaryKeyPosition": 1,
            "unique": false, "defaultExpression": null, "generatedExpression": null,
        }],
        "foreignKeys": [], "checks": [], "withoutRowid": false,
    });
    crate::create_database_table(window.into(), serde_json::from_value(spec).unwrap()).unwrap();
    let values = vec![crate::data::NamedValue {
        column: "id".into(),
        value: serde_json::from_value(json!({ "type": "integer", "value": 7 })).unwrap(),
    }];
    crate::recordstore::commands::insert_row(window.into(), "notes".into(), values, None).unwrap();
    let crashed = crate::document_state(window.into()).unwrap();
    assert!(crashed.dirty);
    crate::recovery::simulate_crash(window.into()).unwrap();
    crashed
}

fn notes_in_archive(path: &PathBuf) -> Option<usize> {
    let window = unique("verify");
    crate::open_document(window.clone(), path.to_string_lossy().into()).unwrap();
    let total = crate::read_table_page(window.clone(), "notes".into(), 0, 10, vec![], vec![], None)
        .ok()
        .map(|p| p.total as usize);
    crate::close_document(window, true).unwrap();
    total
}

#[gpui_kit::test]
fn recovers_a_crashed_sessions_unsaved_work_into_its_archive(cx: &mut TestAppContext) {
    let _guard = exclusive_recovery();
    let path = archive_path("crash");
    let name = unique("Crash Notes");
    let crashed = seed_crashed_session(&unique("crashy"), &name, Some(&path));
    assert_eq!(notes_in_archive(&path), None);

    let mut h = Harness::start(cx);
    assert!(h.exists("recover-unsaved-work"));
    h.click(id(&format!("recover:{name}")));
    assert_eq!(h.label("document-status"), "Saved");
    let recovered = h.state();
    assert_eq!(recovered.session_id, crashed.session_id);
    assert!(!recovered.dirty);
    assert_eq!(h.read_page("notes").total, 1);
    assert_eq!(notes_in_archive(&path), Some(1));
    let checkpoints = crate::checkpoints::list_checkpoints(h.label.clone()).unwrap();
    assert!(checkpoints.iter().any(|c| c.reason == "before-recovery"));
    let left = crate::list_recovery_sessions().unwrap();
    assert!(left.iter().all(|r| r.session_id != crashed.session_id));
}

#[gpui_kit::test]
fn asks_where_to_save_recovered_work_that_was_never_saved(cx: &mut TestAppContext) {
    let _guard = exclusive_recovery();
    let name = unique("Draft Ideas");
    seed_crashed_session(&unique("untitled"), &name, None);
    let destination = archive_path("rescued");
    let mut h = Harness::start(cx);
    h.dialogs.queue_save(Some(destination.clone()));
    h.click(id(&format!("recover:{name}")));
    assert_eq!(h.label("document-status"), "Saved");
    assert_eq!(h.dialogs.count("save"), 1);
    assert_eq!(h.state().path.as_deref(), Some(destination.to_str().unwrap()));
    assert_eq!(notes_in_archive(&destination), Some(1));
}

#[gpui_kit::test]
fn asks_for_save_as_instead_of_overwriting_a_file_it_cannot_checkpoint(cx: &mut TestAppContext) {
    let _guard = exclusive_recovery();
    let path = archive_path("unreadable");
    let name = unique("Garbled Notes");
    seed_crashed_session(&unique("garbled"), &name, Some(&path));
    std::fs::write(&path, "not an archive any more").unwrap();
    let before = std::fs::read(&path).unwrap();
    let destination = archive_path("garbled-rescued");

    let mut h = Harness::start(cx);
    h.dialogs.queue_save(Some(destination.clone()));
    h.click(id(&format!("recover:{name}")));
    assert_eq!(h.label("document-status"), "Saved");
    assert_eq!(h.dialogs.count("save"), 1);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(h.state().path.as_deref(), Some(destination.to_str().unwrap()));
    assert_eq!(notes_in_archive(&destination), Some(1));
}

#[gpui_kit::test]
fn keeps_the_last_valid_archive_when_recovered_work_is_invalid_then_discards_it(
    cx: &mut TestAppContext,
) {
    let _guard = exclusive_recovery();
    let path = archive_path("invalid");
    let name = unique("Broken Work");
    let crashed = seed_crashed_session(&unique("broken"), &name, Some(&path));
    let before = std::fs::read(&path).unwrap();
    let workspace = PathBuf::from(&crashed.workspace);
    std::fs::write(workspace.join("document.json"), "{ truncated").unwrap();
    std::fs::write(workspace.join("config.yaml"), "name: [").unwrap();

    let mut h = Harness::start(cx);
    h.click(id(&format!("recover:{name}")));
    let error = h.error().expect("an error banner");
    assert!(error.contains("RECOVERY_FAILED"), "{error}");
    assert!(error.contains("The last saved archive was kept"), "{error}");
    assert_eq!(std::fs::read(&path).unwrap(), before);

    h.dialogs.queue_confirm(true);
    h.click(id(&format!("discard:{name}")));
    assert!(!h.exists("recover-unsaved-work"));
    assert!(!workspace.exists());
}

#[gpui_kit::test]
fn ignores_leftovers_of_an_interrupted_write_and_cleans_them_up_on_the_next_save(
    cx: &mut TestAppContext,
) {
    let path = archive_path("interrupted");
    let seed = unique("seed");
    crate::new_document(seed.clone()).unwrap();
    crate::save_document_as(seed.clone(), path.to_string_lossy().into()).unwrap();
    crate::close_document(seed, true).unwrap();
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let leftover = path.with_file_name(format!(".{name}.crashed.tmp"));
    std::fs::write(&leftover, &std::fs::read(&path).unwrap()[..512]).unwrap();

    let mut h = Harness::start(cx);
    crate::open_document(h.label.clone(), path.to_string_lossy().into()).unwrap();
    crate::save_document(h.label.clone()).unwrap();
    assert!(!leftover.exists());
    assert!(h.state().last_error.is_none());
    let _ = &mut h;
}

// ---- Forwarded files -------------------------------------------------------------

fn open_request(h: &mut Harness, path: &str) {
    let root = h.root.clone();
    let path = path.to_string();
    h.cx.update(|cx| root.update(cx, |view, cx| view.open_request(path, cx)));
    h.settle();
}

fn seed_document(name: &str) -> PathBuf {
    let window = unique("seed");
    crate::new_document(window.clone()).unwrap();
    let mut config = crate::read_document_config(window.clone()).unwrap();
    config.name = name.into();
    crate::update_document_config(window.clone(), config).unwrap();
    let path = archive_path(&name.replace(' ', "-"));
    crate::save_document_as(window.clone(), path.to_string_lossy().into()).unwrap();
    crate::close_document(window, true).unwrap();
    path
}

#[gpui_kit::test]
fn opens_a_forwarded_document_from_the_start_screen(cx: &mut TestAppContext) {
    let name = unique("Launch one");
    let path = seed_document(&name);
    let mut h = Harness::start(cx);
    open_request(&mut h, path.to_str().unwrap());
    assert!(h.has_shell());
    assert_eq!(h.label("document-name"), name);
}

#[gpui_kit::test]
fn asks_before_a_forwarded_document_replaces_unsaved_work(cx: &mut TestAppContext) {
    let name = unique("Launch two");
    let path = seed_document(&name);
    let mut h = Harness::new_document(cx);
    h.switch_mode(ModeId::App);
    assert!(h.state().dirty);
    h.dialogs.queue_confirm(false);
    open_request(&mut h, path.to_str().unwrap());
    assert!(h.notice().unwrap().contains("Kept Untitled open"));
    assert_eq!(h.label("document-name"), "Untitled");

    h.dialogs.queue_confirm(true);
    open_request(&mut h, path.to_str().unwrap());
    assert_eq!(h.label("document-name"), name);
    assert_eq!(h.dialogs.count("confirm"), 2);
}

// ---- Archives written by a newer ixtable ------------------------------------------

fn working_copy(h: &Harness, file: &str) -> PathBuf {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures/archives/newer-build")
        .join(file);
    let dest = h.temp_path(file);
    std::fs::copy(source, &dest).unwrap();
    dest
}

fn sha(path: &PathBuf) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

#[gpui_kit::test]
fn refuses_newer_archives_with_an_update_hint_and_leaves_them_untouched(cx: &mut TestAppContext) {
    let mut h = Harness::start(cx);
    for (file, message) in [
        (
            "newer-format.ixt",
            "This document was created by a newer ixtable (format 99). Update ixtable to open it.",
        ),
        (
            "newer-config.ixt",
            "This document's configuration (version 99) was created by a newer ixtable. Update ixtable to open it.",
        ),
    ] {
        let path = working_copy(&h, file);
        let before = sha(&path);
        open_request(&mut h, path.to_str().unwrap());
        let error = h.error().expect("an error banner");
        assert!(error.contains("UNSUPPORTED_VERSION"), "{error}");
        assert!(error.contains(message), "{error}");
        assert!(!h.has_shell());
        assert_eq!(sha(&path), before);
    }
}

#[gpui_kit::test]
fn opens_an_archive_with_unknown_entries_at_the_current_versions(cx: &mut TestAppContext) {
    let mut h = Harness::start(cx);
    let path = working_copy(&h, "unknown-entries.ixt");
    open_request(&mut h, path.to_str().unwrap());
    assert!(h.has_shell());
    assert!(h.error().is_none());
}
