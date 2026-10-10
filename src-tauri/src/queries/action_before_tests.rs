//! Deleted rows and before-change triggers on action queries, on a real SQLite file.
use super::action::{BeforeChange, Override, Pending};
use super::action_exec::{compute_update, direct, Before, Job};
use super::action_tests::{apply, copy_sql, job, rows, workspace, writer};
use crate::archive::ActionKind;
use crate::data::logical::LogicalType;
use crate::data::{DataValue, NamedValue};

fn shipments(dir: &std::path::Path) {
    rusqlite::Connection::open(dir.join("data.db"))
        .unwrap()
        .execute_batch(
            "CREATE TABLE shipments(id INTEGER PRIMARY KEY, order_id INTEGER, shipped DATE, note TEXT,
               label TEXT GENERATED ALWAYS AS ('#' || id) VIRTUAL);
             INSERT INTO shipments (id, order_id, note) VALUES (1, 2, 'old');",
        )
        .unwrap();
}

fn shipments_job(kind: ActionKind, before: Before) -> Job {
    let declared = [
        ("id", "INTEGER"),
        ("order_id", "INTEGER"),
        ("shipped", "DATE"),
        ("note", "TEXT"),
    ];
    Job {
        table: "data.\"main\".\"shipments\"".into(),
        name: "shipments".into(),
        before,
        logical: declared
            .iter()
            .map(|(n, t)| (n.to_string(), LogicalType::from_sqlite_declared(t)))
            .collect(),
        generated: vec!["label".into()],
        ..job(kind, false, false)
    }
}

fn set(row: usize, values: &[(&str, DataValue)]) -> Override {
    Override {
        row,
        values: values
            .iter()
            .map(|(c, v)| NamedValue {
                column: c.to_string(),
                value: v.clone(),
            })
            .collect(),
    }
}

