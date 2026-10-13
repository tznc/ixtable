//! Concurrent writes and reads (port of the first test of tests/integration/read-write-race
//! .test.tsx). The second test (work order details while the async audit job writes) needs
//! the job executor, which runs actions through the Automation runner.

use super::testkit::Db;
use crate::data::{DataValue, NamedValue};

fn count(label: &str) -> i64 {
    let params = vec![NamedValue { column: "p".into(), value: DataValue::Text("r%".into()) }];
    let run = tauri::async_runtime::block_on(crate::queries::execute_parameterized_query(
        label.to_string(),
        "SELECT count(*) FROM race WHERE note LIKE $p".into(),
        params,
        None,
        None,
        None,
    ))
    .expect("read");
    match run.rows[0][0] {
        DataValue::Integer(n) => n,
        ref other => panic!("unexpected count {other:?}"),
    }
}

#[test]
fn concurrent_writes_and_reads_never_fail_and_reads_see_committed_writes() {
    let db = Db::open();
    db.table("race", &[("id", "INTEGER", 1), ("note", "TEXT", 0)]);
    for round in 0..20i64 {
        let reads: Vec<_> = (0..3)
            .map(|_| {
                let label = db.label.clone();
                std::thread::spawn(move || count(&label))
            })
            .collect();
        db.insert("race", serde_json::json!({ "note": format!("r{round}-{}", "x".repeat(4000)) }));
        assert_eq!(count(&db.label), round + 1);
        for read in reads {
            assert!(read.join().expect("read thread") >= round);
        }
    }
}
