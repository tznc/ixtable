//! Port of the "runtime navigation with roles" tests of tests/unit/rbac.test.ts, plus page
//! titles.

use super::{PageKind, RuntimePage, can_open, page_title, start_page, visible_navigation};
use crate::archive::DocumentConfig;
use crate::design::NavigationItem;
use crate::forms::rbac::tests::clerk;
use crate::forms::rbac::{assign_runtime_role, serial, set_previewed_role};
use serde_json::json;

fn config() -> DocumentConfig {
    let mut config = DocumentConfig::default();
    config.roles = vec![clerk()];
    config.design = serde_json::from_value(json!({
        "version": 3,
        "forms": [{"id": "orders-form", "name": "Order entry"}],
        "startPage": "customers",
        "navigation": [
            {"id": "orders", "label": "Orders", "kind": "form", "targetId": "orders-form"},
            {"id": "customers", "label": "Customers", "kind": "form", "targetId": "customers-form"},
            {"id": "sales", "label": "Sales", "kind": "group", "children": [
                {"id": "hidden", "label": "Report", "kind": "report", "targetId": "r1"}
            ]},
        ]
    }))
    .unwrap();
    config
}

fn ids(items: &[NavigationItem]) -> Vec<&str> {
    items.iter().map(|i| i.id.as_str()).collect()
}

fn reset() {
    assign_runtime_role(None);
    set_previewed_role("w", None);
}

#[test]
fn hides_items_without_view_or_target_read_and_empty_groups() {
    let _s = serial();
    reset();
    let c = config();
    assert_eq!(
        ids(&visible_navigation(&c.design.navigation, &c, None)),
        ["orders", "customers", "sales"]
    );
    assert_eq!(
        ids(&visible_navigation(&c.design.navigation, &c, Some("clerk"))),
        ["orders"]
    );
}

#[test]
fn falls_back_to_the_first_visible_page_when_the_start_page_is_hidden() {
    let _s = serial();
    reset();
    let c = config();
    let dev = start_page(&c, None).unwrap();
    assert_eq!((dev.kind, dev.id.as_str()), (PageKind::Form, "customers-form"));
    let clerk = start_page(&c, Some("clerk")).unwrap();
    assert_eq!((clerk.kind, clerk.id.as_str()), (PageKind::Form, "orders-form"));
    assert!(!can_open(&c, Some("clerk"), &RuntimePage::new(PageKind::Report, "r1")));
}

#[test]
fn titles_a_page_by_its_nav_label_else_its_targets_name() {
    let _s = serial();
    reset();
    let c = config();
    let nav = start_page(&c, None).unwrap();
    assert_eq!(page_title(&c, &nav), "Customers");
    assert_eq!(page_title(&c, &RuntimePage::new(PageKind::Form, "orders-form")), "Order entry");
    assert_eq!(page_title(&c, &RuntimePage::new(PageKind::Table, "order_lines")), "Order lines");
    assert_eq!(page_title(&c, &RuntimePage::new(PageKind::Report, "gone")), "previous page");
}
