//! Runtime bundles: export, password prompt, runtime-only window, update, confirm dialogs
//! and diagnostics (`release-*`). Port of scripts/screenshot/specs/release.spec.tsx.

use crate::data::DataValue;
use crate::ui::data::testkit::choose;
use crate::ui::persistence::tests::unique;
use crate::ui::runtime::tests::{nav, wait};
use crate::ui::screenshot::context;
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use gpui_kit::HeadlessAppContext;
use serde_json::json;
use std::path::PathBuf;

/// Tall enough that the export form needs no scrolling.
const TALL: (f32, f32) = (1280.0, 1500.0);

fn bundle(h: &Harness<'_, HeadlessAppContext>, name: &str) -> PathBuf {
    h.temp_path(&format!("{}-{name}", unique("b")))
}

fn export(h: &mut Harness<'_, HeadlessAppContext>, version: &str, path: &PathBuf, password: Option<&str>) {
    h.fill("release-version", version);
    if let Some(password) = password {
        if !h.exists("release-password") {
            h.click("release-protect");
        }
        h.fill("release-password", password);
        h.fill("release-confirm", password);
    }
    h.dialogs.queue_save(Some(path.clone()));
    h.click("export-bundle");
    assert_eq!(
        h.label("exported-bundle"),
        format!("Exported Untitled {version}"),
        "{:?}",
        h.try_label("release-error")
    );
}

fn seed(h: &mut Harness<'_, HeadlessAppContext>) {
    h.create_table("customers", &[("id", "INTEGER", 1), ("name", "TEXT", 0), ("city", "TEXT", 0)]);
    h.insert_row("customers", &[("name", DataValue::Text("Ada".into())), ("city", DataValue::Text("Leeds".into()))]);
}

