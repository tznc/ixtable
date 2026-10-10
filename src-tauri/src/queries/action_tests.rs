use super::action::{guard, plan, ChangedRows, Plan};
use super::action_exec::{compute_update, direct, Job};
use crate::archive::{ActionKind, ActionSpec, SavedQuery};
use crate::data::{self, DataValue};

fn spec(kind: ActionKind, table: &str) -> ActionSpec {
    ActionSpec {
        kind,
        table: table.into(),
    }
}

#[test]
fn the_guard_accepts_one_statement_of_the_declared_kind_and_table() {
    let update = spec(ActionKind::Update, "Order Lines");
    for sql in [
        "UPDATE \"Order Lines\" SET qty = qty + 1 WHERE id = $1",
        "update data.main.\"Order Lines\" set qty = 0;",
        "WITH big AS (SELECT id FROM orders WHERE total > 100) UPDATE \"Order Lines\" SET qty = 0 FROM big WHERE big.id = \"Order Lines\".order_id",
    ] {
        assert!(guard(sql, &update, "main").is_ok(), "{sql}");
    }
    let insert = spec(ActionKind::Insert, "audit");
    assert!(guard("INSERT INTO audit BY NAME SELECT 1 AS id", &insert, "main").is_ok());
    assert!(guard("INSERT OR IGNORE INTO audit VALUES (1)", &insert, "main").is_ok());
    let delete = spec(ActionKind::Delete, "audit");
    assert_eq!(
        guard(
            "DELETE FROM audit WHERE note = 'drop table x; ok'; ",
            &delete,
            "main"
        )
        .unwrap(),
        "DELETE FROM audit WHERE note = 'drop table x; ok';".trim_end_matches(';')
    );
}

#[test]
fn the_guard_refuses_other_tables_kinds_ddl_and_files() {
    let update = spec(ActionKind::Update, "orders");
    let refused = [
        ("UPDATE customers SET x = 1", "must write to the table"),
        ("UPDATE memory.orders SET x = 1", "must write to the table"),
        ("DELETE FROM orders", "must be one UPDATE"),
        ("WITH x AS (SELECT 1) SELECT * FROM x", "must be one UPDATE"),
        (
            "UPDATE orders SET x = 1; DROP TABLE orders",
            "exactly one statement",
        ),
        (
            "UPDATE orders SET x = 1 RETURNING (SELECT 1 FROM (DROP TABLE y))",
            "cannot use DROP",
        ),
        (
            "UPDATE orders SET x = 1; UPDATE orders SET y = 2",
            "exactly one statement",
        ),
        (
            "UPDATE orders SET note = (SELECT content FROM read_text('/etc/passwd'))",
            "cannot use READ_TEXT",
        ),
        (
            "UPDATE orders SET note = (SELECT * FROM read_xlsx('x'))",
            "cannot read files",
        ),
        ("CREATE TABLE orders AS SELECT 1", "cannot use CREATE"),
        ("ATTACH 'x.db' AS y", "cannot use ATTACH"),
        (
            "UPDATE orders SET x = (SELECT 1 FROM sqlite_query('data', 'DELETE FROM customers RETURNING id'))",
            "cannot use SQLITE_QUERY",
        ),
    ];
    for (sql, message) in refused {
        let err = guard(sql, &update, "main").unwrap_err();
        assert!(err.contains(message), "{sql}: {err}");
    }
}

#[test]
fn replace_queries_delete_then_insert_by_name() {
    let query = SavedQuery {
        sql: "SELECT id, sum(total) AS total FROM orders WHERE placed >= $since GROUP BY id".into(),
        ..Default::default()
    };
    let (planned, names) =
        plan(&query, &spec(ActionKind::Replace, "totals"), "main", true).unwrap();
    assert_eq!(
        planned,
        Plan::Direct(vec![
            "DELETE FROM data.\"main\".\"totals\"".into(),
            "INSERT INTO data.\"main\".\"totals\" BY NAME (SELECT id, sum(total) AS total FROM orders WHERE placed >= $1 GROUP BY id)".into(),
        ])
    );
    assert_eq!(names, ["since"]);
}

