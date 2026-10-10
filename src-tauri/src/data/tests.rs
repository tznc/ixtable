use super::*;
use std::sync::{Arc, Mutex};

#[test]
fn duckdb_reader_is_session_serializable_and_keeps_full_width_values() {
    // The session runtime is moved to another thread behind its mutex.
    let runtime = Arc::new(Mutex::new(ReadRuntime::isolated_for_test().unwrap()));
    let worker = Arc::clone(&runtime);
    let result = std::thread::spawn(move || {
        let guard = worker.lock().unwrap();
        guard
            .connection()
            .query_row(
                "SELECT CAST(9223372036854775807 AS BIGINT), CAST(-9223372036854775808 AS BIGINT), CAST('00FF' AS BLOB)",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, Vec<u8>>(2)?)),
            )
            .unwrap()
    })
    .join()
    .unwrap();
    assert_eq!(result.0, i64::MAX);
    assert_eq!(result.1, i64::MIN);
    assert_eq!(result.2, b"00FF");
}

#[test]
fn duckdb_reader_rejects_extension_autoload() {
    let runtime = ReadRuntime::isolated_for_test().unwrap();
    let error = runtime
        .connection()
        .execute_batch("LOAD definitely_not_an_installed_extension")
        .unwrap_err()
        .to_string();
    assert!(error.to_ascii_lowercase().contains("extension"));
}

#[test]
fn duckdb_values_convert_losslessly_to_canonical_forms() {
    let runtime = ReadRuntime::isolated_for_test().unwrap();
    let row: Vec<DataValue> = runtime
        .connection()
        .query_row(
            "SELECT CAST('12345678901234567890.12' AS DECIMAL(22,2)), DATE '2024-02-29', TIME '13:05:09.25', \
             TIMESTAMP '2024-01-31 13:05:09.000123', CAST(-0.5 AS DECIMAL(4,2)), [1,2], {'a': 1}, \
             CAST(18446744073709551615 AS UBIGINT), true, NULL",
            [],
            |r| Ok((0..10).map(|i| duck_value(r.get(i).unwrap())).collect()),
        )
        .unwrap();
    assert_eq!(
        row,
        vec![
            DataValue::Decimal("12345678901234567890.12".into()),
            DataValue::Date("2024-02-29".into()),
            DataValue::Time("13:05:09.25".into()),
            DataValue::Timestamp("2024-01-31T13:05:09.000123".into()),
            DataValue::Decimal("-0.50".into()),
            DataValue::Text("[1,2]".into()),
            DataValue::Text("{\"a\":1}".into()),
            DataValue::Decimal("18446744073709551615".into()),
            DataValue::Boolean(true),
            DataValue::Null,
        ]
    );
}

#[test]
fn read_only_guard_rejects_writes_and_scanner_functions() {
    for sql in [
        "DELETE FROM item",
        "SELECT * FROM read_csv('/tmp/x.csv')",
        "SELECT * FROM postgres_query('data', 'DELETE FROM t')",
        "SELECT * FROM sqlite_query('data', 'PRAGMA journal_mode=off')",
        "SELECT 1; SELECT 2",
    ] {
        assert!(read_only_guard(sql).is_err(), "{sql}");
    }
    assert!(read_only_guard("WITH x AS (SELECT 1) SELECT * FROM x").is_ok());
}

#[test]
fn read_only_guard_ignores_literals_identifiers_and_comments() {
    for sql in [
        "SELECT * FROM calls WHERE kind = 'Call'",
        "SELECT 'Delete request' AS label",
        "SELECT \"update\", \"set\" FROM t",
        "SELECT 'a;b' AS x",
        "SELECT 1;",
        "SELECT 1; -- trailing comment",
        "SELECT 1 /* drop table x; */",
        "SELECT read_count FROM t",
    ] {
        assert!(read_only_guard(sql).is_ok(), "{sql}");
    }
    assert_eq!(read_only_guard(" SELECT 1 ; ").unwrap(), "SELECT 1");
    for sql in [
        "SELECT 1; SELECT 2",
        "SELECT 1;;",
        ";",
        "SELECT * FROM read_csv_auto('/etc/passwd')",
        "SELECT * FROM read_json_objects('/etc/passwd')",
        "SELECT * FROM read_text ('/etc/passwd')",
        "SELECT * FROM glob('/etc/*')",
        "SELECT * FROM duckdb_databases()",
        "SELECT * FROM delta_scan('/x')",
        "SELECT 'x'; DELETE FROM t",
    ] {
        assert!(read_only_guard(sql).is_err(), "{sql}");
    }
}

