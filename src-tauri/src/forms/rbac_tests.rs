//! Ports of tests/unit/rbac.test.ts (can, dashboard queries, permission edits),
//! tests/unit/runtime-authz.test.ts and the assigned-role half of tests/unit/cloud-rbac.test.ts.

use super::{
    AssignedRole, Operation, active_role_id, assign_runtime_role, authorizer, can,
    can_read_dashboard_query, dashboard_query_ids, serial, set_object_permission,
    set_previewed_role, toggle_listed,
};
use crate::archive::DocumentConfig;
use crate::dashboards::Dashboard;
use crate::roles::{ObjectPermission, Permissions, Role};
use serde_json::json;

fn object(kind: &str, id: &str, flags: [bool; 4]) -> ObjectPermission {
    ObjectPermission {
        kind: kind.into(),
        id: id.into(),
        read: flags[0],
        create: flags[1],
        update: flags[2],
        delete: flags[3],
    }
}

pub(crate) fn clerk() -> Role {
    Role {
        id: "clerk".into(),
        name: "Clerk".into(),
        permissions: Permissions {
            navigation: vec!["orders".into(), "sales".into()],
            objects: vec![
                object("form", "orders-form", [true, true, true, false]),
                object("table", "orders", [true, false, false, false]),
            ],
            actions: vec!["approve".into()],
        },
    }
}

fn config() -> DocumentConfig {
    DocumentConfig {
        roles: vec![clerk()],
        ..Default::default()
    }
}

fn role(id: &str, actions: &[&str]) -> Role {
    Role {
        id: id.into(),
        name: id.into(),
        permissions: Permissions {
            actions: actions.iter().map(|a| a.to_string()).collect(),
            ..Default::default()
        },
    }
}

fn reset() {
    assign_runtime_role(None);
    set_previewed_role("w", None);
}

#[test]
fn gives_the_developer_full_access() {
    let _s = serial();
    reset();
    let c = config();
    assert!(can(&c, None, "form", "anything", Operation::Delete));
    assert!(can(&c, None, "action", "x", Operation::Execute));
}

#[test]
fn checks_object_flags_navigation_and_actions_for_a_role() {
    let _s = serial();
    reset();
    let c = config();
    let r = Some("clerk");
    assert!(can(&c, r, "form", "orders-form", Operation::Read));
    assert!(can(&c, r, "form", "orders-form", Operation::View));
    assert!(can(&c, r, "form", "orders-form", Operation::Update));
    assert!(!can(&c, r, "form", "orders-form", Operation::Delete));
    assert!(!can(&c, r, "table", "orders", Operation::Create));
    assert!(!can(&c, r, "form", "customers-form", Operation::Read));
    assert!(can(&c, r, "navigation", "orders", Operation::View));
    assert!(!can(&c, r, "navigation", "customers", Operation::View));
    assert!(can(&c, r, "action", "approve", Operation::Execute));
    assert!(!can(&c, r, "action", "approve", Operation::Read));
    assert!(!can(&c, r, "action", "purge", Operation::Execute));
}

fn dashboard(id: &str) -> Dashboard {
    serde_json::from_value(json!({
        "id": id, "name": id,
        "filters": [{"id": "f", "name": "Region", "param": "region", "optionsQueryId": "q-regions"}],
        "components": [
            {"id": "kpi", "kind": "kpi", "queryId": "q-sales"},
            {"id": "chart", "kind": "chart", "queryId": "q-chart"},
            {"id": "table", "kind": "table", "queryId": "q-table"},
            {"id": "form", "kind": "form", "formId": "orders-form", "queryId": "q-form"},
            {"id": "text", "kind": "text", "queryId": "q-stray"},
        ]
    }))
    .unwrap()
}

#[test]
fn lets_a_dashboard_grant_read_the_dashboards_queries_and_nothing_else() {
    let _s = serial();
    reset();
    let viewer = Role {
        id: "viewer".into(),
        name: "Viewer".into(),
        permissions: Permissions {
            objects: vec![object("dashboard", "d1", [true, false, false, false])],
            ..Default::default()
        },
    };
    let c = DocumentConfig {
        roles: vec![clerk(), viewer],
        ..Default::default()
    };
    let (d1, d2) = (dashboard("d1"), dashboard("d2"));
    assert_eq!(
        dashboard_query_ids(&d1),
        ["q-sales", "q-chart", "q-table", "q-regions"]
    );
    for q in ["q-sales", "q-chart", "q-table", "q-regions"] {
        assert!(can_read_dashboard_query(&c, Some("viewer"), &d1, q), "{q}");
    }
    for q in ["q-form", "q-stray", "q-other"] {
        assert!(!can_read_dashboard_query(&c, Some("viewer"), &d1, q), "{q}");
    }
    assert!(!can_read_dashboard_query(&c, Some("viewer"), &d2, "q-sales"));
    assert!(!can(&c, Some("viewer"), "query", "q-sales", Operation::Read));
    assert!(!can(&c, Some("viewer"), "query", "q-sales", Operation::Update));
    assert!(!can_read_dashboard_query(&c, Some("clerk"), &d1, "q-sales"));
    assert!(can_read_dashboard_query(&c, None, &d1, "q-sales"));
}

