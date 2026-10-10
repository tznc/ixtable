use super::totals::{TotalFunction, TotalSpec};
use super::*;

fn reader() -> (ReadRuntime, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("ixtable-totals-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    rusqlite::Connection::open(dir.join("data.db"))
        .unwrap()
        .execute_batch(
            "CREATE TABLE t(id INTEGER PRIMARY KEY, region TEXT, qty INTEGER, price REAL, note BLOB);
             INSERT INTO t VALUES (1,'north',2,1.5,NULL),(2,'north',3,2.5,NULL),
                                  (3,'south',10,NULL,NULL),(4,'south',NULL,4.0,NULL);",
        )
        .unwrap();
    (ReadRuntime::for_test(&dir).unwrap(), dir)
}

fn spec(column: &str, function: TotalFunction) -> TotalSpec {
    TotalSpec {
        column: column.into(),
        function,
    }
}

#[test]
fn totals_aggregate_every_filtered_row() {
    let (reader, dir) = reader();
    let all = reader
        .totals(
            "t",
            &[],
            &[
                spec("qty", TotalFunction::Sum),
                spec("qty", TotalFunction::Count),
                spec("price", TotalFunction::Avg),
                spec("region", TotalFunction::Max),
                spec("qty", TotalFunction::Min),
            ],
        )
        .unwrap();
    assert_eq!(
        all,
        vec![
            DataValue::Integer(15),
            DataValue::Integer(3),
            DataValue::Real(8.0 / 3.0),
            DataValue::Text("south".into()),
            DataValue::Integer(2),
        ]
    );
    let north = Filter {
        column: "region".into(),
        operator: FilterOperator::Eq,
        value: Some(DataValue::Text("north".into())),
        values: None,
    };
    let filtered = reader
        .totals("t", &[north], &[spec("qty", TotalFunction::Sum)])
        .unwrap();
    assert_eq!(filtered, vec![DataValue::Integer(5)]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn totals_reject_numeric_aggregates_on_text_and_unknown_columns() {
    let (reader, dir) = reader();
    let text = reader
        .totals("t", &[], &[spec("region", TotalFunction::Sum)])
        .unwrap_err();
    assert!(text.contains("not available"), "{text}");
    let blob = reader
        .totals("t", &[], &[spec("note", TotalFunction::Max)])
        .unwrap_err();
    assert!(blob.contains("not available"), "{blob}");
    let unknown = reader
        .totals("t", &[], &[spec("missing", TotalFunction::Count)])
        .unwrap_err();
    assert!(unknown.contains("Unknown column"), "{unknown}");
    assert!(reader.totals("t", &[], &[]).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}
