// Runtime authorization tests (included from authz.rs).
use super::*;
use crate::manager::DocumentManager;
use crate::roles::{ObjectPermission, Permissions};
use serde_json::json;

fn grant(kind: &str, id: &str, flags: &[Op]) -> ObjectPermission {
    ObjectPermission {
        kind: kind.into(),
        id: id.into(),
        read: flags.contains(&Op::Read),
        create: flags.contains(&Op::Create),
        update: flags.contains(&Op::Update),
        delete: flags.contains(&Op::Delete),
    }
}

fn role(objects: Vec<ObjectPermission>, actions: &[&str]) -> Role {
    Role {
        id: "clerk".into(),
        name: "Clerk".into(),
        permissions: Permissions {
            navigation: vec!["nav-1".into()],
            objects,
            actions: actions.iter().map(|a| a.to_string()).collect(),
        },
    }
}

/// Forms on `orders` (with a customer lookup, an order-lines related list, and
/// an options query) and on a query; a report on `invoices` with a table query.
fn config() -> DocumentConfig {
    let mut config = DocumentConfig::default();
    config.design.forms = serde_json::from_value(json!([
        {
            "id": "f-orders", "name": "Orders",
            "source": {"kind": "table", "table": "orders"},
            "controls": [
                {"id": "c1", "kind": "relationship", "label": "Customer",
                 "relationship": {"table": "customers", "valueColumn": "id", "displayColumn": "name"}},
                {"id": "c2", "kind": "relatedList", "label": "Lines",
                 "related": {"table": "order_lines", "foreignKey": "order_id", "parentColumn": "id"}},
                {"id": "c3", "kind": "select", "label": "Status", "optionsQueryId": "q-status"}
            ]
        },
        {"id": "f-summary", "name": "Summary", "source": {"kind": "query", "queryId": "q-summary"}}
    ]))
    .unwrap();
    config.reports = serde_json::from_value(json!([{
        "id": "r-invoices", "name": "Invoices", "table": "invoices",
        "bands": {"detail": {"height": 10, "components": [
            {"id": "t1", "kind": "table", "x": 0, "y": 0, "w": 10, "h": 10, "queryId": "q-lines"}
        ]}}
    }]))
    .unwrap();
    config
}

fn no_fk(_: &str) -> Vec<String> {
    vec![]
}

#[test]
fn explicit_grants_mirror_the_frontend_can() {
    let c = config();
    let r = role(
        vec![grant("table", "notes", &[Op::Read, Op::Create])],
        &["act-1"],
    );
    assert!(allows(&c, &r, "table", "notes", Op::Read, &no_fk));
    assert!(allows(&c, &r, "table", "notes", Op::Create, &no_fk));
    assert!(!allows(&c, &r, "table", "notes", Op::Update, &no_fk));
    assert!(!allows(&c, &r, "table", "notes", Op::Delete, &no_fk));
    assert!(!allows(&c, &r, "table", "other", Op::Read, &no_fk));
    assert!(allows(&c, &r, "navigation", "nav-1", Op::Read, &no_fk));
    assert!(allows(&c, &r, "action", "act-1", Op::Execute, &no_fk));
    assert!(!allows(&c, &r, "action", "act-1", Op::Read, &no_fk));
    assert!(!allows(&c, &r, "action", "act-2", Op::Execute, &no_fk));
    assert!(!allows(&c, &deny_all(), "table", "notes", Op::Read, &no_fk));
}

#[test]
fn a_form_grants_its_source_table_lookups_related_lists_and_queries() {
    let c = config();
    let r = role(
        vec![grant(
            "form",
            "f-orders",
            &[Op::Read, Op::Create, Op::Update],
        )],
        &[],
    );
    for op in [Op::Read, Op::Create, Op::Update] {
        assert!(allows(&c, &r, "table", "orders", op, &no_fk), "{op:?}");
    }
    assert!(!allows(&c, &r, "table", "orders", Op::Delete, &no_fk));
    // Lookups and related lists are readable, not writable.
    for table in ["customers", "order_lines"] {
        assert!(allows(&c, &r, "table", table, Op::Read, &no_fk), "{table}");
        assert!(
            !allows(&c, &r, "table", table, Op::Create, &no_fk),
            "{table}"
        );
    }
    assert!(allows(&c, &r, "query", "q-status", Op::Read, &no_fk));
    assert!(!allows(&c, &r, "query", "q-summary", Op::Read, &no_fk));
    assert!(!allows(&c, &r, "table", "invoices", Op::Read, &no_fk));
    // Foreign-key targets of readable tables show as list lookups.
    let fk = |t: &str| {
        if t == "orders" {
            vec!["regions".to_string()]
        } else {
            vec![]
        }
    };
    assert!(allows(&c, &r, "table", "regions", Op::Read, &fk));
    assert!(!allows(&c, &r, "table", "regions", Op::Update, &fk));
    // A query-sourced form grants its query.
    let q = role(vec![grant("form", "f-summary", &[Op::Read])], &[]);
    assert!(allows(&c, &q, "query", "q-summary", Op::Read, &no_fk));
    assert!(!allows(&c, &q, "table", "orders", Op::Read, &no_fk));
}