#[test]
fn denies_unknown_roles() {
    let _s = serial();
    reset();
    assert!(!can(&config(), Some("ghost"), "form", "orders-form", Operation::Read));
}

#[test]
fn edits_permission_flags_consistently() {
    let mut permissions = clerk().permissions;
    permissions = set_object_permission(&permissions, "report", "r1", "delete", true);
    let last = permissions.objects.last().unwrap();
    assert_eq!((last.kind.as_str(), last.read, last.delete), ("report", true, true));
    permissions = set_object_permission(&permissions, "report", "r1", "read", false);
    assert!(!permissions.objects.iter().any(|o| o.kind == "report"));
    let list = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(toggle_listed(&list(&["a"]), "b", true), list(&["a", "b"]));
    assert_eq!(toggle_listed(&list(&["a", "b"]), "a", false), list(&["b"]));
}

#[test]
fn authorization_follows_the_active_role() {
    let _s = serial();
    reset();
    let c = DocumentConfig {
        roles: vec![role("clerk", &["approve"]), role("viewer", &[])],
        ..Default::default()
    };
    let authorize = authorizer(c, "w".into(), None);
    assert_eq!(active_role_id("w"), None);
    assert!(authorize("action", "purge", "execute"));
    set_previewed_role("w", Some("clerk"));
    assert_eq!(active_role_id("w").as_deref(), Some("clerk"));
    assert!(authorize("action", "approve", "execute"));
    assert!(!authorize("action", "purge", "execute"));
    assign_runtime_role(Some(AssignedRole {
        role: role("viewer", &[]),
        user_name: "Ada".into(),
        user_email: None,
        user_id: None,
    }));
    assert_eq!(active_role_id("w").as_deref(), Some("viewer"));
    assert!(!authorize("action", "approve", "execute"));
    reset();
}

#[test]
fn never_lets_a_background_job_exceed_the_role_it_was_created_under() {
    let _s = serial();
    reset();
    let c = DocumentConfig {
        roles: vec![role("clerk", &["approve"]), role("viewer", &[])],
        ..Default::default()
    };
    let ctx = |role: Option<&str>| authorizer(c.clone(), "w".into(), role.map(str::to_string));
    assert!(ctx(None)("action", "approve", "execute"));
    assert!(ctx(Some("clerk"))("action", "approve", "execute"));
    assert!(!ctx(Some("viewer"))("action", "approve", "execute"));
    set_previewed_role("w", Some("viewer"));
    assert!(!ctx(Some("clerk"))("action", "approve", "execute"));
    reset();
}

#[test]
fn a_cloud_assigned_role_replaces_developer_access_and_local_roles() {
    let _s = serial();
    reset();
    let c = DocumentConfig {
        roles: vec![Role {
            id: "admin".into(),
            name: "Admin".into(),
            permissions: Permissions {
                navigation: vec!["nav-orders".into()],
                objects: vec![object("form", "orders", [true, true, true, true])],
                actions: vec!["approve".into()],
            },
        }],
        ..Default::default()
    };
    assert!(can(&c, None, "form", "orders", Operation::Delete));
    assign_runtime_role(Some(AssignedRole {
        role: Role {
            id: "sales".into(),
            name: "Sales".into(),
            permissions: Permissions {
                navigation: vec!["nav-orders".into()],
                objects: vec![object("form", "orders", [true, false, false, false])],
                actions: vec![],
            },
        },
        user_name: "Ada".into(),
        user_email: Some("ada@example.com".into()),
        user_id: Some("u1".into()),
    }));
    assert!(can(&c, None, "form", "orders", Operation::Read));
    assert!(!can(&c, None, "form", "orders", Operation::Delete));
    assert!(can(&c, Some("sales"), "navigation", "nav-orders", Operation::View));
    assert!(!can(&c, Some("sales"), "action", "approve", Operation::Execute));
    assert!(!can(&c, Some("admin"), "form", "orders", Operation::Delete));
    reset();
    assert!(can(&c, None, "form", "orders", Operation::Delete));
}
