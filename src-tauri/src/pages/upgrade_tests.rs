use super::*;
use crate::archive::DocumentConfig;
use serde_json::json;

fn v7() -> DocumentConfig {
    serde_json::from_value(json!({
        "version": 7, "name": "Shop", "activeMode": "design",
        "design": {
            "version": 3,
            "forms": [
                {"id": "f1", "name": "Customers", "modes": ["detail", "list"]},
                {"id": "f2", "name": "Orders"}
            ],
            "navigation": [
                {"id": "n1", "label": "Customers", "kind": "form", "targetId": "f1", "mode": "list"},
                {"id": "g", "label": "More", "kind": "group", "children": [
                    {"id": "n2", "label": "Sales", "kind": "report", "targetId": "r1"},
                    {"id": "n3", "label": "Ops", "kind": "dashboard", "targetId": "d1"}
                ]}
            ],
            "startPage": "n1"
        },
        "reports": [{"id": "r1", "name": "Sales"}],
        "dashboards": [{"id": "d1", "name": "Ops", "components": [
            {"id": "c1", "kind": "form", "formId": "f2", "placement": {"column": 1, "row": 1}}
        ]}],
        "roles": [{"id": "clerk", "name": "Clerk", "permissions": {"objects": [
            {"kind": "form", "id": "f1", "read": true, "update": true},
            {"kind": "dashboard", "id": "d1", "read": true}
        ]}}]
    }))
    .expect("v7 config")
}

#[test]
fn dashboards_become_pages_with_their_ids() {
    let config = v7().upgrade().expect("upgrade");
    assert_eq!(config.version, crate::archive::CONFIG_VERSION);
    let ops = config.pages.iter().find(|p| p.id == "d1").expect("dashboard page");
    assert_eq!(ops.name, "Ops");
    assert_eq!(ops.components[0].form_id.as_deref(), Some("f2"));
}

#[test]
fn unplaced_forms_and_reports_get_a_full_width_page() {
    let config = v7().upgrade().expect("upgrade");
    // f2 is already on the Ops page; f1 and r1 are wrapped.
    assert_eq!(config.pages.len(), 3);
    let customers = config.pages.iter().find(|p| p.name == "Customers").expect("form page");
    let block = &customers.components[0];
    assert_eq!(block.kind, ComponentKind::Form);
    assert_eq!(block.form_id.as_deref(), Some("f1"));
    assert_eq!(block.mode, Some(FormMode::Detail));
    assert_eq!(block.placement.column_span, 12);
    let sales = config.pages.iter().find(|p| p.name == "Sales").expect("report page");
    assert_eq!(sales.components[0].kind, ComponentKind::Report);
    assert_eq!(sales.components[0].report_id.as_deref(), Some("r1"));
    assert!(crate::pages::validate(&config).iter().all(|i| i.severity != crate::archive::Severity::Error));
}

#[test]
fn navigation_points_at_the_pages_and_keeps_the_form_mode() {
    let config = v7().upgrade().expect("upgrade");
    let page_of = |name: &str| config.pages.iter().find(|p| p.name == name).unwrap().id.clone();
    let nav = &config.design.navigation;
    assert_eq!(nav[0].kind, NavKind::Page);
    assert_eq!(nav[0].target_id, Some(page_of("Customers")));
    assert_eq!(nav[0].mode, Some(FormMode::List));
    let children = &nav[1].children;
    assert_eq!((children[0].kind, children[0].target_id.clone()), (NavKind::Page, Some(page_of("Sales"))));
    assert_eq!((children[1].kind, children[1].target_id.as_deref()), (NavKind::Page, Some("d1")));
    assert_eq!(config.design.start_page.as_deref(), Some("n1"));
}

#[test]
fn roles_read_the_pages_of_the_forms_they_read() {
    let config = v7().upgrade().expect("upgrade");
    let customers = config.pages.iter().find(|p| p.name == "Customers").unwrap();
    let objects = &config.roles[0].permissions.objects;
    assert!(objects.iter().any(|o| o.kind == "page" && o.id == "d1" && o.read));
    let grant = objects.iter().find(|o| o.kind == "page" && o.id == customers.id).expect("page grant");
    assert!(grant.read && !grant.update);
    assert!(objects.iter().any(|o| o.kind == "form" && o.id == "f1" && o.update));
    assert!(crate::roles::validate(&config).is_empty(), "{:?}", crate::roles::validate(&config));
}

#[test]
fn wrapper_ids_are_stable_across_loads() {
    let a = v7().upgrade().expect("upgrade");
    let b = v7().upgrade().expect("upgrade");
    let ids = |c: &DocumentConfig| c.pages.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&a), ids(&b));
    assert!(a.pages.iter().all(|p| p.id == "d1" || uuid::Uuid::parse_str(&p.id).is_ok_and(|u| u.get_version_num() == 7)));
}

#[test]
fn a_current_config_is_not_wrapped_again() {
    let mut config = v7().upgrade().expect("upgrade");
    config.design.forms.push(crate::design::Form { id: "f3".into(), name: "Loose".into(), ..Default::default() });
    let config = config.upgrade().expect("second upgrade");
    assert!(!config.pages.iter().any(|p| p.name == "Loose"));
}

#[test]
fn round_trips_under_the_pages_key() {
    let config = v7().upgrade().expect("upgrade");
    let value = serde_json::to_value(&config).expect("json");
    assert!(value.get("dashboards").is_none());
    assert_eq!(value["pages"].as_array().map(Vec::len), Some(3));
    let back: DocumentConfig = serde_json::from_value(value).expect("back");
    assert_eq!(back.pages, config.pages);
}