#[test]
fn a_report_grants_its_dataset_and_table_queries_for_reading_only() {
    let c = config();
    let r = role(vec![grant("report", "r-invoices", &[Op::Read])], &[]);
    assert!(allows(&c, &r, "report", "r-invoices", Op::Read, &no_fk));
    assert!(allows(&c, &r, "table", "invoices", Op::Read, &no_fk));
    assert!(!allows(&c, &r, "table", "invoices", Op::Create, &no_fk));
    assert!(allows(&c, &r, "query", "q-lines", Op::Read, &no_fk));
    assert!(!allows(
        &c,
        &deny_all(),
        "report",
        "r-invoices",
        Op::Read,
        &no_fk
    ));
}

/// `config()` plus a dashboard whose components and filter read queries, which
/// also embeds query-reading forms and a report, runs an action, and carries a
/// stray `queryId` on a text component.
fn dashboard_config() -> DocumentConfig {
    let mut c = config();
    c.pages = serde_json::from_value(json!([
        {
            "id": "d-sales", "name": "Sales",
            "filters": [{"id": "fl1", "name": "Region", "param": "region",
                         "control": "select", "optionsQueryId": "q-regions"}],
            "components": [
                {"id": "k1", "kind": "kpi", "queryId": "q-kpi"},
                {"id": "c1", "kind": "chart", "queryId": "q-chart"},
                {"id": "t1", "kind": "table", "queryId": "q-table"},
                {"id": "e1", "kind": "form", "formId": "f-summary"},
                {"id": "e2", "kind": "form", "formId": "f-orders"},
                {"id": "r1", "kind": "report", "reportId": "r-invoices"},
                {"id": "b1", "kind": "button", "actionId": "act-1"},
                {"id": "x1", "kind": "text", "queryId": "q-stray"}
            ]
        },
        {"id": "d-other", "name": "Other",
         "components": [{"id": "k1", "kind": "kpi", "queryId": "q-other"}]}
    ]))
    .unwrap();
    c
}

#[test]
fn a_dashboard_grants_read_on_its_component_and_filter_queries_only() {
    let c = dashboard_config();
    let r = role(vec![grant("page", "d-sales", &[Op::Read])], &[]);
    let queries = ["q-kpi", "q-chart", "q-table", "q-regions"];
    for q in queries {
        assert!(allows(&c, &r, "query", q, Op::Read, &no_fk), "{q}");
        for op in [Op::Create, Op::Update, Op::Delete, Op::Execute] {
            assert!(!allows(&c, &r, "query", q, op, &no_fk), "{q} {op:?}");
        }
    }
    // Another dashboard's query, embedded forms' and reports' queries, and a
    // `queryId` on a component kind that runs no query stay closed.
    for q in ["q-other", "q-summary", "q-status", "q-lines", "q-stray"] {
        assert!(!allows(&c, &r, "query", q, Op::Read, &no_fk), "{q}");
    }
    // Granting the embedded form opens its queries through the form.
    let both = role(
        vec![
            grant("page", "d-sales", &[Op::Read]),
            grant("form", "f-summary", &[Op::Read]),
        ],
        &[],
    );
    assert!(allows(&c, &both, "query", "q-summary", Op::Read, &no_fk));
    assert!(!allows(&c, &both, "query", "q-status", Op::Read, &no_fk));
    // Embedded forms, reports, their tables and actions are not granted.
    for (kind, id) in [
        ("form", "f-orders"),
        ("form", "f-summary"),
        ("report", "r-invoices"),
        ("table", "orders"),
        ("table", "customers"),
        ("table", "invoices"),
    ] {
        assert!(!allows(&c, &r, kind, id, Op::Read, &no_fk), "{kind} {id}");
    }
    assert!(!allows(&c, &r, "table", "orders", Op::Create, &no_fk));
    assert!(!allows(&c, &r, "action", "act-1", Op::Execute, &no_fk));
    // Without the dashboard grant nothing is implied.
    assert!(!allows(&c, &deny_all(), "query", "q-kpi", Op::Read, &no_fk));
    let none = role(vec![grant("page", "d-other", &[Op::Read])], &[]);
    assert!(!allows(&c, &none, "query", "q-kpi", Op::Read, &no_fk));
    assert!(allows(&c, &none, "query", "q-other", Op::Read, &no_fk));
}

#[test]
fn a_previewed_role_granted_only_a_dashboard_runs_its_queries() {
    let (m, base) = session();
    m.with_session("w", |s| {
        s.doc.config.pages = dashboard_config().pages;
        s.doc.config.roles = vec![role(vec![grant("page", "d-sales", &[Op::Read])], &[])];
        preview_role(s, Some("clerk".into()))
    })
    .unwrap();
    let check =
        |kind: &str, id: &str, op: Op| m.with_session("w", |s| check_session(s, kind, id, op));
    assert!(check("query", "q-chart", Op::Read).is_ok());
    assert_eq!(
        check("query", "q-summary", Op::Read).unwrap_err().code,
        "FORBIDDEN"
    );
    assert_eq!(
        check("query", "q-other", Op::Read).unwrap_err().code,
        "FORBIDDEN"
    );
    assert_eq!(
        check("table", "orders", Op::Read).unwrap_err().code,
        "FORBIDDEN"
    );
    let _ = m.close("w", true);
    let _ = std::fs::remove_dir_all(base);
}