fn shipped(dir: &std::path::Path) -> Vec<(i64, Option<String>, Option<String>)> {
    let c = rusqlite::Connection::open(dir.join("data.db")).unwrap();
    let mut stmt = c
        .prepare("SELECT id, shipped, note FROM shipments ORDER BY id")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

const INSERT: &str = "INSERT INTO data.main.shipments (id, order_id) SELECT id + 100, id FROM data.main.orders WHERE paid = 0";

fn collect(dir: &std::path::Path) -> Pending {
    let c = writer(dir);
    let mut collecting = shipments_job(ActionKind::Insert, Before::Collect);
    collecting.dry_run = true;
    direct(&c, &[INSERT.into()], &[], &collecting)
        .unwrap()
        .pending
        .unwrap()
}

#[test]
fn deletes_and_replaces_report_the_rows_they_removed() {
    let dir = workspace();
    let c = writer(&dir);
    let deleting = Job {
        deleted: true,
        ..job(ActionKind::Delete, false, false)
    };
    let out = direct(
        &c,
        &["DELETE FROM data.main.orders WHERE paid = 0".into()],
        &[],
        &deleting,
    )
    .unwrap();
    let ids: Vec<_> = out
        .rows
        .deleted
        .iter()
        .map(|r| r.identity.clone())
        .collect();
    assert_eq!(ids, [[DataValue::Integer(1)], [DataValue::Integer(3)]]);
    let total = out.rows.deleted[1]
        .values
        .iter()
        .find(|v| v.column == "total")
        .unwrap();
    assert_eq!(total.value, DataValue::Real(30.0));
    let replace = vec![
        "DELETE FROM data.main.orders".to_string(),
        "INSERT INTO data.main.orders BY NAME (SELECT 7 AS id, 1 AS customer)".to_string(),
    ];
    let replacing = Job {
        kind: ActionKind::Replace,
        ..deleting
    };
    let out = direct(&c, &replace, &[], &replacing).unwrap();
    assert_eq!(out.rows.deleted.len(), 1);
    assert_eq!(out.rows.deleted[0].identity, [DataValue::Integer(2)]);
    drop(c);
    assert_eq!(rows(&dir, "SELECT id FROM orders"), [[7]]);
}

#[test]
fn an_insert_collects_pending_rows_then_applies_the_fields_triggers_set() {
    let dir = workspace();
    shipments(&dir);
    let pending = collect(&dir);
    // The first call rolls back and returns every new row in key order.
    assert_eq!(shipped(&dir).len(), 1);
    let ids: Vec<_> = pending.rows.iter().map(|r| r.identity.clone()).collect();
    assert_eq!(ids, [[DataValue::Integer(101)], [DataValue::Integer(103)]]);
    assert!(pending.rows.iter().all(|r| r.old.is_none()));
    assert!(pending.rows[0]
        .values
        .iter()
        .any(|v| v.column == "order_id" && v.value == DataValue::Integer(1)));
    let decided = BeforeChange {
        fingerprint: pending.fingerprint.clone(),
        overrides: vec![
            set(0, &[("shipped", DataValue::Text("2024-05-01".into()))]),
            set(0, &[("note", DataValue::Text("rush".into()))]),
        ],
    };
    let c = writer(&dir);
    let out = direct(
        &c,
        &[INSERT.into()],
        &[],
        &shipments_job(ActionKind::Insert, Before::Apply(decided)),
    )
    .unwrap();
    assert_eq!(out.changed, 2);
    drop(c);
    assert_eq!(
        shipped(&dir),
        [
            (1, None, Some("old".into())),
            (101, Some("2024-05-01".into()), Some("rush".into())),
            (103, None, None),
        ]
    );
}

#[test]
fn a_changed_result_keys_and_unknown_rows_are_refused_with_nothing_written() {
    let dir = workspace();
    shipments(&dir);
    let pending = collect(&dir);
    let run = |overrides: Vec<Override>, fingerprint: &str| {
        let decided = BeforeChange {
            fingerprint: fingerprint.into(),
            overrides,
        };
        let c = writer(&dir);
        direct(
            &c,
            &[INSERT.into()],
            &[],
            &shipments_job(ActionKind::Insert, Before::Apply(decided)),
        )
        .map(|_| ())
        .unwrap_err()
    };
    assert_eq!(run(vec![], "stale").code, "CONFLICT");
    let key = run(
        vec![set(0, &[("id", DataValue::Integer(5))])],
        &pending.fingerprint,
    );
    assert!(key.message.contains("key column id"), "{}", key.message);
    let other = run(
        vec![set(5, &[("note", DataValue::Text("x".into()))])],
        &pending.fingerprint,
    );
    assert!(
        other.message.contains("does not change"),
        "{}",
        other.message
    );
    let bad = run(
        vec![set(0, &[("shipped", DataValue::Text("soon".into()))])],
        &pending.fingerprint,
    );
    assert_eq!(bad.code, "VALIDATION_ERROR");
    assert_eq!(shipped(&dir).len(), 1);
}

#[test]
fn an_embedded_update_applies_trigger_fields_to_its_copy() {
    let dir = workspace();
    shipments(&dir);
    rusqlite::Connection::open(dir.join("data.db"))
        .unwrap()
        .execute_batch("INSERT INTO shipments (id, order_id, note) VALUES (2, 3, 'old')")
        .unwrap();
    let sql = copy_sql("UPDATE shipments SET note = 'packed'", "shipments");
    let c = writer(&dir);
    let mut collecting = shipments_job(ActionKind::Update, Before::Collect);
    collecting.watch = true;
    let computed = compute_update(&c, &sql, &[], &collecting).unwrap();
    let pending = computed.outcome.pending.unwrap();
    assert_eq!(pending.rows.len(), 2);
    let old = pending.rows[0].old.as_ref().unwrap();
    assert!(old
        .iter()
        .any(|v| v.column == "note" && v.value == DataValue::Text("old".into())));
    let decided = BeforeChange {
        fingerprint: pending.fingerprint,
        overrides: vec![set(1, &[("shipped", DataValue::Text("2024-06-02".into()))])],
    };
    let mut applying = shipments_job(ActionKind::Update, Before::Apply(decided));
    applying.watch = true;
    let computed = compute_update(&c, &sql, &[], &applying).unwrap();
    drop(c);
    assert_eq!(computed.outcome.rows.updated.len(), 2);
    apply(&dir.join("data.db"), &applying, &computed.changes).unwrap();
    assert_eq!(
        shipped(&dir),
        [
            (1, None, Some("packed".into())),
            (2, Some("2024-06-02".into()), Some("packed".into())),
        ]
    );
}

#[test]
#[ignore = "needs IXTABLE_TEST_POSTGRES_URL; CI runs it with --include-ignored"]
fn postgres_trigger_fields_and_deleted_rows_go_through_the_attachment() {
    let Some(url) = crate::test_env::postgres_url() else {
        return;
    };
    let schema = format!("ixt_{}", uuid::Uuid::new_v4().simple());
    let mut admin = ::postgres::Client::connect(&url, ::postgres::NoTls).unwrap();
    admin
        .batch_execute(&format!(
            "CREATE SCHEMA {schema};
             CREATE TABLE {schema}.tasks(id BIGINT PRIMARY KEY, done BOOLEAN NOT NULL DEFAULT false, due DATE,
               seq BIGINT GENERATED ALWAYS AS IDENTITY);
             INSERT INTO {schema}.tasks (id, done) VALUES (1, false), (2, true);"
        ))
        .unwrap();
    let dir = std::env::temp_dir().join(format!("ixtable-action-pg-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let extension = crate::data::extensions::sqlite_extension_path().unwrap();
    let target = crate::data::read::ReadTarget::Postgres {
        conninfo: url.clone(),
        schema: schema.clone(),
    };
    let c = crate::data::write::open_writer(&dir, &extension, &target, false).unwrap();
    let tasks = |kind, before| Job {
        table: format!("data.\"{schema}\".\"tasks\""),
        name: "tasks".into(),
        rowid: false,
        sqlite: false,
        before,
        logical: vec![("due".into(), LogicalType::Date)],
        generated: vec![],
        defaulted: vec!["done".into(), "seq".into()],
        ..job(kind, false, false)
    };
    let insert = vec!["INSERT INTO tasks (id) VALUES (3), (4)".to_string()];
    let mut collecting = tasks(ActionKind::Insert, Before::Collect);
    collecting.dry_run = true;
    let pending = direct(&c, &insert, &[], &collecting)
        .unwrap()
        .pending
        .unwrap();
    assert_eq!(pending.rows.len(), 2);
    let decided = BeforeChange {
        fingerprint: pending.fingerprint,
        overrides: vec![set(1, &[("due", DataValue::Text("2024-07-01".into()))])],
    };
    direct(
        &c,
        &insert,
        &[],
        &tasks(ActionKind::Insert, Before::Apply(decided)),
    )
    .unwrap();
    let update = vec!["UPDATE tasks SET done = true WHERE id = 1".to_string()];
    let mut collecting = tasks(ActionKind::Update, Before::Collect);
    collecting.dry_run = true;
    let pending = direct(&c, &update, &[], &collecting)
        .unwrap()
        .pending
        .unwrap();
    assert!(pending.rows[0].old.is_some());
    let decided = BeforeChange {
        fingerprint: pending.fingerprint,
        overrides: vec![set(0, &[("due", DataValue::Text("2024-07-02".into()))])],
    };
    direct(
        &c,
        &update,
        &[],
        &tasks(ActionKind::Update, Before::Apply(decided)),
    )
    .unwrap();
    let deleting = Job {
        deleted: true,
        ..tasks(ActionKind::Delete, Before::Off)
    };
    let delete = vec!["DELETE FROM tasks WHERE id = 2".to_string()];
    let out = direct(&c, &delete, &[], &deleting).unwrap();
    assert_eq!(out.rows.deleted.len(), 1);
    assert_eq!(out.rows.deleted[0].identity, [DataValue::Integer(2)]);
    drop(c);
    let due: Vec<(i64, Option<String>)> = admin
        .query(
            &format!("SELECT id, due::text FROM {schema}.tasks ORDER BY id"),
            &[],
        )
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    assert_eq!(
        due,
        [
            (1, Some("2024-07-02".into())),
            (3, None),
            (4, Some("2024-07-01".into()))
        ]
    );
    admin
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
        .unwrap();
}
