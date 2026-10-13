//! Settings tabs: datasource, entities, migrations, YAML and Problems (`settings-01..05`).
//! Port of scripts/screenshot/specs/settings.spec.tsx.

use super::runtime::seed_sales;
use crate::ui::data::testkit::choose;
use crate::ui::screenshot::context;
use crate::ui::shell::{ModeId, settings::AppSettings, yaml::YamlTab};
use crate::ui::testing::{Harness, UiContext as _};
use gpui_kit::{Entity, HeadlessAppContext};

const TALL: (f32, f32) = (1280.0, 1500.0);

fn yaml_tab(h: &mut Harness<'_, HeadlessAppContext>) -> Entity<YamlTab> {
    h.switch_mode(ModeId::App);
    h.click("tab-yaml");
    let view = h.read_shell(|s, _| s.mode_view()).expect("settings view");
    let settings: Entity<AppSettings> = view.downcast().expect("AppSettings");
    let tab = h.cx.read(|cx| settings.read(cx).tab_view()).expect("yaml tab");
    tab.downcast().expect("YamlTab")
}

fn set_yaml(h: &mut Harness<'_, HeadlessAppContext>, tab: &Entity<YamlTab>, text: &str) {
    let editor = h.cx.read(|cx| tab.read(cx).editor.clone());
    h.update(|window, cx| editor.update(cx, |e, cx| e.set_value(text.to_string(), window, cx)));
    h.settle();
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn settings_migrations_yaml_and_problems() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, TALL);
    h.click("new-document");
    assert!(h.has_shell());
    seed_sales(&mut h);

    h.switch_mode(ModeId::App);
    h.click("tab-datasource");
    assert!(h.exists("tab-title"));
    choose(&mut h, "Record store", "PostgreSQL");
    choose(&mut h, "TLS (sslmode)", "disable (no TLS)");
    assert!(h.label("plaintext-warning").contains("Severe security warning"));
    assert!(h.exists("accept-plaintext"));
    assert!(h.label("shared-credentials-note").contains("Shared credentials reduce revocation"));
    h.capture(
        "settings-01-datasource",
        &[
            "PostgreSQL connection fields (host, port, database, user, password, TLS) are labelled.",
            "Disabling TLS shows a severe warning with an explicit acceptance checkbox; Save is disabled.",
            "A note warns that shared credentials reduce revocation.",
        ],
    );
    choose(&mut h, "Record store", "Embedded SQLite (in the .ixt file)");

    h.click("tab-entities");
    choose(&mut h, "Concurrency policy for orders", "Last write wins");
    assert!(h.exists("entity:orders"));
    h.capture(
        "settings-02-entities",
        &["Each table has a concurrency policy selector; orders uses last-write-wins."],
    );

    h.click("tab-migrations");
    assert_eq!(h.label("migrations"), "Migrations");
    h.click("new-migration");
    h.fill("migration-name", "Add order tags");
    h.fill("migration-up", "CREATE TABLE order_tags (order_id INTEGER, tag TEXT NOT NULL);");
    h.click("migration-reversible");
    h.fill("migration-down", "DROP TABLE order_tags;");
    h.click("migration-save");
    assert!(h.exists("migration-name:Add order tags"));
    h.click("dry-run");
    assert_eq!(h.label("dry-run-status"), "Dry run succeeded; nothing was changed.");
    h.capture(
        "settings-03-migrations",
        &[
            "The migration list shows Add order tags as pending and reversible.",
            "The dry-run result confirms nothing was changed; Apply pending (1) is offered.",
        ],
    );

    let tab = yaml_tab(&mut h);
    assert!(h.cx.read(|cx| tab.read(cx).editor.read(cx).value().to_string()).contains("name: Untitled"));
    h.capture(
        "settings-04-yaml",
        &["The YAML tab shows the document configuration in a monospace editor with Apply YAML."],
    );
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
    h.capture(
        "settings-05-problems",
        &[
            "The Problems tab summarises errors and warnings and lists each with its object.",
            "The duplicate report id error and the missing report name warning are distinguishable.",
        ],
    );
}