/// A reader over a real data.db, used to prove that external access is off even
/// when the keyword guard is bypassed.
fn locked_reader() -> (ReadRuntime, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("ixtable-lock-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let c = rusqlite::Connection::open(dir.join("data.db")).unwrap();
    c.execute_batch(
        "CREATE TABLE t(id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1,'a');",
    )
    .unwrap();
    std::fs::write(dir.join("x.csv"), "a,b\n1,2\n").unwrap();
    (ReadRuntime::for_test(&dir).unwrap(), dir)
}

#[test]
fn reader_runs_without_external_access() {
    let (mut reader, dir) = locked_reader();
    let csv = dir.join("x.csv").to_string_lossy().replace('\'', "''");
    let out = dir.join("out.csv").to_string_lossy().replace('\'', "''");
    let other = dir
        .join("other.duckdb")
        .to_string_lossy()
        .replace('\'', "''");
    let attacks = [
        format!("SELECT * FROM read_csv_auto('{csv}')"),
        "SELECT * FROM read_text('/etc/passwd')".to_string(),
        format!("SELECT * FROM '{csv}'"),
        "SELECT * FROM '/etc/passwd'".to_string(),
        format!("COPY (SELECT 1) TO '{out}'"),
        format!("ATTACH '{other}' AS other"),
        "INSTALL httpfs".to_string(),
        "LOAD httpfs".to_string(),
        "SELECT * FROM glob('/etc/*')".to_string(),
        "SET enable_external_access = true".to_string(),
        "SET allowed_paths = ['/etc/passwd']".to_string(),
    ];
    for sql in &attacks {
        // The guard rejects it or DuckDB refuses it...
        assert!(reader.query(sql).is_err(), "guarded: {sql}");
        // ...and DuckDB refuses it even when the guard is bypassed.
        let direct = reader.connection().prepare(sql).and_then(|mut s| {
            let mut rows = s.query([])?;
            while rows.next()?.is_some() {}
            Ok(())
        });
        assert!(direct.is_err(), "direct: {sql}");
    }
    assert!(!dir.join("out.csv").exists());
    assert!(!dir.join("other.duckdb").exists());
    // PRAGMA is rejected by the guard (it reads no files, so DuckDB allows it).
    assert!(reader.query("PRAGMA database_list").is_err());
    // The datasource still reads, and refresh re-attaches it after a write.
    assert_eq!(reader.row_count("t").unwrap(), 1);
    rusqlite::Connection::open(dir.join("data.db"))
        .unwrap()
        .execute("INSERT INTO t VALUES (2,'b')", [])
        .unwrap();
    reader.refresh().unwrap();
    assert_eq!(reader.row_count("t").unwrap(), 2);
    assert_eq!(reader.table_def("t").unwrap().primary_key, vec!["id"]);
    // A cloned connection (saved-query runs) is locked too.
    let clone = reader.connection().try_clone().unwrap();
    clone.execute_batch("USE data").unwrap();
    assert!(clone
        .query_row(
            &format!("SELECT count(*) FROM read_csv_auto('{csv}')"),
            [],
            |r| r.get::<_, i64>(0)
        )
        .is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn like_patterns_escape_wildcards() {
    assert_eq!(like_pattern("50%", true), "%50\\%%");
    assert_eq!(like_pattern("a_b\\c", false), "a\\_b\\\\c%");
}

#[test]
fn table_page_search_is_case_insensitive_and_literal() {
    let workspace = std::env::temp_dir().join(format!("ixtable-search-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace).unwrap();
    rusqlite::Connection::open(workspace.join("data.db"))
        .unwrap()
        .execute_batch(
            "CREATE TABLE t(id INTEGER PRIMARY KEY, v TEXT);
             INSERT INTO t VALUES (1,'50% off'),(2,'500 units'),(3,'a_b'),(4,'axb'),(5,'ACME');",
        )
        .unwrap();
    let reader = ReadRuntime::for_test(&workspace).unwrap();
    let search = |text: &str| {
        let filters = [Filter {
            column: "v".into(),
            operator: FilterOperator::Contains,
            value: Some(DataValue::Text(text.into())),
            values: None,
        }];
        reader.page("t", 0, 10, &[], &filters).unwrap().total
    };
    assert_eq!(search("50%"), 1);
    assert_eq!(search("a_b"), 1);
    assert_eq!(search("acme"), 1);
}

#[test]
fn in_filters_match_any_listed_value_and_nothing_when_empty() {
    let dir = std::env::temp_dir().join(format!("ixtable-in-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let c = rusqlite::Connection::open(dir.join("data.db")).unwrap();
    c.execute_batch(
        "CREATE TABLE t(id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1,'a'),(2,'b'),(3,'c');",
    )
    .unwrap();
    let reader = ReadRuntime::for_test(&dir).unwrap();
    let filter = |values: Vec<DataValue>| Filter {
        column: "id".into(),
        operator: FilterOperator::In,
        value: None,
        values: Some(values),
    };
    let page = reader
        .page(
            "t",
            0,
            10,
            &[],
            &[filter(vec![DataValue::Integer(1), DataValue::Integer(3)])],
        )
        .unwrap();
    assert_eq!(page.total, 2);
    let none = reader.page("t", 0, 10, &[], &[filter(vec![])]).unwrap();
    assert_eq!(none.total, 0);
    let _ = std::fs::remove_dir_all(dir);
}
