//! Shell and document flows (port of tests/integration/shell.test.tsx and
//! document.test.tsx). Imports are explicit: `gpui_kit::*` would shadow `#[test]`.

use super::{ModeId, settings::AppSettings, yaml::YamlTab};
use crate::ui::testing::Harness;
use gpui_kit::{Entity, TestAppContext};

fn yaml_tab(h: &mut Harness) -> Entity<YamlTab> {
    h.switch_mode(ModeId::App);
    h.click("tab-yaml");
    let view = h.read_shell(|s, _| s.mode_view()).expect("settings view");
    let settings: Entity<AppSettings> = view.downcast().expect("AppSettings");
    let tab = h.cx.read(|cx| settings.read(cx).tab_view()).expect("yaml tab");
    tab.downcast().expect("YamlTab")
}

fn yaml_text(h: &mut Harness, tab: &Entity<YamlTab>) -> String {
    h.cx.read(|cx| tab.read(cx).editor.read(cx).value().to_string())
}

fn set_yaml(h: &mut Harness, tab: &Entity<YamlTab>, text: &str) {
    let editor = h.cx.read(|cx| tab.read(cx).editor.clone());
    h.update(|window, cx| editor.update(cx, |e, cx| e.set_value(text.to_string(), window, cx)));
    h.settle();
}

#[gpui_kit::test]
fn new_document_opens_the_shell(cx: &mut TestAppContext) {
    let mut h = Harness::start(cx);
    assert_eq!(h.label("start-heading"), "Your data, in one portable file.");
    h.click("new-document");
    assert!(h.has_shell());
    assert_eq!(h.label("document-name"), "Untitled");
    assert_eq!(h.label("document-status"), "Not saved yet");
    assert!(!h.can_undo());
}

#[gpui_kit::test]
fn first_save_chooses_a_destination_then_saves_in_place(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let destination = h.temp_path("created.ixt");
    h.dialogs.queue_save(Some(destination.clone()));
    h.click("save-project");
    assert_eq!(h.label("document-status"), "Saved archive");
    assert_eq!(h.dialogs.count("save"), 1);
    h.click("save-project");
    assert_eq!(h.dialogs.count("save"), 1);
    assert_eq!(h.state().path.as_deref(), Some(destination.to_str().unwrap()));
}

#[gpui_kit::test]
fn save_as_moves_an_existing_document(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let (first, second) = (h.temp_path("first.ixt"), h.temp_path("second.ixt"));
    h.dialogs.queue_save(Some(first));
    h.dialogs.queue_save(Some(second.clone()));
    h.click("save-project");
    h.click("save-project-as");
    assert_eq!(h.state().path.as_deref(), Some(second.to_str().unwrap()));
}

#[gpui_kit::test]
fn picker_cancellation_is_a_notice_not_an_error(cx: &mut TestAppContext) {
    let mut h = Harness::start(cx);
    h.dialogs.queue_open(None);
    h.click("open-document");
    assert_eq!(h.notice().as_deref(), Some("Open canceled."));
    assert!(h.error().is_none());

    h.click("new-document");
    h.dialogs.queue_save(None);
    h.click("save-project");
    assert_eq!(h.notice().as_deref(), Some("Save canceled."));
    assert!(h.error().is_none());
}

#[gpui_kit::test]
fn opens_an_archive_and_reports_an_invalid_one(cx: &mut TestAppContext) {
    let archive = {
        let seed = format!("seed-{}", uuid::Uuid::new_v4());
        crate::new_document(seed.clone()).unwrap();
        let path = crate::paths::state_dir().join(format!("{seed}.ixt"));
        crate::save_document_as(seed.clone(), path.to_string_lossy().into()).unwrap();
        crate::close_document(seed, true).unwrap();
        path
    };
    let mut h = Harness::start(cx);
    h.dialogs.queue_open(Some(archive));
    h.click("open-document");
    assert_eq!(h.label("document-status"), "Saved archive");

    let invalid = h.temp_path("invalid.ixt");
    std::fs::write(&invalid, "not an ixtable archive").unwrap();
    h.click("close-project");
    h.dialogs.queue_open(Some(invalid));
    h.click("open-document");
    assert!(h.error().unwrap().contains("INVALID_ARCHIVE"));
}

#[gpui_kit::test]
fn lists_recent_documents_and_opens_one(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let name = format!("recent-{}.ixt", h.label);
    let path = h.temp_path(&name);
    h.dialogs.queue_save(Some(path.clone()));
    h.click("save-project");
    h.click("close-project");
    h.click(gpui_kit::SharedString::from(format!("recent:{name}")));
    assert_eq!(h.label("document-status"), "Saved archive");
    assert_eq!(h.state().path.as_deref(), Some(path.to_str().unwrap()));
}

