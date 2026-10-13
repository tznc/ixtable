//! Port of tests/unit/runtime-source-params.test.ts and the stale-label half of
//! tests/unit/runtime-ux.test.tsx, plus data-layer coverage the TypeScript suite got from
//! integration tests: pushed and residual row filters, paging over a filtered scan, record
//! ids and loading, relationship choices, and the cache.

use super::{
    Bound, PageRequest, cached_page, cached_relationship_label, export_filters, first_record_id,
    load_page, load_record, passes_filter, record_id_for, relationship_choices,
    relationship_label, source_params, table_schema,
};
use crate::data::Sort;
use crate::design::{Form, FormSource, Relationship, SourceKind};
use crate::expr::Scope;
use crate::forms::schema::new_form;
use crate::forms::testkit::Db;
use serde_json::{Value, json};

fn scope(value: Value) -> Scope {
    value.as_object().unwrap().clone()
}

fn table_form(table: &str, filter: Option<&str>) -> Form {
    let mut form = new_form(
        "T",
        Some(FormSource {
            kind: SourceKind::Table,
            table: Some(table.into()),
            ..Default::default()
        }),
    );
    form.filter = filter.map(str::to_string);
    form
}

fn request(offset: u64, limit: u64) -> PageRequest {
    PageRequest {
        offset,
        limit,
        sorts: vec![Sort {
            column: "id".into(),
            descending: false,
        }],
        filters: vec![],
    }
}

fn ids(rows: &[crate::forms::values::RecordValues]) -> Vec<i64> {
    rows.iter().map(|r| r["id"].as_i64().unwrap()).collect()
}

fn items() -> Db {
    let db = Db::open();
    db.table(
        "items",
        &[("id", "INTEGER", 1), ("qty", "INTEGER", 0), ("status", "TEXT", 0)],
    );
    for id in 1..=10 {
        db.insert(
            "items",
            json!({"id": id, "qty": id, "status": if id % 3 == 0 { "won" } else { "open" }}),
        );
    }
    db
}

#[test]
fn evaluates_query_source_bindings_over_app_and_page_params() {
    let scope = scope(json!({"app": {"user": {"name": "Ana"}}, "params": {"bin": "B"}}));
    let mut form = new_form("Q", None);
    form.source = Some(FormSource {
        kind: SourceKind::Query,
        query_id: Some("q".into()),
        params: [
            ("bin".to_string(), "params.bin".to_string()),
            ("who".to_string(), "app.user.name".to_string()),
            ("blank".to_string(), " ".to_string()),
        ]
        .into(),
        ..Default::default()
    });
    let bound = source_params(&form, &scope).unwrap();
    assert_eq!(Value::Object(bound), json!({"bin": "B", "who": "Ana"}));
}

#[test]
fn ignores_table_sources_and_names_the_failing_parameter() {
    let scope = scope(json!({"app": {}, "params": {"bin": "B"}}));
    assert!(source_params(&table_form("t", None), &scope).unwrap().is_empty());
    let mut broken = new_form("Q", None);
    broken.source = Some(FormSource {
        kind: SourceKind::Query,
        query_id: Some("q".into()),
        params: [("bin".to_string(), "params.bin +".to_string())].into(),
        ..Default::default()
    });
    assert!(source_params(&broken, &scope).unwrap_err().starts_with("Parameter $bin:"));
}

#[test]
fn pages_an_unfiltered_table_and_caches_the_page_until_a_write() {
    let db = items();
    let form = table_form("items", None);
    let (scope, bound) = (Scope::new(), Bound::new());
    assert!(cached_page(&db.label, &form, &request(0, 4), &scope, &bound).is_none());
    let page = load_page(&db.label, &form, &request(0, 4), &scope, &bound).unwrap();
    assert_eq!((ids(&page.rows), page.total), (vec![1, 2, 3, 4], 10));
    assert!(page.identities.is_some());
    assert!(cached_page(&db.label, &form, &request(0, 4), &scope, &bound).is_some());
    db.insert("items", json!({"id": 11, "qty": 11, "status": "open"}));
    assert!(cached_page(&db.label, &form, &request(0, 4), &scope, &bound).is_none());
}

#[test]
fn pushes_the_filter_to_duckdb_so_the_total_is_exact() {
    let db = items();
    let form = table_form("items", Some("record.qty >= app.min and record.status = \"open\""));
    let scope = scope(json!({"app": {"min": 5}}));
    let page = load_page(&db.label, &form, &request(0, 50), &scope, &Bound::new()).unwrap();
    assert_eq!((ids(&page.rows), page.total, page.truncated), (vec![5, 7, 8, 10], 4, false));
}

