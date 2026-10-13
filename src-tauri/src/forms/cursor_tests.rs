//! Record cursor positions read through the same page reader as list mode.

use super::{RecordCursor, id_for_row, record_at};
use crate::data::Sort;
use crate::design::{Form, FormSource, SourceKind};
use crate::forms::data::{PageRequest, load_page};
use crate::forms::schema::new_form;
use crate::forms::testkit::Db;
use serde_json::{Map, Value, json};

fn form(table: &str, filter: Option<&str>) -> Form {
    let mut form = new_form(
        "Items",
        Some(FormSource {
            kind: SourceKind::Table,
            table: Some(table.into()),
            ..Default::default()
        }),
    );
    form.filter = filter.map(str::to_string);
    form
}

fn items() -> Db {
    let db = Db::open();
    db.table("items", &[("id", "INTEGER", 1), ("status", "TEXT", 0)]);
    for id in 1..=5 {
        db.insert("items", json!({"id": id, "status": if id % 2 == 0 { "won" } else { "open" }}));
    }
    db
}

fn cursor(descending: bool) -> RecordCursor {
    RecordCursor {
        sorts: vec![Sort { column: "id".into(), descending }],
        ..Default::default()
    }
}

#[test]
fn reads_the_record_id_and_total_at_each_position_in_the_list_order() {
    let db = items();
    let form = form("items", None);
    let app = Map::new();
    let first = record_at(&db.label, &form, &cursor(true), 0, &app).unwrap();
    assert_eq!((first.record_id, first.total), (json!(5), 5));
    let third = record_at(&db.label, &form, &cursor(true), 2, &app).unwrap();
    assert_eq!(third.record_id, json!(3));
}

#[test]
fn returns_a_null_id_past_the_end_and_follows_the_row_filter() {
    let db = items();
    let app = Map::new();
    let past = record_at(&db.label, &form("items", None), &cursor(false), 9, &app).unwrap();
    assert_eq!((past.record_id, past.total), (Value::Null, 5));
    let filtered = form("items", Some("record.status = \"won\""));
    let last = record_at(&db.label, &filtered, &cursor(false), 1, &app).unwrap();
    assert_eq!((last.record_id, last.total), (json!(4), 2));
}

#[test]
fn opens_query_rows_and_keyless_rows_by_the_row_itself() {
    let db = items();
    let form = form("items", None);
    let page = load_page(
        &db.label,
        &form,
        &PageRequest { limit: 2, ..Default::default() },
        &Map::new(),
        &Map::new(),
    )
    .unwrap();
    assert_eq!(id_for_row(&db.label, &form, &page, 1).unwrap(), json!(page.rows[1]["id"]));
    assert_eq!(id_for_row(&db.label, &form, &page, 7).unwrap(), Value::Null);
    let mut keyless = page.clone();
    keyless.identities = None;
    let id = id_for_row(&db.label, &form, &keyless, 0).unwrap();
    assert_eq!(id, Value::Object(page.rows[0].clone()));
}
