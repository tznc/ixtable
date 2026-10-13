//! Ports of tests/integration/{migrations,migrations-editor,migrations-hardening}.test.tsx.
//! Imports are explicit: `gpui_kit::*` would shadow `#[test]`.

use crate::ui::{shell::ModeId, testing::Harness};
use gpui_kit::{ScrollDelta, SharedString, TestAppContext, point, px, test::TestWindowExt as _};
use serde_json::{Value, json};

fn id(text: &str) -> SharedString {
    SharedString::from(text.to_string())
}

fn open(h: &mut Harness) {
    h.switch_mode(ModeId::App);
    h.click("tab-migrations");
    assert_eq!(h.label("migrations"), "Migrations");
}

fn scroll_to_end(h: &mut Harness) {
    h.update(|w, cx| {
        w.scroll("settings-panel", ScrollDelta::Pixels(point(px(0.), px(-5000.))), cx)
    });
    h.settle();
}

fn scroll_to_start(h: &mut Harness) {
    h.update(|w, cx| {
        w.scroll("settings-panel", ScrollDelta::Pixels(point(px(0.), px(5000.))), cx)
    });
    h.settle();
}

/// Scrolls the settings panel until `target` is on screen.
fn reveal(h: &mut Harness, target: &str) {
    scroll_to_start(h);
    for _ in 0..16 {
        if h.try_find(id(target)).is_some_and(|s| s.visible()) {
            return;
        }
        h.update(|w, cx| {
            w.scroll("settings-panel", ScrollDelta::Pixels(point(px(0.), px(-300.))), cx)
        });
        h.settle();
    }
}

fn apply_label(h: &mut Harness) -> Option<String> {
    reveal(h, "apply-migrations");
    h.find(id("apply-migrations")).label().map(str::to_string)
}

fn tap(h: &mut Harness, target: &str) {
    reveal(h, target);
    h.click(id(target));
}

fn fill(h: &mut Harness, target: &str, text: &str) {
    reveal(h, target);
    h.fill(id(target), text);
}

/// Whether a click on a disabled button did nothing: no run, no dry run, no error.
fn assert_inert(h: &mut Harness, button: &str) {
    tap(h, button);
    for result in ["run-status", "dry-run-status", "migrations-error"] {
        assert!(!h.exists(id(result)), "{button} ran: {result}");
    }
}

fn author(h: &mut Harness, name: &str, up: &str, down: Option<&str>) {
    tap(h, "new-migration");
    fill(h, "migration-name", name);
    fill(h, "migration-up", up);
    if let Some(down) = down {
        tap(h, "migration-reversible");
        fill(h, "migration-down", down);
    }
    tap(h, "migration-save");
    assert!(h.exists(id(&format!("migration-name:{name}"))), "{name} is listed");
}

fn objects(h: &Harness) -> Vec<String> {
    crate::list_database_objects(h.label.clone())
        .unwrap()
        .into_iter()
        .map(|o| o.name)
        .collect()
}

fn status_of(h: &mut Harness, name: &str) -> String {
    h.label(id(&format!("migration-status:{name}")))
}

/// Edits the stored definition behind the UI, then remounts the tab so it re-reads it.
fn edit_config(h: &mut Harness, edit: impl FnOnce(&mut Value)) {
    let config = crate::read_document_config(h.label.clone()).unwrap();
    let mut value = serde_json::to_value(config).unwrap();
    edit(&mut value);
    crate::update_document_config(h.label.clone(), serde_json::from_value(value).unwrap()).unwrap();
    h.click("tab-datasource");
    h.click("tab-migrations");
    assert_eq!(h.label("migrations"), "Migrations");
}

fn failure_of(h: &Harness, command: &str) -> String {
    let label = h.label.clone();
    let result = match command {
        "apply_migrations" => crate::migrations::commands::apply_migrations(label).map(|_| ()),
        "rollback_migration" => crate::migrations::commands::rollback_migration(label).map(|_| ()),
        _ => crate::migrations::commands::dry_run_migrations(label, None).map(|_| ()),
    };
    match result {
        Ok(()) => "succeeded".into(),
        Err(e) => format!("{}: {}", e.code, e.message),
    }
}

#[gpui_kit::test]
fn dry_runs_applies_with_a_checkpoint_logs_and_rolls_back_a_reversible_migration(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    open(&mut h);
    author(
        &mut h,
        "Create audit",
        "CREATE TABLE audit (id INTEGER PRIMARY KEY, note TEXT NOT NULL);",
        Some("DROP TABLE audit;"),
    );

    tap(&mut h, "dry-run");
    assert_eq!(h.label("dry-run-status"), "Dry run succeeded; nothing was changed.");
    assert!(!objects(&h).contains(&"audit".to_string()));

    tap(&mut h, "apply-migrations");
    let status = h.label("run-status");
    assert!(
        status.starts_with("All migrations succeeded. Checkpoint ")
            && status.ends_with(" was saved first."),
        "{status}"
    );
    let all = objects(&h);
    assert!(all.contains(&"audit".to_string()));
    assert!(!all.iter().any(|name| name.starts_with("_ixtable_")));
    scroll_to_end(&mut h);
    assert_eq!(h.label("history:0:status"), "applied");
    assert_eq!(status_of(&mut h, "Create audit"), "Applied");

    scroll_to_start(&mut h);
    tap(&mut h, "edit-migration:Create audit");
    // Applied migrations are locked.
    assert!(h.label("migration-id-note").contains("its SQL is locked"));
    tap(&mut h, "migration-preview-up");
    assert_eq!(h.label("preview-summary"), "1 statement on sqlite, in one transaction");
    tap(&mut h, "migration-close");
    assert!(!h.exists("migration-editor"));

    tap(&mut h, "rollback-migration");
    assert!(!objects(&h).contains(&"audit".to_string()));
    scroll_to_end(&mut h);
    assert_eq!(h.label("history:1:status"), "rolled_back");
}

