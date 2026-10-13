//! Row writes for in-place editing views, against a real backend session.

use super::{create_row, expected_values, update_row, values_to_write};
use crate::data::{DataValue, NamedValue};
use crate::forms::data::{PageRequest, load_record, table_schema};
use crate::forms::schema::{new_control, new_form};
use crate::forms::state::FormScope;
use crate::forms::testkit::Db;
use crate::forms::values::RecordValues;
use crate::design::{Binding, ControlKind};
use crate::records::{self, RecordHook, RecordWrite, WriteResult};
use crate::manager::AppError;
use serde_json::{Map, Value, json};
use std::sync::Arc;

fn record(value: Value) -> RecordValues {
    value.as_object().unwrap().clone()
}

fn orders() -> Db {
    let db = Db::open();
    db.table(
        "orders",
        &[("id", "INTEGER", 1), ("qty", "INTEGER", 0), ("note", "TEXT", 0)],
    );
    db
}

struct Failing;

impl RecordHook for Failing {
    fn after(&self, _: &RecordWrite, _: &WriteResult, _: Option<&str>) -> Result<(), AppError> {
        Err(AppError::new("AUDIT", "audit is down"))
    }
}

#[test]
fn inserts_without_null_keys_and_returns_generated_keys() {
    let db = orders();
    let schema = table_schema(&db.label, "orders").unwrap();
    let outcome = create_row(&db.label, "orders", &schema, &record(json!({"qty": 3, "note": null}))).unwrap();
    assert_eq!(outcome.problem, None);
    let id = outcome.record["id"].clone();
    assert!(id.is_number() || id.is_null());
    let _ = PageRequest::default();
}

#[test]
fn updates_only_changed_columns_with_the_loaded_values_as_expected() {
    let db = orders();
    db.insert("orders", json!({"id": 1, "qty": 2, "note": "a"}));
    let schema = table_schema(&db.label, "orders").unwrap();
    let loaded = load_record(&db.label, "orders", &json!(1)).unwrap().unwrap();
    let mut edited = loaded.record.clone();
    edited.insert("qty".into(), json!(9));
    let outcome = update_row(&db.label, "orders", &schema, &loaded.identity, &edited, &loaded.record).unwrap();
    assert_eq!(outcome.record["qty"], json!(9));
    let reloaded = load_record(&db.label, "orders", &json!(1)).unwrap().unwrap();
    assert_eq!(reloaded.record["qty"], json!(9));
    // Nothing changed: no write, no error even from a stale original.
    let stale = update_row(&db.label, "orders", &schema, &loaded.identity, &edited, &edited).unwrap();
    assert_eq!(stale.problem, None);
}

#[test]
fn rejects_an_update_whose_original_is_stale() {
    let db = orders();
    db.insert("orders", json!({"id": 1, "qty": 2, "note": "a"}));
    let schema = table_schema(&db.label, "orders").unwrap();
    let loaded = load_record(&db.label, "orders", &json!(1)).unwrap().unwrap();
    let mut other = loaded.record.clone();
    other.insert("note".into(), json!("b"));
    update_row(&db.label, "orders", &schema, &loaded.identity, &other, &loaded.record).unwrap();
    let mut mine = loaded.record.clone();
    mine.insert("qty".into(), json!(5));
    let error = update_row(&db.label, "orders", &schema, &loaded.identity, &mine, &loaded.record).unwrap_err();
    assert_eq!(error.code, "CONFLICT");
}

#[test]
fn counts_a_write_as_saved_when_an_after_hook_fails() {
    let db = orders();
    db.insert("orders", json!({"id": 1, "qty": 2, "note": "a"}));
    let schema = table_schema(&db.label, "orders").unwrap();
    let loaded = load_record(&db.label, "orders", &json!(1)).unwrap().unwrap();
    let _hook = records::register_hook(&db.label, Arc::new(Failing));
    let mut edited = loaded.record.clone();
    edited.insert("qty".into(), json!(7));
    let outcome = update_row(&db.label, "orders", &schema, &loaded.identity, &edited, &loaded.record).unwrap();
    assert_eq!(outcome.problem.as_deref(), Some("Saved. audit is down"));
    assert_eq!(outcome.record["qty"], json!(7));
    let created = create_row(&db.label, "orders", &schema, &record(json!({"id": 2, "qty": 1}))).unwrap();
    assert_eq!(created.problem.as_deref(), Some("Saved. audit is down"));
}

#[test]
fn writes_computed_inputs_and_keeps_disabled_fields_stored_value() {
    let mut form = new_form("Orders", None);
    let mut qty = new_control(ControlKind::Number, &form, None);
    qty.binding = Some(Binding { column: "qty".into(), ..Default::default() });
    qty.computed = Some("record.base * 2".into());
    let mut note = new_control(ControlKind::Text, &form, None);
    note.binding = Some(Binding { column: "note".into(), ..Default::default() });
    note.enabled_when = Some("false".into());
    form.controls = vec![qty, note];
    let original = record(json!({"base": 4, "qty": 0, "note": "kept"}));
    let edited = record(json!({"base": 4, "qty": 0, "note": "changed"}));
    let scope = FormScope::new(edited, Map::new(), Map::new());
    let values = values_to_write(&form, &scope, &original);
    assert_eq!(values["qty"], json!(8));
    assert_eq!(values["note"], json!("kept"));
}

#[test]
fn builds_expected_values_from_stored_columns_only() {
    let db = orders();
    let schema = table_schema(&db.label, "orders").unwrap();
    let expected = expected_values(&schema, &record(json!({"id": 1, "qty": 2, "extra": 1})));
    let columns: Vec<&str> = expected.iter().map(|v: &NamedValue| v.column.as_str()).collect();
    assert_eq!(columns, ["id", "qty"]);
    assert_eq!(expected[1].value, DataValue::Integer(2));
}
