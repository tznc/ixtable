//! Start screen, shell, mode switching and the settings YAML/Problems tabs (`app-qa-*`).

use crate::ui::screenshot::context;
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn app_shell() {
    let mut cx = context();
    let mut h = Harness::start(&mut cx);
    assert!(h.exists("new-document"));
    h.capture(
        "app-qa-01-start",
        &[
            "The start screen shows New document and Open document actions",
            "The ix mark sits on a white tile, never a dark one",
        ],
    );

    h.click("new-document");
    assert!(h.has_shell());
    h.create_table("customers", &[("id", "INTEGER", 1), ("name", "TEXT", 0)]);
    assert!(h.exists("table:customers"));
    h.capture(
        "app-qa-02-shell",
        &[
            "The sidebar lists the modes, with Data selected",
            "A gear icon button sits beside Save and Save as at the top of the sidebar",
            "The object browser lists the customers table",
        ],
    );

    h.switch_mode(ModeId::App);
    assert_eq!(h.mode(), ModeId::App);
    h.click("tab-yaml");
    assert!(h.exists("apply-yaml"));
    h.capture(
        "app-qa-05-settings-yaml",
        &["The Settings YAML tab shows the document config in a code editor"],
    );

    h.click("tab-problems");
    assert!(h.exists("problems-summary"));
    h.capture(
        "app-qa-08-problems",
        &["The Problems tab reports its error and warning counts"],
    );
}
