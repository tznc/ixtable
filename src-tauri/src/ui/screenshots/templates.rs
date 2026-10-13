//! Starting from the golden templates (`templates-*`). Port of
//! scripts/screenshot/specs/templates.spec.tsx.
//!
//! Not ported: `templates-01..04`, which show the template's Runtime pages, dashboards and
//! the relationship diagram (other areas' UI). This spec covers the start-screen part: the
//! template list and creating a document from a template.

use crate::ui::screenshot::context;
use crate::ui::testing::Harness;

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn templates_start_screen() {
    let mut cx = context();
    let mut h = Harness::start(&mut cx);
    for id in ["crm", "inventory", "work-orders"] {
        assert!(h.exists(format!("template:{id}")), "{id}");
    }
    h.capture(
        "templates-00-start-templates",
        &["The start screen lists the CRM, Inventory, and Work orders templates under 'Start from a template'."],
    );
    h.click("template:crm");
    assert!(h.has_shell());
    assert_eq!(h.config().name, "CRM");
    assert!(h.state().path.is_none());
}
