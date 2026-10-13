//! Migration problems and preflight errors (`migrations-*`). Port of
//! scripts/screenshot/specs/migrations.spec.tsx.

use crate::ui::screenshot::context;
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use gpui_kit::HeadlessAppContext;
use serde_json::{Value, json};

const TALL: (f32, f32) = (1280.0, 2000.0);

/// Edits the stored definition behind the UI, then remounts the tab so it re-reads it.
fn load_config(h: &mut Harness<'_, HeadlessAppContext>, edit: impl FnOnce(&mut Value)) {
    let config = crate::read_document_config(h.label.clone()).unwrap();
    let mut value = serde_json::to_value(config).unwrap();
    edit(&mut value);
    crate::update_document_config(h.label.clone(), serde_json::from_value(value).unwrap()).unwrap();
    h.click("tab-datasource");
    h.click("tab-migrations");
    assert_eq!(h.label("migrations"), "Migrations");
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn migrations_problems_and_preflight() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, TALL);
    h.click("new-document");
    assert!(h.has_shell());
    h.create_table("items", &[("id", "INTEGER", 1), ("label", "TEXT", 0)]);
    h.switch_mode(ModeId::App);
    h.click("tab-migrations");
    load_config(&mut h, |c| {
        c["design"]["forms"][0]["source"] = json!({ "kind": "table", "table": "items" });
        c["design"]["forms"][0]["controls"] = json!([{
            "id": uuid::Uuid::new_v4().to_string(), "kind": "text",
            "label": "Item label", "binding": { "column": "label" },
        }]);
        c["migrations"] = json!([
            { "id": "m-drop", "name": "Drop label", "order": 1, "up": "ALTER TABLE items DROP COLUMN label;" }
        ]);
    });
    assert!(h.exists("migration-name:Drop label"));
    h.click("apply-migrations");
    let problem = h.label("after-problem:0");
    assert!(problem.contains("\"Item label\" is bound to items.label"), "{problem}");
    let health = h.label("history:0:health");
    assert!(health.starts_with("Health passed: ") && health.contains("integrity_check: ok"), "{health}");
    h.capture(
        "migrations-01-problems-after",
        &[
            "A 'Problems after migration' panel says the Item label control is bound to the dropped items.label.",
            "The migration log shows the run succeeded with a passed health check.",
        ],
    );

    load_config(&mut h, |c| {
        c["migrations"] = json!([
            { "id": "m-drop", "name": "Drop label", "order": 1, "up": "ALTER TABLE items DROP COLUMN id;" },
            { "id": "m-tags", "name": "Add tags", "order": 2, "up": "CREATE TABLE tags (id INTEGER);" },
        ]);
    });
    assert_eq!(h.label("migration-status:Drop label"), "Changed after apply");
    h.click("dry-run");
    assert!(h.label("migrations-error").contains("Drop label changed after it was applied"));
    assert!(!h.exists("dry-run-status"));
    assert_eq!(h.find("apply-migrations").label(), Some("Apply pending (1)"));
    h.capture(
        "migrations-02-dry-run-preflight-error",
        &[
            "Drop label is marked 'Changed after apply' and Add tags is pending in the migration list.",
            "The dry run shows an error that Drop label changed after it was applied; nothing ran.",
            "'Apply pending (1)' is disabled after the failed dry run.",
        ],
    );
}
