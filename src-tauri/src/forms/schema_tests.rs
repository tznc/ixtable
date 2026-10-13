//! Port of the schema upgrade tests of tests/unit/forms-schema.test.ts. The form operation
//! tests of the same file are in `operations_tests.rs`.

use super::{new_control, new_form, next_placement, upgrade_design};
use crate::design::{
    ControlKind, DESIGN_SCHEMA_VERSION, FormMode, NavKind, Placement, SourceKind,
};
use serde_json::json;

fn v2() -> serde_json::Value {
    json!({
        "version": 2,
        "forms": [{
            "id": "f",
            "name": "Customers",
            "table": "customers",
            "controls": [{
                "id": "c",
                "kind": "checkbox",
                "label": "Active",
                "binding": {"table": "customers", "column": "active"},
                "validation": {"required": true},
                "placement": {"column": 1, "row": 1, "columnSpan": 6, "rowSpan": 1}
            }]
        }],
        "navigation": [{"id": "n", "label": "Customers", "formId": "f"}]
    })
}

#[test]
fn upgrades_a_v2_design_to_the_current_shape() {
    let design = upgrade_design(v2()).unwrap();
    assert_eq!(design.version, DESIGN_SCHEMA_VERSION);
    let form = &design.forms[0];
    let source = form.source.as_ref().unwrap();
    assert_eq!(source.kind, SourceKind::Table);
    assert_eq!(source.table.as_deref(), Some("customers"));
    assert_eq!(
        form.modes,
        [FormMode::List, FormMode::Detail, FormMode::Create, FormMode::Edit]
    );
    assert!(form.rules.is_empty());
    assert_eq!(form.layout.columns.len(), 12);
    assert_eq!(form.controls[0].kind, ControlKind::Boolean);
    assert_eq!(form.controls[0].binding.as_ref().unwrap().column, "active");
    assert_eq!(
        form.controls[0].placement,
        Placement {
            column: 1,
            row: 1,
            column_span: 6,
            row_span: 1,
            region: None
        }
    );
    let nav = &design.navigation[0];
    assert_eq!(nav.kind, NavKind::Form);
    assert_eq!(nav.target_id.as_deref(), Some("f"));
    assert!(nav.children.is_empty());
    assert_eq!(design.start_page.as_deref(), Some("n"));
}

#[test]
fn keeps_a_current_design_as_is_and_fills_missing_defaults() {
    let design = upgrade_design(json!({
        "version": 3, "forms": [{"id": "a", "name": "A"}], "startPage": null
    }))
    .unwrap();
    assert_eq!(design.start_page, None);
    let form = &design.forms[0];
    assert!(form.source.is_none());
    assert_eq!(form.page_size, 25);
    assert!(form.list_columns.is_empty());
    // The TS upgrade also accepted `undefined`; the Rust loader needs a version.
    assert!(upgrade_design(json!({})).is_err());
}

#[test]
fn places_new_controls_in_the_first_free_slot_of_their_container() {
    let mut form = new_form(
        "Orders",
        Some(crate::design::FormSource {
            kind: SourceKind::Table,
            table: Some("orders".into()),
            ..Default::default()
        }),
    );
    let tabs = new_control(ControlKind::Tabs, &form, None);
    form.controls.push(tabs.clone());
    let tab = tabs.tabs[0].id.clone();
    let parent = crate::design::ControlParent {
        id: tabs.id.clone(),
        tab: Some(tab),
    };
    let inner = new_control(ControlKind::Text, &form, Some(&parent));
    assert_eq!(
        (inner.placement.column, inner.placement.row, inner.placement.column_span),
        (1, 1, 6)
    );
    form.controls.push(inner);
    let second = new_control(ControlKind::Number, &form, Some(&parent));
    assert_eq!((second.placement.column, second.placement.row), (7, 1));
    form.controls.push(second);
    let section = new_control(ControlKind::Section, &form, None);
    form.controls.push(section);
    let next = next_placement(&form, None, None, None);
    assert_eq!((next.column, next.row, next.column_span), (1, 3, 12));
}
