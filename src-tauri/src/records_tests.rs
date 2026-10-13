use std::sync::{Arc, Mutex};

use serde_json::json;

use super::{
    delete, insert, register_hook, set_router, update, write_batch, RecordHook, RecordRouter,
    RecordWrite, RecordWriteMeta, WriteResult,
};
use crate::data::{DataValue, NamedValue};
use crate::manager::AppError;

fn session() -> String {
    let label = format!("records-{}", uuid::Uuid::new_v4());
    crate::manager().unwrap().new_session(&label).expect("session");
    let spec = json!({
        "name": "items",
        "columns": [
            {"name": "id", "declaredType": "INTEGER", "nullable": false, "primaryKeyPosition": 1,
             "unique": false, "defaultExpression": null, "generatedExpression": null},
            {"name": "name", "declaredType": "TEXT", "nullable": true, "primaryKeyPosition": 0,
             "unique": false, "defaultExpression": null, "generatedExpression": null},
        ],
        "foreignKeys": [], "checks": [], "withoutRowid": false,
    });
    crate::create_database_table(label.clone(), serde_json::from_value(spec).unwrap())
        .expect("table");
    label
}

fn nv(column: &str, value: DataValue) -> NamedValue {
    NamedValue {
        column: column.into(),
        value,
    }
}

fn expect(id: i64, name: Option<&str>) -> RecordWriteMeta {
    let name = name.map_or(DataValue::Null, |n| DataValue::Text(n.into()));
    RecordWriteMeta {
        expected: Some(vec![nv("id", DataValue::Integer(id)), nv("name", name)]),
        ..Default::default()
    }
}

fn names(label: &str) -> Vec<String> {
    let page = crate::read_table_page(label.into(), "items".into(), 0, 100, vec![], vec![], None)
        .expect("page");
    let json = serde_json::to_value(&page).unwrap();
    json["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row[1].to_string())
        .collect()
}

#[derive(Default)]
struct Recorder {
    seen: Mutex<Vec<(String, WriteResult)>>,
    fail_after: bool,
}

impl RecordHook for Recorder {
    fn before(&self, mut write: RecordWrite) -> Result<RecordWrite, AppError> {
        for v in &mut write.values {
            if let DataValue::Text(t) = &v.value {
                v.value = DataValue::Text(t.to_uppercase());
            }
        }
        Ok(write)
    }
    fn after(&self, write: &RecordWrite, result: &WriteResult, _: Option<&str>) -> Result<(), AppError> {
        assert!(!write.meta.write_id.is_empty());
        self.seen
            .lock()
            .unwrap()
            .push((write.table.clone(), result.clone()));
        if self.fail_after {
            return Err(AppError::new("TRIGGER", "after failed"));
        }
        Ok(())
    }
}

#[test]
fn hooks_replace_writes_and_see_results() {
    let label = session();
    let hook = Arc::new(Recorder::default());
    let reg = register_hook(&label, hook.clone());
    let id = insert(&label, "items", vec![nv("id", DataValue::Integer(1)), nv("name", DataValue::Text("a".into()))])
        .expect("insert");
    assert_eq!(id, vec![DataValue::Integer(1)]);
    let changed = update(
        &label,
        "items",
        vec![nv("name", DataValue::Text("b".into()))],
        vec![DataValue::Integer(1)],
        expect(1, Some("A")),
    )
    .expect("update");
    assert_eq!(changed, 1);
    assert!(names(&label).iter().any(|n| n.contains('B')));
    assert_eq!(hook.seen.lock().unwrap().len(), 2);
    drop(reg);
    delete(&label, "items", vec![DataValue::Integer(1)], expect(1, Some("B"))).expect("delete");
    assert_eq!(hook.seen.lock().unwrap().len(), 2, "unregistered hook stays silent");
}

#[test]
fn failing_after_hook_reports_a_committed_write() {
    let label = session();
    let _reg = register_hook(
        &label,
        Arc::new(Recorder {
            fail_after: true,
            ..Default::default()
        }),
    );
    let err = insert(&label, "items", vec![nv("id", DataValue::Integer(2))]).unwrap_err();
    assert!(err.committed);
    assert_eq!(err.results, vec![WriteResult::Inserted(vec![DataValue::Integer(2)])]);
    assert_eq!(names(&label).len(), 1);
}

struct Refuse;
impl RecordHook for Refuse {
    fn before(&self, _: RecordWrite) -> Result<RecordWrite, AppError> {
        Err(AppError::new("REJECTED", "no"))
    }
}

#[test]
fn failing_before_hook_aborts_batch() {
    let label = session();
    insert(&label, "items", vec![nv("id", DataValue::Integer(1))]).unwrap();
    let reg = register_hook(&label, Arc::new(Refuse));
    let err = write_batch(&label, vec![RecordWrite::insert("items", vec![nv("id", DataValue::Integer(3))])])
        .unwrap_err();
    assert_eq!(err.error.code, "REJECTED");
    assert!(!err.committed);
    drop(reg);
    let results = write_batch(
        &label,
        vec![
            RecordWrite::insert("items", vec![nv("id", DataValue::Integer(3))]),
            RecordWrite::delete("items", vec![DataValue::Integer(1)]).with_meta(expect(1, None)),
        ],
    )
    .expect("batch");
    assert_eq!(results[1], WriteResult::Changed(1));
    assert_eq!(names(&label).len(), 1);
}

struct Takeover;
impl RecordRouter for Takeover {
    fn route(&self, write: &RecordWrite) -> Option<Result<u64, AppError>> {
        (!write.meta.direct).then_some(Ok(42))
    }
}

#[test]
fn router_takes_over_updates_unless_direct() {
    let label = session();
    insert(&label, "items", vec![nv("id", DataValue::Integer(1))]).unwrap();
    let _reg = set_router(&label, Arc::new(Takeover));
    let routed = delete(&label, "items", vec![DataValue::Integer(1)], RecordWriteMeta::default()).unwrap();
    assert_eq!(routed, 42);
    assert_eq!(names(&label).len(), 1);
    let direct = RecordWriteMeta {
        direct: true,
        ..expect(1, None)
    };
    assert_eq!(delete(&label, "items", vec![DataValue::Integer(1)], direct).unwrap(), 1);
    assert!(names(&label).is_empty());
}
