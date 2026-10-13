//! Port of tests/unit/runtime-lookups.test.ts against a real backend session (the TypeScript
//! test mocked `invoke`); call counts become cache assertions.

use super::{
    column_lookups, guess_display_column, label_for, label_query, lookup_key, lookup_labels,
};
use crate::data::{DataValue, NamedValue};
use crate::design::{KeyPair, Relationship};
use crate::forms::testkit::{Db, col, pk, table};
use serde_json::{Value, json};

fn customers() -> Db {
    let db = Db::open();
    db.table(
        "customers",
        &[("id", "INTEGER", 1), ("code", "TEXT", 0), ("Name", "TEXT", 0)],
    );
    db.table("regions", &[("id", "INTEGER", 1), ("code", "TEXT", 0)]);
    db.table_with(
        "orders",
        &[("id", "INTEGER", 1), ("customer_id", "INTEGER", 0), ("region_id", "INTEGER", 0)],
        json!([
            {"columns": ["customer_id"], "targetTable": "customers", "targetColumns": ["id"]},
            {"columns": ["region_id"], "targetTable": "regions", "targetColumns": ["id"]},
        ]),
    );
    db.insert("customers", json!({"id": 1, "code": "AC", "Name": "Acme"}));
    db.insert("customers", json!({"id": 2, "code": "GX", "Name": "Globex"}));
    db
}

fn rel(table: &str, value: &str, display: &str) -> Relationship {
    Relationship {
        table: table.into(),
        value_column: value.into(),
        display_column: display.into(),
        ..Default::default()
    }
}

fn row(value: Value) -> crate::forms::values::RecordValues {
    value.as_object().unwrap().clone()
}

fn entries(labels: &std::collections::HashMap<String, String>) -> Vec<(String, String)> {
    let mut all: Vec<_> = labels.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    all.sort();
    all
}

#[test]
fn guesses_a_text_column_named_name_title_or_label() {
    let with_name = table(
        "customers",
        vec![pk(col("id", "INTEGER"), 1), col("code", "TEXT"), col("Name", "TEXT")],
        vec![],
    );
    let without = table("regions", vec![pk(col("id", "INTEGER"), 1), col("code", "TEXT")], vec![]);
    assert_eq!(guess_display_column(&with_name).as_deref(), Some("Name"));
    assert_eq!(guess_display_column(&without), None);
}

#[test]
fn prefers_the_relationship_control_falls_back_to_foreign_keys_and_skips_id_only_targets() {
    let db = customers();
    let control = rel("customers", "id", "code");
    let columns: Vec<String> = ["customer_id", "region_id", "id"].map(String::from).into();
    let lookups = column_lookups(&db.label, Some("orders"), &columns, |c| {
        (c == "customer_id").then(|| control.clone())
    });
    assert_eq!(lookups, [("customer_id".to_string(), control)]);
    let columns: Vec<String> = ["customer_id", "region_id"].map(String::from).into();
    let lookups = column_lookups(&db.label, Some("orders"), &columns, |_| None);
    assert_eq!(
        lookups,
        [("customer_id".to_string(), rel("customers", "id", "Name"))]
    );
}

#[test]
fn builds_one_bound_in_query_per_lookup() {
    let lookup = rel("customers", "id", "Name");
    let rows = [json!({"customer_id": 1}), json!({"customer_id": 2}), json!({"customer_id": 9})]
        .map(row);
    let (sql, params) = label_query(&lookup, "customer_id", &rows);
    assert_eq!(
        sql,
        "SELECT \"id\" AS lookup_key, \"Name\" AS lookup_display FROM \"customers\" WHERE \"id\" IN ($k0, $k1, $k2)"
    );
    let expected: Vec<NamedValue> = [("k0", 1), ("k1", 2), ("k2", 9)]
        .into_iter()
        .map(|(column, value)| NamedValue {
            column: column.into(),
            value: DataValue::Integer(value),
        })
        .collect();
    assert_eq!(params, expected);
}

