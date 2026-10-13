//! Port of tests/unit/design-legacy-ids.test.ts.

use super::{has_legacy_ids, upgrade_legacy_ids};
use serde_json::{Value, json};

fn legacy() -> Value {
    json!({
        "version": 3, "name": "Old", "activeMode": "design",
        "design": {
            "version": 3,
            "forms": [
                {"id": "main", "name": "Main form", "detailFormId": "main", "controls": [
                    {"id": "c1", "kind": "relatedList", "label": "Lines", "related": {
                        "table": "t", "foreignKey": "p", "parentColumn": "id",
                        "columns": [], "formId": "main"}}
                ]},
                {"id": "other", "name": "Other", "detailFormId": null},
            ],
            "navigation": [
                {"id": "main", "label": "Main form", "kind": "form", "targetId": "main"},
                {"id": "g", "label": "Group", "kind": "group", "children": [
                    {"id": "n2", "label": "Again", "targetId": "main"},
                    {"id": "n3", "label": "Report", "kind": "report", "targetId": "main"},
                ]},
            ],
            "startPage": "main",
        },
        "actions": [{"id": "a", "name": "Open", "onError": "stop", "steps": [
            {"id": "s1", "kind": "openForm", "formId": "main"},
            {"id": "s2", "kind": "condition",
             "then": [{"id": "s3", "kind": "navigate", "target": {"kind": "form", "id": "main"}}],
             "else": [{"id": "s4", "kind": "navigate", "target": {"kind": "report", "id": "main"}}]},
        ]}],
        "dashboards": [
            {"id": "d", "name": "D", "components": [
                {"id": "k", "kind": "form", "title": "F", "formId": "main"}]}
        ],
        "roles": [{"id": "r", "name": "Clerk", "permissions": {
            "navigation": ["main", "g"],
            "objects": [
                {"kind": "form", "id": "main", "read": true},
                {"kind": "report", "id": "main", "read": true},
            ],
            "actions": [],
        }}],
    })
}

#[test]
fn rewrites_the_form_and_navigation_ids_and_every_reference() {
    let mut ids = vec!["N".to_string(), "F".to_string()];
    let before = legacy();
    let value = upgrade_legacy_ids(&before, || ids.pop().unwrap_or_default()).unwrap();
    let expected = [
        ("/design/forms/0/id", json!("F")),
        ("/design/forms/0/detailFormId", json!("F")),
        ("/design/forms/0/controls/0/related/formId", json!("F")),
        ("/design/forms/1/id", json!("other")),
        ("/design/navigation/0/id", json!("N")),
        ("/design/navigation/0/targetId", json!("F")),
        ("/design/navigation/1/children/0/targetId", json!("F")),
        ("/design/navigation/1/children/1/targetId", json!("main")),
        ("/design/startPage", json!("N")),
        ("/actions/0/steps/0/formId", json!("F")),
        ("/actions/0/steps/1/then/0/target/id", json!("F")),
        ("/actions/0/steps/1/else/0/target/id", json!("main")),
        ("/dashboards/0/components/0/formId", json!("F")),
        ("/roles/0/permissions/navigation", json!(["N", "g"])),
        ("/roles/0/permissions/objects/0/id", json!("F")),
        ("/roles/0/permissions/objects/1/id", json!("main")),
    ];
    for (path, want) in expected {
        assert_eq!(value.pointer(path), Some(&want), "{path}");
    }
    assert_eq!(before.pointer("/design/forms/0/id"), Some(&json!("main")));
}

#[test]
fn uses_uuidv7_ids_and_leaves_current_configs_untouched() {
    let upgraded = upgrade_legacy_ids(&legacy(), || uuid::Uuid::now_v7().to_string()).unwrap();
    let form = upgraded.pointer("/design/forms/0/id").and_then(Value::as_str).unwrap();
    let parsed = uuid::Uuid::parse_str(form).unwrap();
    assert_eq!(parsed.get_version_num(), 7);
    assert_ne!(upgraded.pointer("/design/startPage").and_then(Value::as_str), Some(form));
    assert!(!has_legacy_ids(&upgraded));
    assert!(upgrade_legacy_ids(&upgraded, String::new).is_none());
    assert!(has_legacy_ids(&legacy()));
}
