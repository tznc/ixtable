//! Port of tests/unit/trigger-warnings.test.ts.

use super::{TriggerGap, user_trigger_gaps};
use crate::archive::DocumentConfig;
use crate::forms::rbac::{assign_runtime_role, serial};
use serde_json::{Value, json};

fn config() -> Value {
    json!({
        "roles": [{"id": "clerk", "name": "Clerk", "permissions": {
            "navigation": [], "actions": ["stock"],
            "objects": [{"kind": "table", "id": "inventory", "read": true, "create": false,
                         "update": true, "delete": false}]
        }}],
        "actions": [
            {"id": "stock", "name": "Stock", "onError": "stop", "steps": [
                {"id": "c", "kind": "condition", "when": "true",
                 "then": [{"id": "u", "kind": "updateRecord", "table": "inventory", "match": "current"}],
                 "else": [{"id": "r", "kind": "runAction", "actionId": "audit"}]}
            ]},
            {"id": "audit", "name": "Audit", "onError": "stop", "steps": [
                {"id": "l", "kind": "createRecord", "table": "audit_log", "values": {}}
            ]},
        ],
        "triggers": [
            {"id": "t-user", "name": "User stock", "table": "orders", "event": "created",
             "actionId": "stock", "mode": "sync", "enabled": true, "maxAttempts": 3,
             "backoffMs": 1, "runAs": "user"},
            {"id": "t-app", "name": "App stock", "table": "orders", "event": "created",
             "actionId": "stock", "mode": "sync", "enabled": true, "maxAttempts": 3,
             "backoffMs": 1},
        ],
    })
}

fn parse(mut value: Value) -> DocumentConfig {
    let base = serde_json::to_value(DocumentConfig::default()).unwrap();
    let mut merged = base.as_object().unwrap().clone();
    merged.extend(value.as_object_mut().unwrap().clone());
    serde_json::from_value(Value::Object(merged)).unwrap()
}

#[test]
fn lists_the_grants_a_role_lacks_for_user_mode_triggers_only() {
    let _s = serial();
    assign_runtime_role(None);
    assert_eq!(
        user_trigger_gaps(&parse(config()), "clerk"),
        [TriggerGap {
            trigger_id: "t-user".into(),
            name: "User stock".into(),
            missing: vec![
                "execute action \"Audit\"".into(),
                "create table \"audit_log\"".into()
            ],
        }]
    );
}

#[test]
fn follows_updates_of_custom_action_entities_to_the_action_they_run() {
    let _s = serial();
    assign_runtime_role(None);
    let mut value = config();
    value["actions"].as_array_mut().unwrap().push(json!({
        "id": "guard", "name": "Guard", "onError": "stop",
        "steps": [{"id": "g", "kind": "createRecord", "table": "stock_audit", "values": {}}]
    }));
    value["entities"] = json!([
        {"id": "e", "table": "inventory", "concurrency": "customAction", "actionId": "guard"}
    ]);
    let mut missing = user_trigger_gaps(&parse(value), "clerk")[0].missing.clone();
    missing.sort();
    assert_eq!(
        missing,
        [
            "create table \"audit_log\"",
            "create table \"stock_audit\"",
            "execute action \"Audit\"",
            "execute action \"Guard\"",
        ]
    );
}