#[gpui_kit::test]
fn stops_on_a_failing_migration_rolls_it_back_and_shows_recovery_instructions(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    open(&mut h);
    author(
        &mut h,
        "Make items",
        "CREATE TABLE items (id INTEGER PRIMARY KEY, label TEXT NOT NULL);",
        None,
    );
    author(
        &mut h,
        "Broken seed",
        "INSERT INTO items (label) VALUES ('ok'); INSERT INTO items (label) VALUES (NULL);",
        None,
    );

    tap(&mut h, "dry-run");
    assert_eq!(h.label("dry-run-status"), "Dry run failed; nothing was changed.");

    assert_eq!(apply_label(&mut h), Some("Apply pending (2)".to_string()));
    tap(&mut h, "apply-migrations");
    assert_eq!(h.label("run-status").split(" Checkpoint").next().unwrap(),
        "A migration failed; later ones did not run.");
    scroll_to_end(&mut h);
    let failures: Vec<String> = (0..3).filter_map(|i| h.try_label(id(&format!("run:{i}:error")))).collect();
    assert!(
        failures.iter().any(|e| e.to_lowercase().contains("not null constraint failed")),
        "{failures:?}"
    );
    let recoveries: Vec<String> =
        (0..3).filter_map(|i| h.try_label(id(&format!("run:{i}:recovery")))).collect();
    assert!(
        recoveries
            .iter()
            .any(|r| r.contains("Recovery: \"Broken seed\" failed and its transaction was rolled back")),
        "{recoveries:?}"
    );
    assert_eq!(h.read_page("items").total, 0);
    scroll_to_start(&mut h);
    assert_eq!(apply_label(&mut h), Some("Apply pending (1)".to_string()));
}

#[gpui_kit::test]
fn flags_a_legacy_postgresql_target_migration_and_refuses_to_apply_it(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    open(&mut h);
    edit_config(&mut h, |c| {
        c["migrations"] = json!([
            { "id": "m-pg", "name": "Old PG change", "order": 1, "targetStore": "postgres", "up": "SELECT 1" }
        ]);
    });
    assert_eq!(
        h.label("migration-issue:0"),
        "Old PG change: PostgreSQL migrations are not supported in this version; manage external database schema yourself"
    );
    assert_eq!(h.label("migration-target:Old PG change"), "PostgreSQL (not supported)");
    assert_eq!(status_of(&mut h, "Old PG change"), "Not supported");
    assert_eq!(apply_label(&mut h), Some("Apply pending (0)".to_string()));
    assert_inert(&mut h, "apply-migrations");
    assert!(failure_of(&h, "apply_migrations").contains("PostgreSQL migrations are not"));

    tap(&mut h, "edit-migration:Old PG change");
    tap(&mut h, "migration-target-sqlite");
    tap(&mut h, "migration-save");
    tap(&mut h, "apply-migrations");
    assert!(h.label("run-status").starts_with("All migrations succeeded."));
}

#[gpui_kit::test]
fn disables_migrations_for_a_document_whose_datasource_is_postgresql(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    open(&mut h);
    edit_config(&mut h, |c| {
        c["datasource"] = json!({ "kind": "postgres", "host": "127.0.0.1", "port": 1, "database": "x", "user": "x" });
        c["migrations"] = json!([{ "id": "m1", "name": "Create things", "order": 1, "up": "CREATE TABLE things (x)" }]);
    });
    assert!(
        h.label("migrations-disabled")
            .starts_with("Migrations apply to the embedded SQLite store only and are disabled for this document")
    );
    assert_eq!(status_of(&mut h, "Create things"), "Disabled");
    for button in ["dry-run", "apply-migrations", "rollback-migration"] {
        assert_inert(&mut h, button);
    }
    assert_eq!(apply_label(&mut h), Some("Apply pending (0)".to_string()));
    for command in ["apply_migrations", "rollback_migration", "dry_run_migrations"] {
        let failure = failure_of(&h, command);
        assert!(
            failure.starts_with("VALIDATION_ERROR")
                && failure.contains("Migrations apply to the embedded SQLite store only"),
            "{command}: {failure}"
        );
    }
}

