//! Port of the "runtime navigation with roles" tests of tests/unit/rbac.test.ts, plus page
//! titles.

use super::{PageKind, RuntimePage, can_open, page_title, solo_page, start_page, visible_navigation};
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

#[test]
fn a_page_with_only_a_form_or_report_opens_it_full_page() {
    let mut config = DocumentConfig::default();
    config.design.forms.push(crate::design::Form { id: "f".into(), modes: crate::design::default_modes(), ..Default::default() });
    config.pages = serde_json::from_value(json!([
        {"id": "p1", "name": "Orders", "components": [{"id": "c", "kind": "form", "formId": "f", "mode": "detail"}]},
        {"id": "p2", "name": "Sales", "components": [{"id": "c", "kind": "report", "reportId": "r"}]},
        {"id": "p3", "name": "Mixed", "components": [
            {"id": "a", "kind": "form", "formId": "f"}, {"id": "b", "kind": "kpi"}
        ]}
    ]))
    .unwrap();
    let form = solo_page(&config, &RuntimePage::new(PageKind::Page, "p1"));
    assert_eq!((form.kind, form.id.as_str(), form.mode.as_deref()), (PageKind::Form, "f", Some("detail")));
    let listed = RuntimePage { mode: Some("list".into()), nav_id: Some("n".into()), ..RuntimePage::new(PageKind::Page, "p1") };
    let form = solo_page(&config, &listed);
    assert_eq!((form.mode.as_deref(), form.nav_id.as_deref()), (Some("list"), Some("n")));
    let report = solo_page(&config, &RuntimePage::new(PageKind::Page, "p2"));
    assert_eq!((report.kind, report.id.as_str(), report.mode), (PageKind::Report, "r", None));
    let mixed = RuntimePage::new(PageKind::Page, "p3");
    assert_eq!(solo_page(&config, &mixed), mixed);

    // Reading the form opens its page; a page with more on it needs its own grant.
    config.roles = serde_json::from_value(json!([{"id": "reader", "name": "Reader", "permissions": {
        "objects": [{"kind": "form", "id": "f", "read": true}]
    }}]))
    .unwrap();
    assert!(can_open(&config, Some("reader"), &RuntimePage::new(PageKind::Page, "p1")));
    assert!(!can_open(&config, Some("reader"), &mixed));
}