#[gpui_kit::test]
fn switches_modes_and_persists_the_active_mode(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    for (mode, heading) in [
        (ModeId::Query, "Query"),
        (ModeId::Reports, "Reports"),
        (ModeId::Dashboards, "Dashboards"),
        (ModeId::Automation, "Automation"),
        (ModeId::Run, "Runtime"),
        (ModeId::App, "Application settings"),
        (ModeId::Design, "Form builder"),
    ] {
        h.switch_mode(mode);
        assert_eq!(h.label("mode-title"), heading, "{mode:?}");
        assert_eq!(h.mode(), mode);
    }
    assert_eq!(h.config().active_mode, "design");
}

#[gpui_kit::test]
fn settings_is_an_icon_button_beside_save(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let save = h.find("save-project").bounds();
    let settings = h.find(ModeId::App.button_id()).bounds();
    assert_eq!(settings.origin.y, save.origin.y);
    assert!(settings.origin.x > save.origin.x);
    assert_eq!(settings.size, save.size);
    h.switch_mode(ModeId::App);
    assert_eq!(h.label("mode-title"), "Application settings");
}

#[gpui_kit::test]
fn keeps_the_mode_out_of_undo_history(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.switch_mode(ModeId::Reports);
    assert!(!h.can_undo());
}

#[gpui_kit::test]
fn edits_yaml_reports_errors_and_undoes_an_applied_change(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let tab = yaml_tab(&mut h);
    assert!(yaml_text(&mut h, &tab).contains("name: Untitled"));

    set_yaml(&mut h, &tab, "name: [unclosed");
    h.click("apply-yaml");
    assert!(h.label("yaml-error").contains("INVALID_CONFIG"));

    set_yaml(&mut h, &tab, "name: Inventory\nactiveMode: app\nversion: 2\n");
    h.click("apply-yaml");
    assert_eq!(h.label("yaml-status"), "YAML applied.");
    assert!(!h.exists("yaml-error"));
    assert_eq!(h.label("document-name"), "Inventory");
    assert!(yaml_text(&mut h, &tab).contains("version: 7"));

    h.click("undo");
    assert_eq!(h.config().name, "Untitled");
    assert!(yaml_text(&mut h, &tab).contains("name: Untitled"));
}

#[gpui_kit::test]
fn lists_validation_problems(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.switch_mode(ModeId::App);
    h.click("tab-problems");
    assert_eq!(h.label("no-problems"), "No problems found.");

    let tab = yaml_tab(&mut h);
    set_yaml(
        &mut h,
        &tab,
        "name: Broken\nactiveMode: app\nversion: 3\nreports:\n  - id: r1\n    name: Sales\n  - id: r1\n    name: ''\n",
    );
    h.click("apply-yaml");
    assert_eq!(h.label("yaml-status"), "YAML applied.");
    h.click("tab-problems");
    let problems: Vec<String> = (0..2usize).map(|i| h.label(("problem", i))).collect();
    assert!(problems.iter().any(|p| p.contains("duplicate report id r1")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("report r1 has no name")), "{problems:?}");
    assert_eq!(h.label("problems-summary"), "1 error, 1 warning");
}

#[gpui_kit::test]
fn undoes_and_redoes_config_edits_from_the_toolbar_and_keyboard(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let store = h.read_shell(|s, _| s.store.clone());
    h.cx.update(|cx| {
        store.update(cx, |s, cx| s.update("Rename", |c| c.name = "Answer".into(), cx))
    })
    .unwrap();
    h.settle();
    assert_eq!(h.label("document-name"), "Answer");

    h.click("undo");
    assert_eq!(h.config().name, "Untitled");
    h.click("redo");
    assert_eq!(h.config().name, "Answer");

    h.click("shell");
    h.press("secondary-z");
    assert_eq!(h.config().name, "Untitled");
    h.press("secondary-shift-z");
    assert_eq!(h.config().name, "Answer");
}

#[gpui_kit::test]
fn confirms_before_closing_a_dirty_document(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.switch_mode(ModeId::Design);
    assert!(h.state().dirty);
    h.dialogs.queue_confirm(false);
    h.click("close-project");
    assert!(h.has_shell());
    h.dialogs.queue_confirm(true);
    h.click("close-project");
    assert!(!h.has_shell());
    assert_eq!(h.label("start-heading"), "Your data, in one portable file.");
    assert_eq!(h.dialogs.count("confirm"), 2);
}