fn close_to_start(h: &mut Harness<'_, HeadlessAppContext>) {
    h.dialogs.queue_confirm(true);
    h.click("close-project");
    assert!(h.exists("open-bundle"));
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn release_export_runtime_and_update() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, TALL);
    h.click("new-document");
    assert!(h.has_shell());
    seed(&mut h);
    // Give recipients a real page: a generated Customers list and detail form.
    h.switch_mode(ModeId::Design);
    choose(&mut h, "generate-table", "customers");
    h.click("generate-form");
    assert!(h.config().design.forms.iter().any(|f| f.name == "Customers"));
    h.switch_mode(ModeId::App);
    h.click("tab-release");
    let v1 = bundle(&h, "orders-1.0.0.ixtr");
    export(&mut h, "1.0.0", &v1, Some("correct horse"));
    h.capture(
        "release-01-exported",
        &[
            "The Release tab shows version, notes, and password protection settings.",
            "The Exported bundle panel names Untitled 1.0.0 with its signer fingerprint and protection.",
        ],
    );
    let v2 = bundle(&h, "orders-1.1.0.ixtr");
    export(&mut h, "1.1.0", &v2, Some("correct horse"));
    close_to_start(&mut h);

    h.dialogs.queue_open(Some(v1));
    h.click("open-bundle");
    assert_eq!(h.label("bundle-password"), "Password for Untitled 1.0.0");
    h.fill("bundle-password-input", "correct horse");
    h.capture(
        "release-02-password",
        &["A password dialog for Untitled 1.0.0 sits over the start screen."],
    );
    h.click("bundle-unlock");
    assert_eq!(h.label("runtime-version"), "Version 1.0.0");
    h.insert_row("customers", &[("name", DataValue::Text("Runtime Rita".into())), ("city", DataValue::Text("Bath".into()))]);
    for hidden in ["save-project", "mode-data", "mode-app", "undo", "redo"] {
        assert!(!h.exists(hidden), "{hidden}");
    }
    assert_eq!(h.read_page("customers").total, 2);
    let customers = nav(&mut h, "Customers");
    h.click(customers);
    wait(&mut h);
    assert!((0..5).any(|n| h.try_label(format!("cell:{n}:name")).as_deref() == Some("Runtime Rita")));
    h.capture(
        "release-03-runtime-only",
        &[
            "The runtime-only window shows a Runtime bundle bar with Version 1.0.0.",
            "No ribbon (Undo/Redo), mode switch, Preview as role, PROJECT breadcrumb, or Save buttons.",
            "The Customers list shows the bundled records plus the recipient's own Runtime Rita.",
        ],
    );

    h.dialogs.queue_open(Some(v2));
    h.click("check-update");
    // The 1.1.0 bundle is protected too.
    if h.exists("bundle-password-input") {
        h.fill("bundle-password-input", "correct horse");
        h.click("bundle-unlock");
    }
    assert_eq!(h.label("update-confirm"), "Update Untitled to 1.1.0?");
    h.click("apply-update");
    assert!(h.notice().unwrap().contains("Updated to version 1.1.0. Your records were kept."));
    assert_eq!(h.read_page("customers").total, 2);
    h.capture(
        "release-04-updated",
        &["The Runtime bundle bar reads Version 1.1.0 with a 'records were kept' confirmation."],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn release_confirm_downgrade_and_diagnostics() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, TALL);
    h.click("new-document");
    seed(&mut h);
    h.switch_mode(ModeId::App);
    h.click("tab-release");
    let v1 = bundle(&h, "sales-1.0.0.ixtr");
    export(&mut h, "1.0.0", &v1, None);
    let mut config = serde_json::to_value(crate::read_document_config(h.label.clone()).unwrap()).unwrap();
    config["migrations"] = json!([{
        "id": "0190c3f4-eeee-7bbb-8ccc-0123456789ab",
        "name": "Add delivery slots",
        "order": 1,
        "targetStore": "sqlite",
        "up": "CREATE TABLE delivery_slots (id INTEGER PRIMARY KEY, label TEXT)",
    }]);
    crate::update_document_config(h.label.clone(), serde_json::from_value(config).unwrap()).unwrap();
    crate::migrations::commands::apply_migrations(h.label.clone()).unwrap();
    let v2 = bundle(&h, "sales-2.0.0.ixtr");
    let options = serde_json::from_value(json!({
        "version": "2.0.0",
        "releaseNotes": "Adds delivery slots.\nFixes the order total rounding.\nFaster customer search.",
    }))
    .unwrap();
    crate::bundle_export::export_runtime_bundle(h.label.clone(), v2.to_string_lossy().into(), options).unwrap();
    close_to_start(&mut h);
    h.dialogs.queue_open(Some(v1.clone()));
    h.click("open-bundle");
    assert_eq!(h.label("runtime-version"), "Version 1.0.0");

    h.dialogs.queue_open(Some(v2));
    h.click("check-update");
    assert_eq!(h.label("update-confirm"), "Update Untitled to 2.0.0?");
    assert_eq!(h.label("migration:0"), "Add delivery slots");
    assert_eq!(h.label("runtime-version"), "Version 1.0.0");
    h.capture(
        "release-05-update-confirm",
        &[
            "The 'Update Untitled to 2.0.0?' dialog shows three release-note lines on separate lines.",
            "The Migrations list names 'Add delivery slots' with a collapsed SQL disclosure.",
            "The Runtime bundle bar behind still reads Version 1.0.0.",
        ],
    );
    h.click("apply-update");
    assert_eq!(h.label("runtime-version"), "Version 2.0.0");

    h.dialogs.queue_open(Some(v1));
    h.click("check-update");
    assert_eq!(h.label("update-confirm"), "Install older version 1.0.0 of Untitled?");
    assert_eq!(h.label("no-migrations"), "No migrations will run on your records.");
    h.capture(
        "release-06-downgrade-confirm",
        &[
            "The dialog asks 'Install older version 1.0.0 of Untitled?' and says no migrations will run.",
            "Cancel and a confirm button are offered; the bar behind reads Version 2.0.0.",
        ],
    );
    h.click("cancel-update");
    assert_eq!(h.label("runtime-version"), "Version 2.0.0");

    h.click("diagnostics-open");
    assert_eq!(h.label("diagnostics"), "Diagnostics");
    assert_eq!(h.label("jobs-empty"), "No jobs.");
    h.click("diagnostics-tab-logs");
    let lines: Vec<String> = (0..60)
        .filter_map(|i| h.try_label(crate::ui::forms::eid(format!("log:{i}"))))
        .collect();
    assert!(lines.iter().any(|l| l.contains("Untitled 2.0.0")), "{lines:?}");
    h.capture(
        "release-07-diagnostics",
        &[
            "The Diagnostics dialog has Background jobs and Logs tabs with Logs selected.",
            "Log entries include the update to Untitled 2.0.0.",
        ],
    );
}
