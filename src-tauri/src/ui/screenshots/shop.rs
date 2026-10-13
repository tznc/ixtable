//! The Data mode captures of the old app-shell spec: the records grid, the table
//! designer, the relationship canvas and inline record entry (`app-qa-03-*`,
//! `app-qa-04-*`).

use crate::ui::data::testkit::seed_shop;
use crate::ui::screenshot::context;
use crate::ui::testing::Harness;

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn shop_data() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, (1440.0, 900.0));
    h.click("new-document");
    seed_shop(&mut h);
    assert_eq!(h.label("relationship-title"), "4 tables \u{b7} 3 relationships");

    h.click("table:Customers");
    assert_eq!(h.label("cell:0:0"), "CUST-001");
    h.capture(
        "app-qa-03-data",
        &[
            "The selected table is highlighted in the object browser and relationship canvas.",
            "The records grid shows rows loaded through backend commands.",
            "Document mode and save status remain visible.",
        ],
    );

    h.click("design-table");
    assert_eq!(h.label("design-heading"), "Design Customers");
    h.capture(
        "app-qa-03-table-design",
        &[
            "Selected table metadata is editable in the table designer.",
            "In-place schema changes and table-rebuild migrations are visually distinct.",
            "Column constraints and relationship implications remain visible.",
        ],
    );

    h.click("close-designer");
    assert!(!h.exists("design-heading"));
    // With no table selected the diagram fills the workbench, so every node shows.
    let shell = h.shell();
    h.update(|_, cx| shell.update(cx, |s, cx| s.select(None, cx)));
    h.settle();
    assert!(h.exists("node:Products"));
    h.capture(
        "app-qa-03-data-canvas",
        &[
            "All four table nodes and their fields are fully visible.",
            "Three labelled relationships connect the related keys.",
        ],
    );

    h.click("table:Orders");
    h.type_into("New Order ID", "10483");
    h.press("tab");
    h.input("CUST-001");
    assert_eq!(h.find("New Customer ID").value(), Some("CUST-001"));
    h.capture(
        "app-qa-04-inline-record",
        &[
            "A new record is entered directly in the final grid row.",
            "The active draft row remains aligned with the table columns.",
        ],
    );
    h.press("enter");
    assert_eq!(h.read_page("Orders").total, 3);
}