#[test]
fn scans_once_for_a_residual_filter_and_pages_the_matches() {
    let db = items();
    let form = table_form("items", Some("record.qty % 2 = 0 and record.qty > 2"));
    let (scope, bound) = (Scope::new(), Bound::new());
    let first = load_page(&db.label, &form, &request(0, 2), &scope, &bound).unwrap();
    assert_eq!((ids(&first.rows), first.total), (vec![4, 6], 4));
    let second = load_page(&db.label, &form, &request(2, 2), &scope, &bound).unwrap();
    assert_eq!((ids(&second.rows), second.total), (vec![8, 10], 4));
    assert_eq!(second.identities.as_ref().map(Vec::len), Some(2));
}

#[test]
fn reports_a_filter_that_does_not_parse() {
    let db = items();
    let form = table_form("items", Some("record.qty >"));
    let error = load_page(&db.label, &form, &request(0, 5), &Scope::new(), &Bound::new()).unwrap_err();
    assert_eq!(error.code, "INVALID_FILTER");
    assert!(passes_filter(&form, &serde_json::Map::new(), &Scope::new()).is_err());
}

#[test]
fn checks_a_record_against_the_row_filter() {
    let form = table_form("items", Some("record.qty > params.min"));
    let scope = scope(json!({"params": {"min": 3}}));
    let record = |qty: i64| json!({"qty": qty}).as_object().unwrap().clone();
    assert!(passes_filter(&form, &record(4), &scope).unwrap());
    assert!(!passes_filter(&form, &record(2), &scope).unwrap());
    assert!(passes_filter(&table_form("items", None), &record(0), &scope).unwrap());
}

#[test]
fn finds_the_first_record_passing_the_filter() {
    let db = items();
    let form = table_form("items", Some("record.qty % 4 = 0"));
    let id = first_record_id(&db.label, &form, &Bound::new(), &Scope::new()).unwrap();
    assert_eq!(id, Some(json!(4)));
    let none = table_form("items", Some("record.qty > 100"));
    assert_eq!(first_record_id(&db.label, &none, &Bound::new(), &Scope::new()).unwrap(), None);
    let all = table_form("items", None);
    assert_eq!(first_record_id(&db.label, &all, &Bound::new(), &Scope::new()).unwrap(), Some(json!(1)));
    assert_eq!(
        first_record_id(&db.label, &new_form("No source", None), &Bound::new(), &Scope::new()).unwrap(),
        None
    );
}

#[test]
fn exports_only_filters_duckdb_can_apply_whole() {
    let db = items();
    let scope = Scope::new();
    assert_eq!(export_filters(&db.label, &table_form("items", None), &scope).map(|f| f.len()), Some(0));
    let whole = table_form("items", Some("record.qty >= 3"));
    assert_eq!(export_filters(&db.label, &whole, &scope).map(|f| f.len()), Some(1));
    let partial = table_form("items", Some("record.qty >= 3 and record.qty % 2 = 0"));
    assert!(export_filters(&db.label, &partial, &scope).is_none());
}

#[test]
fn loads_a_record_by_a_single_key_a_composite_key_and_a_rowid_snapshot() {
    let db = items();
    let schema = table_schema(&db.label, "items").unwrap();
    let loaded = load_record(&db.label, "items", &json!(3)).unwrap().unwrap();
    assert_eq!(loaded.record["qty"], json!(3));
    assert_eq!(load_record(&db.label, "items", &json!("3")).unwrap().unwrap().record["id"], json!(3));
    assert!(load_record(&db.label, "items", &json!(99)).unwrap().is_none());
    let page = load_page(&db.label, &table_form("items", None), &request(0, 1), &Scope::new(), &Bound::new()).unwrap();
    let id = record_id_for(&db.label, &schema, &page.rows[0], &page.identities.unwrap()[0]);
    assert_eq!(id, json!(1));

    db.table("grid", &[("a", "INTEGER", 1), ("b", "INTEGER", 2), ("note", "TEXT", 0)]);
    db.insert("grid", json!({"a": 1, "b": 2, "note": "x"}));
    db.insert("grid", json!({"a": 1, "b": 3, "note": "y"}));
    let composite = load_record(&db.label, "grid", &json!({"a": 1, "b": 3})).unwrap().unwrap();
    assert_eq!(composite.record["note"], json!("y"));
    let schema = table_schema(&db.label, "grid").unwrap();
    assert_eq!(record_id_for(&db.label, &schema, &composite.record, &composite.identity), json!({"a": 1, "b": 3}));

    db.table("loose", &[("note", "TEXT", 0)]);
    db.insert("loose", json!({"note": "z"}));
    let schema = table_schema(&db.label, "loose").unwrap();
    let page = load_page(&db.label, &table_form("loose", None), &PageRequest { limit: 1, ..Default::default() }, &Scope::new(), &Bound::new()).unwrap();
    let identity = page.identities.unwrap().remove(0);
    let id = record_id_for(&db.label, &schema, &page.rows[0], &identity);
    assert!(id.as_str().unwrap().starts_with("rowid:loose:"));
    let snap = load_record(&db.label, "loose", &id).unwrap().unwrap();
    assert_eq!((snap.record["note"].clone(), snap.identity), (json!("z"), identity));
    // The snapshot is per window label.
    let other = Db::open();
    other.table("loose", &[("note", "TEXT", 0)]);
    assert!(load_record(&other.label, "loose", &id).unwrap().is_none());
}