#[test]
fn embedded_sqlite_updates_run_on_a_copy_under_the_table_name() {
    let update = |sql: &str| {
        let query = SavedQuery {
            sql: sql.into(),
            ..Default::default()
        };
        plan(
            &query,
            &spec(ActionKind::Update, "Order Lines"),
            "main",
            true,
        )
        .unwrap()
        .0
    };
    assert_eq!(
        update("UPDATE \"Order Lines\" SET qty = 0 WHERE \"Order Lines\".id = $id"),
        Plan::Copy("UPDATE temp.main.__ixtable_work AS \"Order Lines\" SET qty = 0 WHERE \"Order Lines\".id = $1".into())
    );
    assert_eq!(
        update("UPDATE data.main.\"Order Lines\" l SET qty = 0"),
        Plan::Copy("UPDATE temp.main.__ixtable_work l SET qty = 0".into())
    );
    let query = SavedQuery {
        sql: "UPDATE \"Order Lines\" SET qty = 0".into(),
        ..Default::default()
    };
    let postgres = plan(
        &query,
        &spec(ActionKind::Update, "Order Lines"),
        "public",
        false,
    )
    .unwrap()
    .0;
    assert_eq!(
        postgres,
        Plan::Direct(vec!["UPDATE \"Order Lines\" SET qty = 0".into()])
    );
}

/// A workspace with `data.db` holding customers and orders.
pub(crate) fn workspace() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ixtable-action-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let c = rusqlite::Connection::open(dir.join("data.db")).unwrap();
    c.execute_batch(
        "PRAGMA foreign_keys=ON;
         CREATE TABLE customers(id INTEGER PRIMARY KEY, name TEXT NOT NULL);
         CREATE TABLE orders(id INTEGER PRIMARY KEY, customer INTEGER NOT NULL REFERENCES customers(id),
           total REAL CHECK (total >= 0), paid INTEGER NOT NULL DEFAULT 0);
         INSERT INTO customers VALUES (1, 'Contoso'), (2, 'Fabrikam');
         INSERT INTO orders VALUES (1, 1, 10, 0), (2, 1, 20, 1), (3, 2, 30, 0);",
    )
    .unwrap();
    dir
}

pub(crate) fn writer(dir: &std::path::Path) -> duckdb::Connection {
    let extension = data::extensions::sqlite_extension_path().unwrap();
    data::write::open_writer(dir, &extension, &data::read::ReadTarget::Sqlite, false).unwrap()
}