fn session() -> (DocumentManager, std::path::PathBuf) {
    let base = std::env::temp_dir().join(format!("ixtable-authz-{}", uuid::Uuid::new_v4()));
    let m = DocumentManager::new(base.join("data"), base.join("cache")).unwrap();
    m.new_session("w").unwrap();
    m.with_session("w", |s| {
        s.doc.config.design.forms = config().design.forms;
        s.doc.config.reports = config().reports;
        s.doc.config.roles = vec![role(
            vec![
                grant("form", "f-orders", &[Op::Read, Op::Create]),
                grant("query", "q-open", &[Op::Read]),
            ],
            &[],
        )];
        Ok(())
    })
    .unwrap();
    (m, base)
}

#[test]
fn sessions_are_unrestricted_until_a_role_is_previewed_and_guard_every_command_family() {
    let (m, base) = session();
    let check =
        |kind: &str, id: &str, op: Op| m.with_session("w", |s| check_session(s, kind, id, op));
    let adhoc = || m.with_session("w", |s| unrestricted_session(s, "run ad hoc SQL"));
    // Developer: everything.
    assert!(check("table", "secrets", Op::Delete).is_ok());
    assert!(adhoc().is_ok());
    m.with_session("w", |s| preview_role(s, Some("clerk".into())))
        .unwrap();
    // insert_row / update_row / delete_row / execute_write_batch.
    assert!(check("table", "orders", Op::Create).is_ok());
    let e = check("table", "orders", Op::Delete).unwrap_err();
    assert_eq!(e.code, "FORBIDDEN");
    assert!(e.message.contains("Clerk") && e.message.contains("orders"));
    assert_eq!(
        check("table", "secrets", Op::Create).unwrap_err().code,
        "FORBIDDEN"
    );
    // read_table_page.
    assert!(check("table", "customers", Op::Read).is_ok());
    assert_eq!(
        check("table", "secrets", Op::Read).unwrap_err().code,
        "FORBIDDEN"
    );
    // run_saved_query / run_saved_query_page.
    assert!(check("query", "q-open", Op::Read).is_ok());
    assert_eq!(
        check("query", "q-other", Op::Read).unwrap_err().code,
        "FORBIDDEN"
    );
    // write_report_pdf.
    assert_eq!(
        check("report", "r-invoices", Op::Read).unwrap_err().code,
        "FORBIDDEN"
    );
    // execute_read_query / execute_parameterized_query / export_attachment.
    assert_eq!(adhoc().unwrap_err().code, "FORBIDDEN");
    // An unknown role is allowed nothing; ending the preview restores access.
    m.with_session("w", |s| preview_role(s, Some("ghost".into())))
        .unwrap();
    assert_eq!(
        check("table", "orders", Op::Read).unwrap_err().code,
        "FORBIDDEN"
    );
    m.with_session("w", |s| preview_role(s, None)).unwrap();
    assert!(check("table", "secrets", Op::Delete).is_ok());
    assert!(adhoc().is_ok());
    // A trusted assignment: Unrestricted for the owner, or a role.
    m.with_session("w", |s| {
        s.access = Access::Role(deny_all());
        Ok(())
    })
    .unwrap();
    assert_eq!(
        check("table", "orders", Op::Read).unwrap_err().code,
        "FORBIDDEN"
    );
    m.with_session("w", |s| {
        s.access = Access::Unrestricted;
        Ok(())
    })
    .unwrap();
    assert!(check("table", "orders", Op::Read).is_ok());
    let _ = m.close("w", true);
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn the_readable_table_set_is_cached_per_role_until_cleared() {
    let cache = ReadableCache::default();
    let calls = std::cell::Cell::new(0);
    let compute = || {
        calls.set(calls.get() + 1);
        HashSet::from(["orders".to_string()])
    };
    let clerk = role(vec![], &[]);
    assert!(cache.get_or(&clerk, compute).contains("orders"));
    assert!(cache.get_or(&clerk, compute).contains("orders"));
    assert_eq!(calls.get(), 1);
    cache.get_or(&deny_all(), compute);
    assert_eq!(calls.get(), 2);
    cache.clear();
    cache.get_or(&deny_all(), compute);
    assert_eq!(calls.get(), 3);
}

#[test]
fn listing_and_inspecting_tables_follow_read_access() {
    let (m, base) = session();
    m.with_session("w", |s| preview_role(s, Some("clerk".into())))
        .unwrap();
    let readable = |t: &str| m.with_session("w", |s| Ok(can_read_table(s, t))).unwrap();
    assert!(readable("orders"));
    assert!(!readable("secrets"));
    m.with_session("w", |s| preview_role(s, None)).unwrap();
    assert!(readable("secrets"));
    let _ = m.close("w", true);
    let _ = std::fs::remove_dir_all(base);
}