fn customers() -> Db {
    let db = Db::open();
    db.table("customers", &[("id", "INTEGER", 1), ("name", "TEXT", 0), ("region", "TEXT", 0)]);
    db.insert("customers", json!({"id": 1, "name": "Acme", "region": "East"}));
    db.insert("customers", json!({"id": 2, "name": "Globex", "region": "West"}));
    db.insert("customers", json!({"id": 3, "name": "Acme West", "region": "West"}));
    db
}

fn customer_rel(filter: Option<&str>) -> Relationship {
    Relationship {
        table: "customers".into(),
        value_column: "id".into(),
        display_column: "name".into(),
        filter: filter.map(str::to_string),
        ..Default::default()
    }
}

fn labels(choices: &[super::Choice]) -> Vec<&str> {
    choices.iter().map(|c| c.label.as_str()).collect()
}

#[test]
fn lists_relationship_choices_searched_on_the_display_column() {
    let db = customers();
    let all = relationship_choices(&db.label, &customer_rel(None), "", 50, &Scope::new()).unwrap();
    assert_eq!(labels(&all), ["Acme", "Acme West", "Globex"]);
    assert_eq!(all[0].value, json!(1));
    let found = relationship_choices(&db.label, &customer_rel(None), " Acme ", 50, &Scope::new()).unwrap();
    assert_eq!(labels(&found), ["Acme", "Acme West"]);
}

#[test]
fn filters_relationship_choices_with_the_form_scope_until_the_limit() {
    let db = customers();
    let scope = scope(json!({"form": {"region": "West"}, "parent": {}}));
    let rel = customer_rel(Some("record.region = form.region"));
    let west = relationship_choices(&db.label, &rel, "", 50, &scope).unwrap();
    assert_eq!(labels(&west), ["Acme West", "Globex"]);
    let one = relationship_choices(&db.label, &rel, "", 1, &scope).unwrap();
    assert_eq!(labels(&one), ["Acme West"]);
    let broken = customer_rel(Some("record.region ="));
    assert_eq!(relationship_choices(&db.label, &broken, "", 50, &Scope::new()).unwrap_err().code, "INVALID_FILTER");
}

#[test]
fn labels_a_relationship_key_and_keeps_the_last_label_for_revalidation() {
    let db = customers();
    let rel = customer_rel(None);
    assert_eq!(cached_relationship_label(&db.label, &rel, &json!(1)), None);
    assert_eq!(relationship_label(&db.label, &rel, &json!(1)), "Acme");
    assert_eq!(cached_relationship_label(&db.label, &rel, &json!(1)).as_deref(), Some("Acme"));
    // Empty keys have an empty label and never touch the backend.
    assert_eq!(relationship_label(&db.label, &rel, &Value::Null), "");
    assert_eq!(cached_relationship_label(&db.label, &rel, &json!("")).as_deref(), Some(""));
    // A key with no record shows the key; a composite key with none shows nothing.
    assert_eq!(relationship_label(&db.label, &rel, &json!(42)), "42");
    assert_eq!(relationship_label(&db.label, &rel, &json!({"id": 42})), "");
    // The cached label survives a record write, so a reloaded field shows it while revalidating.
    db.insert("customers", json!({"id": 4, "name": "Initech", "region": "East"}));
    assert_eq!(cached_relationship_label(&db.label, &rel, &json!(1)).as_deref(), Some("Acme"));
}
