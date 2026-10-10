//! Export streaming over an in-memory DuckDB: query exports, cancellation of
//! a failed export, and the temp-file-then-rename contract.
use super::*;

fn connection() -> duckdb::Connection {
    let c = duckdb::Connection::open_in_memory().unwrap();
    c.execute_batch(
        "CREATE TABLE t(id INTEGER, name VARCHAR); INSERT INTO t VALUES (2,'Bolt'),(1,'ACME, Inc.');",
    )
    .unwrap();
    c
}

fn dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("ixt-export-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn query_export_streams_rows_in_order() {
    let d = dir();
    let out = d.join("t.csv");
    let summary = run::query(
        &connection(),
        "w",
        None,
        "SELECT * FROM t ORDER BY id",
        &[],
        ExportFormat::Csv,
        &out,
    )
    .unwrap();
    assert_eq!(summary.rows, 2);
    let text = std::fs::read_to_string(&out).unwrap();
    assert_eq!(text, "\u{feff}id,name\r\n1,\"ACME, Inc.\"\r\n2,Bolt\r\n");
    // Only the destination remains: the temp file was renamed into place.
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
}

#[test]
fn failed_export_keeps_existing_file_and_cleans_up() {
    let d = dir();
    let out = d.join("t.json");
    std::fs::write(&out, "old").unwrap();
    let err = run::query(
        &connection(),
        "w",
        None,
        "SELECT * FROM missing",
        &[],
        ExportFormat::Json,
        &out,
    )
    .unwrap_err();
    assert_eq!(err.code, "DATABASE_ERROR");
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "old");
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
}

#[test]
fn missing_folder_is_reported() {
    let out = dir().join("nope").join("t.csv");
    let err = run::query(
        &connection(),
        "w",
        None,
        "SELECT 1",
        &[],
        ExportFormat::Csv,
        &out,
    )
    .unwrap_err();
    assert_eq!(err.code, "IO_ERROR");
}

#[test]
fn filtered_sql_applies_filters_and_sorts() {
    use crate::data::{DataValue, Filter, FilterOperator, Sort};
    let filters = vec![Filter {
        column: "id".into(),
        operator: FilterOperator::Gt,
        value: Some(DataValue::Integer(0)),
        values: None,
    }];
    let sorts = vec![Sort {
        column: "name".into(),
        descending: true,
    }];
    let (sql, values) =
        crate::queries::filtered_sql("SELECT * FROM t", &[], &[], &sorts, &filters).unwrap();
    let d = dir();
    let out = d.join("t.json");
    run::query(
        &connection(),
        "w",
        None,
        &sql,
        &values,
        ExportFormat::Json,
        &out,
    )
    .unwrap();
    let rows: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(rows[0]["name"], "Bolt");
    assert_eq!(rows[1]["id"], 1);
}