#[test]
fn reads_labels_and_caches_them_until_a_record_write() {
    let db = customers();
    let lookups = vec![("customer_id".to_string(), rel("customers", "id", "Name"))];
    let rows = [
        json!({"customer_id": 1}),
        json!({"customer_id": 2}),
        json!({"customer_id": 1}),
        json!({"customer_id": 9}),
    ]
    .map(row);
    let labels = lookup_labels(&db.label, &lookups, &rows);
    assert_eq!(
        entries(&labels["customer_id"]),
        [("1".into(), "Acme".into()), ("2".into(), "Globex".into())]
    );
    assert_eq!(
        label_for(&lookups, &labels, "customer_id", &rows[0]).as_deref(),
        Some("Acme")
    );
    assert_eq!(label_for(&lookups, &labels, "customer_id", &rows[3]), None);
    // A change that bypasses the record layer is not seen: the labels come from the cache.
    crate::recordstore::commands::update_row(
        db.label.clone(),
        "customers".into(),
        vec![NamedValue {
            column: "Name".into(),
            value: DataValue::Text("Acme 2".into()),
        }],
        vec![DataValue::Integer(1)],
        Some(vec![NamedValue {
            column: "Name".into(),
            value: DataValue::Text("Acme".into()),
        }]),
        None,
    )
    .unwrap();
    let cached = lookup_labels(&db.label, &lookups, &rows);
    assert_eq!(cached["customer_id"]["1"], "Acme");
    // A write through the record layer clears the cache.
    db.insert("customers", json!({"id": 3, "code": "ZZ", "Name": "Zed"}));
    let fresh = lookup_labels(&db.label, &lookups, &rows);
    assert_eq!(fresh["customer_id"]["1"], "Acme 2");
}

#[test]
fn reads_labels_with_a_filtered_table_page_when_a_runtime_role_may_not_run_sql() {
    let db = customers();
    db.configure(|config| {
        config.roles = serde_json::from_value(json!([{
            "id": "clerk", "name": "Clerk",
            "permissions": {"navigation": [], "actions": [], "objects": [
                {"kind": "table", "id": "customers", "read": true, "create": false,
                 "update": false, "delete": false}
            ]}
        }]))
        .unwrap();
    });
    crate::authz::set_runtime_role_preview(db.label.clone(), Some("clerk".into())).unwrap();
    let lookups = vec![("customer_id".to_string(), rel("customers", "id", "code"))];
    let rows = [json!({"customer_id": 1}), json!({"customer_id": 2})].map(row);
    let labels = lookup_labels(&db.label, &lookups, &rows);
    assert_eq!(
        entries(&labels["customer_id"]),
        [("1".into(), "AC".into()), ("2".into(), "GX".into())]
    );
}

#[test]
fn labels_a_multi_column_relationship_by_every_key_column() {
    let db = Db::open();
    db.table(
        "thresholds",
        &[
            ("product_id", "INTEGER", 1),
            ("location_id", "INTEGER", 2),
            ("label", "TEXT", 0),
        ],
    );
    db.insert(
        "thresholds",
        json!({"product_id": 17, "location_id": 2, "label": "Bolts at Depot"}),
    );
    let relationship = Relationship {
        keys: vec![
            KeyPair { column: "product_id".into(), target: "product_id".into() },
            KeyPair { column: "location_id".into(), target: "location_id".into() },
        ],
        ..rel("thresholds", "product_id", "label")
    };
    let columns = vec!["product_id".to_string()];
    let lookups = column_lookups(&db.label, Some("orders"), &columns, |_| Some(relationship.clone()));
    assert_eq!(lookups, [("product_id".to_string(), relationship.clone())]);
    let rows = [
        json!({"product_id": 17, "location_id": 2}),
        json!({"product_id": 17, "location_id": null}),
    ]
    .map(row);
    let (sql, _) = label_query(&relationship, "product_id", &rows[..1]);
    assert_eq!(
        sql,
        "SELECT \"product_id\" AS lookup_key0, \"location_id\" AS lookup_key1, \"label\" AS lookup_display FROM \"thresholds\" WHERE (\"product_id\" = $k0_0 AND \"location_id\" = $k0_1)"
    );
    let labels = lookup_labels(&db.label, &lookups, &rows);
    let key = lookup_key(&relationship, "product_id", &rows[0]).unwrap();
    assert_eq!(labels["product_id"][&key], "Bolts at Depot");
    assert_eq!(lookup_key(&relationship, "product_id", &rows[1]), None);
}
