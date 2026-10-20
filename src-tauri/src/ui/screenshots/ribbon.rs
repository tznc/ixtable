//! The ribbon above every Studio page (`ribbon-*`): the Home, Create and Database tools tabs.

use crate::ui::screenshot::context;
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn ribbon() {
    let mut cx = context();
    let mut h = Harness::start(&mut cx);
    h.click("new-document");
    h.create_table("customers", &[("id", "INTEGER", 1), ("name", "TEXT", 0)]);
    h.click("table:customers");
    assert!(h.exists("ribbon-group-run"));
    assert!(h.exists("ribbon-find"));
    h.capture(
        "ribbon-01-home",
        &[
            "A ribbon with Home, Create, External data and Database tools tabs sits above the page, right of the sidebar",
            "Home shows the Undo, Records (Refresh, Find) and Run groups, with the primary Run App split button in Run",
            "The sidebar has no Run App button",
        ],
    );

    h.click("tab-create");
    assert!(h.exists("ribbon-new-form"));
    h.capture(
        "ribbon-02-create",
        &["Create shows Tables (Table, Query), Forms (Form, Page), Reports (Report) and Automation (Action, Trigger)"],
    );

    h.switch_mode(ModeId::Design);
    h.click("tab-tools");
    assert!(h.exists("ribbon-problems"));
    h.capture(
        "ribbon-03-tools-on-design",
        &["On the Design page the ribbon keeps its place and shows Database tools: Relationships, Analyze, Security, Advanced"],
    );
}