#[gpui_kit::test]
fn saving_closes_its_editor_and_a_reopened_editor_stays_open(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    open(&mut h);
    tap(&mut h, "new-migration");
    fill(&mut h, "migration-name", "Quick");
    fill(&mut h, "migration-up", "CREATE TABLE quick (id INTEGER);");
    tap(&mut h, "migration-save");
    // The save closes the editor it came from.
    assert!(!h.exists("migration-editor"));
    tap(&mut h, "edit-migration:Quick");
    assert_eq!(h.label("migration-editor"), "Edit Quick");
    h.settle();
    let config = crate::read_document_config(h.label.clone()).unwrap();
    assert_eq!(config.migrations.len(), 1);
    assert_eq!(h.label("migration-editor"), "Edit Quick");
}

#[gpui_kit::test]
fn previews_the_down_sql_of_a_reversible_migration(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    open(&mut h);
    author(&mut h, "Add audit", "CREATE TABLE audit (id INTEGER);", Some("DROP TABLE audit;"));
    tap(&mut h, "edit-migration:Add audit");
    tap(&mut h, "migration-preview-down");
    assert_eq!(h.label("sql-preview"), "SQL preview (down)");
    assert_eq!(h.label("preview-statements"), "DROP TABLE audit;");
}

fn bind_label_control(c: &mut Value, controls: Vec<Value>) {
    c["design"]["forms"][0]["source"] = json!({ "kind": "table", "table": "items" });
    c["design"]["forms"][0]["controls"] = Value::Array(controls);
}

fn text_control(label: &str, column: &str) -> Value {
    json!({ "id": uuid::Uuid::new_v4().to_string(), "kind": "text", "label": label, "binding": { "column": column } })
}

#[gpui_kit::test]
fn shows_health_results_and_times_in_the_log_and_problems_after_a_migration_drops_a_bound_column(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    h.create_table("items", &[("id", "INTEGER", 1), ("label", "TEXT", 0)]);
    open(&mut h);
    edit_config(&mut h, |c| {
        bind_label_control(c, vec![text_control("Item label", "label")]);
        c["migrations"] = json!([
            { "id": "m-drop", "name": "Drop label", "order": 1, "up": "ALTER TABLE items DROP COLUMN label;" }
        ]);
    });
    tap(&mut h, "apply-migrations");
    scroll_to_end(&mut h);
    let problem = h.label("after-problem:0");
    assert!(problem.contains("\"Item label\" is bound to items.label"), "{problem}");
    let health = h.label("history:0:health");
    assert!(health.starts_with("Health passed: ") && health.contains("integrity_check: ok"), "{health}");
}

#[gpui_kit::test]
fn lists_only_problems_the_migration_introduced_and_hides_them_after_a_failed_run(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    h.create_table("items", &[("id", "INTEGER", 1), ("label", "TEXT", 0)]);
    open(&mut h);
    edit_config(&mut h, |c| {
        bind_label_control(
            c,
            vec![text_control("Item label", "label"), text_control("Old ghost", "ghost")],
        );
        c["migrations"] = json!([
            { "id": "m-drop", "name": "Drop label", "order": 1, "up": "ALTER TABLE items DROP COLUMN label;" }
        ]);
    });
    tap(&mut h, "apply-migrations");
    scroll_to_end(&mut h);
    assert!(h.label("after-problem:0").contains("\"Item label\" is bound to items.label"));
    // The ghost control was already broken, so it is not listed.
    assert!(!h.exists("after-problem:1"));

    edit_config(&mut h, |c| {
        c["migrations"].as_array_mut().unwrap().push(json!(
            { "id": "m-broken", "name": "Broken", "order": 2, "up": "INSERT INTO nowhere VALUES (1);" }
        ));
    });
    assert!(!h.exists("after-problems"));
    tap(&mut h, "apply-migrations");
    assert!(h.label("run-status").starts_with("A migration failed; later ones did not run."));
    assert!(!h.exists("after-problems"));
}

#[gpui_kit::test]
fn dry_run_refuses_what_apply_refuses_such_as_an_edited_applied_migration(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    open(&mut h);
    edit_config(&mut h, |c| {
        c["migrations"] = json!([{ "id": "m-a", "name": "Make a", "order": 1, "up": "CREATE TABLE a (x);" }]);
    });
    tap(&mut h, "apply-migrations");
    assert!(h.label("run-status").starts_with("All migrations succeeded."));
    edit_config(&mut h, |c| {
        c["migrations"] = json!([
            { "id": "m-a", "name": "Make a", "order": 1, "up": "CREATE TABLE a (y);" },
            { "id": "m-b", "name": "Make b", "order": 2, "up": "CREATE TABLE b (x);" },
        ]);
    });
    assert_eq!(status_of(&mut h, "Make a"), "Changed after apply");
    tap(&mut h, "dry-run");
    assert!(h.label("migrations-error").contains("Make a changed after it was applied"));
    assert!(!h.exists("dry-run-status"));
    // Apply stays disabled: clicking it runs nothing.
    tap(&mut h, "apply-migrations");
    assert!(!h.exists("run-status"));
}