pub(crate) fn rows(dir: &std::path::Path, sql: &str) -> Vec<Vec<i64>> {
    let c = rusqlite::Connection::open(dir.join("data.db")).unwrap();
    let mut stmt = c.prepare(sql).unwrap();
    let n = stmt.column_count();
    stmt.query_map([], |r| (0..n).map(|i| r.get::<_, i64>(i)).collect())
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

const ORDERS: &str = "data.\"main\".\"orders\"";

pub(crate) fn job(kind: ActionKind, watch: bool, dry_run: bool) -> Job {
    Job {
        table: ORDERS.into(),
        name: "orders".into(),
        keys: vec!["id".into()],
        rowid: true,
        kind,
        watch,
        dry_run,
        deleted: false,
        before: Default::default(),
        sqlite: true,
        logical: vec![],
        generated: vec![],
        defaulted: vec![],
    }
}

pub(crate) fn copy_sql(sql: &str, table: &str) -> String {
    let query = SavedQuery {
        sql: sql.into(),
        ..Default::default()
    };
    match plan(&query, &spec(ActionKind::Update, table), "main", true)
        .unwrap()
        .0
    {
        Plan::Copy(sql) => sql,
        other => panic!("{other:?}"),
    }
}

/// Writes computed changes the way `action::run` does: a text or typed writer, then `write_back`.
pub(crate) fn apply(
    db: &std::path::Path,
    job: &Job,
    changes: &super::action_exec::Changes,
) -> Result<u64, crate::manager::AppError> {
    use crate::data::logical::LogicalType;
    let c = rusqlite::Connection::open(db).unwrap();
    let mut stmt = c
        .prepare(&format!(
            "SELECT name, type FROM pragma_table_info('{}')",
            job.name
        ))
        .unwrap();
    let logical: Vec<(String, LogicalType)> = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .map(|r| {
            let (n, t) = r.unwrap();
            (n, LogicalType::from_sqlite_declared(&t))
        })
        .collect();
    let text = super::action_exec::needs_text(changes, &job.name)?;
    let extension = data::extensions::sqlite_extension_path().unwrap();
    let w = data::write::open_writer(
        db.parent().unwrap(),
        &extension,
        &data::read::ReadTarget::Sqlite,
        text,
    )
    .unwrap();
    super::action_exec::write_back(&w, job, changes, &logical, text)
}

/// Runs a computed update and applies it the way `action::run` does.
fn update(
    dir: &std::path::Path,
    sql: &str,
    values: &[duckdb::types::Value],
    job: &Job,
) -> super::action_exec::Outcome {
    let c = writer(dir);
    let computed = compute_update(&c, &copy_sql(sql, &job.name), values, job).unwrap();
    drop(c);
    let written = apply(&dir.join("data.db"), job, &computed.changes).unwrap();
    assert_eq!(written as usize, computed.changes.rows.len());
    computed.outcome
}

#[test]
fn updates_apply_only_changed_rows_and_report_their_old_values() {
    let dir = workspace();
    let values = [duckdb::types::Value::BigInt(1)];
    let outcome = update(
        &dir,
        "UPDATE orders SET paid = 1 WHERE customer = $c",
        &values,
        &job(ActionKind::Update, true, false),
    );
    // Order 2 was already paid: the statement matched it but changed nothing.
    assert_eq!(outcome.changed, 2);
    assert_eq!(outcome.rows.created, Vec::<Vec<DataValue>>::new());
    assert_eq!(outcome.rows.updated.len(), 1);
    assert_eq!(outcome.rows.updated[0].identity, [DataValue::Integer(1)]);
    let paid = outcome.rows.updated[0]
        .old
        .iter()
        .find(|v| v.column == "paid")
        .unwrap();
    assert_eq!(paid.value, DataValue::Integer(0));
    assert_eq!(
        rows(&dir, "SELECT id, paid FROM orders ORDER BY id"),
        [[1, 1], [2, 1], [3, 0]]
    );
}

#[test]
fn updates_of_dates_keys_and_tables_without_keys_work() {
    let dir = workspace();
    let c = rusqlite::Connection::open(dir.join("data.db")).unwrap();
    c.execute_batch(
        "CREATE TABLE visits(id INTEGER PRIMARY KEY, day DATE CHECK (day IS NULL OR date(day) IS day), seen TIMESTAMP);
         INSERT INTO visits VALUES (1, '2024-01-31', '2024-01-31T09:00:00');
         CREATE TABLE notes(body TEXT, day DATE);
         INSERT INTO notes VALUES ('a', '2024-02-01'), ('b', '2024-02-02');",
    )
    .unwrap();
    drop(c);
    let visits = Job {
        table: "data.\"main\".\"visits\"".into(),
        name: "visits".into(),
        ..job(ActionKind::Update, true, false)
    };
    let out = update(
        &dir,
        "UPDATE visits SET day = day + 1, seen = seen + INTERVAL 2 HOUR, id = id + 10",
        &[],
        &visits,
    );
    assert_eq!(out.rows.updated[0].identity, [DataValue::Integer(11)]);
    let c = rusqlite::Connection::open(dir.join("data.db")).unwrap();
    let (id, day, at): (i64, String, String) = c
        .query_row("SELECT id, day, seen FROM visits", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!((id, day.as_str()), (11, "2024-02-01"));
    assert!(
        at.starts_with("2024-01-31") && at.contains("11:00:00"),
        "{at}"
    );
    drop(c);
    let notes = Job {
        table: "data.\"main\".\"notes\"".into(),
        name: "notes".into(),
        keys: vec![],
        ..job(ActionKind::Update, false, false)
    };
    let out = update(
        &dir,
        "UPDATE notes n SET day = n.day + 7 WHERE n.body = 'b'",
        &[],
        &notes,
    );
    assert_eq!(out.changed, 1);
    let c = rusqlite::Connection::open(dir.join("data.db")).unwrap();
    let days: Vec<String> = c
        .prepare("SELECT day FROM notes ORDER BY body")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(days, ["2024-02-01", "2024-02-09"]);
}

#[test]
fn inserts_report_new_keys_and_replace_reports_every_row() {
    let dir = workspace();
    let c = writer(&dir);
    let insert = vec!["INSERT INTO orders (id, customer, total) SELECT id + 10, customer, total FROM orders WHERE paid = 0".to_string()];
    let out = direct(&c, &insert, &[], &job(ActionKind::Insert, true, false)).unwrap();
    assert_eq!(out.changed, 2);
    let mut created = out.rows.created.clone();
    created.sort_by_key(|k| format!("{k:?}"));
    assert_eq!(
        created,
        [[DataValue::Integer(11)], [DataValue::Integer(13)]]
    );
    let replace = vec![
        format!("DELETE FROM {ORDERS}"),
        format!("INSERT INTO {ORDERS} BY NAME (SELECT 1 AS id, 2 AS customer, 5.0 AS total)"),
    ];
    let out = direct(&c, &replace, &[], &job(ActionKind::Replace, true, false)).unwrap();
    assert_eq!((out.changed, out.removed), (1, 5));
    assert_eq!(out.rows.created, [[DataValue::Integer(1)]]);
    drop(c);
    assert_eq!(
        rows(&dir, "SELECT id, customer, paid FROM orders"),
        [[1, 2, 0]]
    );
}

#[test]
fn foreign_keys_cascade_and_the_journal_mode_is_kept() {
    let dir = workspace();
    rusqlite::Connection::open(dir.join("data.db"))
        .unwrap()
        .execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE notes(id INTEGER PRIMARY KEY, customer INTEGER REFERENCES customers(id) ON DELETE CASCADE);
             INSERT INTO notes VALUES (1, 1), (2, 2);",
        )
        .unwrap();
    for text in [false, true] {
        let extension = data::extensions::sqlite_extension_path().unwrap();
        let c = data::write::open_writer(&dir, &extension, &data::read::ReadTarget::Sqlite, text)
            .unwrap();
        let err = c
            .execute_batch("INSERT INTO notes VALUES (9, 99)")
            .unwrap_err();
        assert!(err.to_string().contains("FOREIGN KEY"), "{err}");
    }
    let c = writer(&dir);
    let delete = vec!["DELETE FROM orders WHERE customer = 2".to_string()];
    direct(&c, &delete, &[], &job(ActionKind::Delete, false, false)).unwrap();
    let delete = vec!["DELETE FROM customers WHERE id = 2".to_string()];
    direct(&c, &delete, &[], &job(ActionKind::Delete, false, false)).unwrap();
    drop(c);
    assert_eq!(rows(&dir, "SELECT id, customer FROM notes"), [[1, 1]]);
    let c = rusqlite::Connection::open(dir.join("data.db")).unwrap();
    let mode: String = c
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
}

#[test]
fn dry_runs_and_failures_leave_the_data_unchanged() {
    let dir = workspace();
    let c = writer(&dir);
    let delete = vec!["DELETE FROM orders WHERE paid = 0".to_string()];
    let out = direct(&c, &delete, &[], &job(ActionKind::Delete, false, true)).unwrap();
    assert_eq!((out.changed, out.rows), (2, ChangedRows::default()));
    let orphan = vec!["INSERT INTO orders (id, customer, total) VALUES (9, 99, 1)".to_string()];
    let err = direct(&c, &orphan, &[], &job(ActionKind::Insert, false, false)).unwrap_err();
    assert!(err.message.contains("FOREIGN KEY"), "{}", err.message);
    let negative = copy_sql("UPDATE orders SET total = total - 15", "orders");
    let computed =
        compute_update(&c, &negative, &[], &job(ActionKind::Update, false, false)).unwrap();
    // The writer is locked down like the reader.
    assert!(c
        .execute_batch("SET enable_external_access = true")
        .is_err());
    drop(c);
    let err = apply(
        &dir.join("data.db"),
        &job(ActionKind::Update, false, false),
        &computed.changes,
    )
    .unwrap_err();
    assert_eq!(err.code, "CONSTRAINT", "{}", err.message);
    assert_eq!(
        rows(
            &dir,
            "SELECT count(*), CAST(sum(total) AS INTEGER) FROM orders"
        ),
        [[3, 60]]
    );
}

#[test]
#[ignore = "needs IXTABLE_TEST_POSTGRES_URL; CI runs it with --include-ignored"]
fn postgres_action_queries_write_through_the_attachment() {
    let Some(url) = crate::test_env::postgres_url() else {
        return;
    };
    let schema = format!("ixt_{}", uuid::Uuid::new_v4().simple());
    let mut admin = ::postgres::Client::connect(&url, ::postgres::NoTls).unwrap();
    admin
        .batch_execute(&format!(
            "CREATE SCHEMA {schema};
             CREATE TABLE {schema}.visits(id BIGINT PRIMARY KEY, day DATE NOT NULL, seen TIMESTAMP, paid BOOLEAN NOT NULL DEFAULT false, total NUMERIC(10,2) CHECK (total >= 0));
             INSERT INTO {schema}.visits VALUES (1, '2024-01-31', '2024-01-31 09:00', false, 10), (2, '2024-02-01', NULL, true, 20);"
        ))
        .unwrap();
    let dir = std::env::temp_dir().join(format!("ixtable-action-pg-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let extension = data::extensions::sqlite_extension_path().unwrap();
    let target = data::read::ReadTarget::Postgres {
        conninfo: url.clone(),
        schema: schema.clone(),
    };
    let c = data::write::open_writer(&dir, &extension, &target, false).unwrap();
    let job = |kind, dry_run| Job {
        table: format!("data.\"{schema}\".\"visits\""),
        name: "visits".into(),
        keys: vec!["id".into()],
        rowid: false,
        kind,
        watch: true,
        dry_run,
        deleted: false,
        before: Default::default(),
        sqlite: false,
        logical: vec![],
        generated: vec![],
        defaulted: vec![],
    };
    let query = SavedQuery {
        sql: "UPDATE visits SET day = day + 1, seen = coalesce(seen, TIMESTAMP '2024-03-01 08:00'), paid = true WHERE total < $max".into(),
        ..Default::default()
    };
    let Plan::Direct(sql) = plan(&query, &spec(ActionKind::Update, "visits"), &schema, false)
        .unwrap()
        .0
    else {
        panic!("PostgreSQL updates run directly");
    };
    let out = direct(
        &c,
        &sql,
        &[duckdb::types::Value::Double(15.0)],
        &job(ActionKind::Update, false),
    )
    .unwrap();
    assert_eq!(out.changed, 1);
    assert_eq!(out.rows.updated.len(), 1);
    assert_eq!(out.rows.updated[0].identity, [DataValue::Integer(1)]);
    let negative = vec!["UPDATE visits SET total = -1".to_string()];
    assert!(direct(&c, &negative, &[], &job(ActionKind::Update, false)).is_err());
    let dry = vec!["DELETE FROM visits".to_string()];
    assert_eq!(
        direct(&c, &dry, &[], &job(ActionKind::Delete, true))
            .unwrap()
            .changed,
        2
    );
    drop(c);
    let row = admin
        .query_one(&format!("SELECT day::text, seen::text, paid, (SELECT count(*) FROM {schema}.visits) FROM {schema}.visits WHERE id = 1"), &[])
        .unwrap();
    let (day, seen, paid, count): (String, String, bool, i64) =
        (row.get(0), row.get(1), row.get(2), row.get(3));
    assert_eq!(
        (day.as_str(), seen.as_str(), paid, count),
        ("2024-02-01", "2024-01-31 09:00:00", true, 2)
    );
    admin
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
        .unwrap();
}

#[test]
fn validation_checks_action_sql_targets_and_readers() {
    let mut config: crate::archive::DocumentConfig = serde_json::from_value(serde_json::json!({
        "version": crate::archive::CONFIG_VERSION, "name": "Doc", "activeMode": "data",
        "savedQueries": [
            {"id": "q-ok", "name": "Mark paid", "sql": "UPDATE orders SET paid = true WHERE id = $id",
             "parameters": [{"name": "id", "logicalType": "integer"}],
             "action": {"kind": "update", "table": "orders"}},
            {"id": "q-wrong", "name": "Wrong table", "sql": "DELETE FROM customers",
             "action": {"kind": "delete", "table": "orders"}},
            {"id": "q-none", "name": "No table", "sql": "DELETE FROM orders",
             "action": {"kind": "delete", "table": ""}}
        ]
    }))
    .unwrap();
    config.design.forms = serde_json::from_value(serde_json::json!([
        {"id": "f1", "name": "Paid", "modes": ["list"], "controls": [],
         "source": {"kind": "query", "queryId": "q-ok"}}
    ]))
    .unwrap();
    let text: Vec<String> = super::validate(&config)
        .into_iter()
        .map(|i| format!("{} {}", i.object_id, i.message))
        .collect();
    let has = |id: &str, part: &str| text.iter().any(|t| t.starts_with(id) && t.contains(part));
    assert!(
        has("q-ok", "Form \"Paid\" reads \"Mark paid\""),
        "{text:#?}"
    );
    assert!(
        has("q-wrong", "must write to the table \"orders\""),
        "{text:#?}"
    );
    assert!(has("q-none", "needs a target table"), "{text:#?}");
    assert!(!text
        .iter()
        .any(|t| t.starts_with("q-ok") && t.contains("declares no parameter")));
}

#[test]
fn inserts_fill_current_date_and_time_defaults_without_icu() {
    let dir = workspace();
    let c = rusqlite::Connection::open(dir.join("data.db")).unwrap();
    c.execute_batch(
        "CREATE TABLE notes(id INTEGER PRIMARY KEY, day DATE DEFAULT CURRENT_DATE, at_time TIME TEXT DEFAULT CURRENT_TIME, stamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP);",
    )
    .unwrap();
    drop(c);
    let job = Job {
        table: "data.\"main\".\"notes\"".into(),
        name: "notes".into(),
        ..job(ActionKind::Insert, false, false)
    };
    let insert = vec!["INSERT INTO notes (id) VALUES (1)".to_string()];
    assert_eq!(
        direct(&writer(&dir), &insert, &[], &job).unwrap().changed,
        1
    );
    let c = rusqlite::Connection::open(dir.join("data.db")).unwrap();
    let (day, time, stamp): (String, String, String) = c
        .query_row("SELECT day, at_time, stamp FROM notes", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    let today: String = c
        .query_row("SELECT CURRENT_DATE", [], |r| r.get(0))
        .unwrap();
    assert_eq!(day, today);
    assert_eq!(time.len(), 8, "{time}");
    assert!(stamp.starts_with(&today), "{stamp}");
}

#[test]
fn binary_updates_use_the_typed_attachment_and_cannot_mix_with_dates() {
    let dir = workspace();
    let c = rusqlite::Connection::open(dir.join("data.db")).unwrap();
    c.execute_batch(
        "CREATE TABLE files(id INTEGER PRIMARY KEY, body BLOB, day DATE, flag BOOLEAN CHECK (flag IS NULL OR flag IN (0,1)));
         INSERT INTO files VALUES (1, x'0102', '2024-01-01', 0);",
    )
    .unwrap();
    drop(c);
    let files = Job {
        table: "data.\"main\".\"files\"".into(),
        name: "files".into(),
        ..job(ActionKind::Update, false, false)
    };
    update(
        &dir,
        "UPDATE files SET body = from_hex('0a0b'), flag = true",
        &[],
        &files,
    );
    let c = rusqlite::Connection::open(dir.join("data.db")).unwrap();
    let (body, flag): (Vec<u8>, i64) = c
        .query_row("SELECT body, flag FROM files", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!((body, flag), (vec![0x0a, 0x0b], 1));
    drop(c);
    let w = writer(&dir);
    let both = copy_sql(
        "UPDATE files SET body = from_hex('0c'), day = day + 1",
        "files",
    );
    let computed = compute_update(&w, &both, &[], &files).unwrap();
    drop(w);
    let err = apply(&dir.join("data.db"), &files, &computed.changes).unwrap_err();
    assert!(
        err.message.contains("split it into two queries"),
        "{}",
        err.message
    );
}
