//! Importing an Access template: inventory, report, assets preview and the migration tab
//! (`access-*`). Port of scripts/screenshot/specs/access-import.spec.tsx.
//!
//! Not ported: `access-03-runtime` and `access-08-open-form` (the Runtime page and the form
//! designer belong to other areas) and `access-05-action-query` (Query mode); the action
//! query itself is covered by the access tests.

use crate::ui::access::migration::{MigrationFilter, StatusFilter, filter_items, item_key, read_migration};
use crate::ui::access::tests::template;
use crate::ui::screenshot::context;
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use gpui_kit::{ScrollDelta, SharedString, point, px, test::TestWindowExt as _};

const TALL: (f32, f32) = (1280.0, 1700.0);

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn access_import_template() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, TALL);
    h.click("import-access");
    assert_eq!(h.label("access-wizard"), "Import Access database");
    h.dialogs.queue_open(Some(template()));
    h.click("access-choose");
    assert!(h.exists("access-table:Customers") && h.exists("access-table:Orders"));
    assert_eq!(h.find("access-include-data").checked(), Some(true));
    h.capture(
        "access-01-inventory",
        &[
            "The Import Access database dialog names the file as an Access template.",
            "It lists the Customers and Orders tables with field and row counts, and counts of queries, forms, reports, macros, and modules.",
            "The Import the data option is checked.",
        ],
    );

    h.click("access-import");
    assert!(h.exists("access-report"));
    h.click("attention-toggle");
    assert!(h.label("attention-toggle").starts_with("What did not convert fully"));
    assert!(h.exists("report-row:table"));
    h.capture(
        "access-02-report",
        &[
            "The report says how many tables and rows the new document has, and that Settings › Access migration keeps it.",
            "A table counts converted, partly converted, and not converted objects per kind.",
            "The expanded notes explain each loss, such as the VBA module kept as an asset.",
        ],
    );

    // The report is taller than the dialog: scroll to its buttons.
    h.update(|w, cx| w.scroll("overlay", ScrollDelta::Pixels(point(px(0.), px(-5000.))), cx));
    h.settle();
    h.click("access-open");
    assert!(h.has_shell());
    h.switch_mode(ModeId::App);
    h.click("tab-assets");
    h.click("view-asset:Access VBA.txt");
    assert_eq!(h.label("asset-preview"), "Preview of Access VBA.txt");
    h.capture(
        "access-04-vba",
        &[
            "Settings › Assets lists the Access VBA.txt asset with a View button.",
            "The preview shows the VBA one section per module and form, with section links above.",
            "The code keeps its line breaks and indentation in a monospace block.",
        ],
    );

    h.click("tab-access");
    assert!(h.label("access-intro").starts_with("Imported from Order Desk.accdt, an Access template"));
    h.update(|w, cx| w.scroll("settings-panel", ScrollDelta::Pixels(point(px(0.), px(5000.))), cx));
    h.capture(
        "access-06-migration",
        &[
            "Settings has an Access migration tab naming the source file and how many items still need review.",
            "A summary table counts converted, partly converted and not converted objects per kind.",
            "The item list shows only what needs attention, worst first, with notes, Open links and Reviewed checkboxes.",
        ],
    );

    let settings = crate::read_document_config(h.label.clone()).unwrap().settings;
    let migration = read_migration(&settings).unwrap();
    h.click("access-status:all");
    h.click("access-kind:form");
    let forms = filter_items(
        &migration,
        &MigrationFilter { kind: "form".into(), ..MigrationFilter::new(StatusFilter::All) },
    );
    assert!(h.exists(SharedString::from(format!("item:{}", item_key(&forms[0])))));
    let before = h.label("access-intro");
    h.click(SharedString::from(format!("reviewed:{}", item_key(&forms[0]))));
    assert_ne!(h.label("access-intro"), before);
    h.capture(
        "access-07-migration-filtered",
        &[
            "Show is set to Everything and Kind to Forms, so only forms are listed, converted ones included.",
            "The form marked reviewed has its checkbox ticked and the review count above went down by one.",
            "Each converted form has an Open form link.",
        ],
    );
}
