//! Data mode flows (ports of tests/integration/table-workflow, shell (object browser),
//! datasheet-tools and optimistic-grid). Imports are explicit: `gpui_kit::*` would
//! shadow `#[test]`.

use crate::{
    data::DataValue,
    records::{register_hook, RecordHook, RecordWrite, WriteResult},
    manager::AppError,
    ui::testing::{shown, Harness},
};
use gpui_kit::TestAppContext;
use std::sync::{Arc, Mutex};

struct Log(Mutex<Vec<String>>);

impl RecordHook for Log {
    fn before(&self, write: RecordWrite) -> Result<RecordWrite, AppError> {
        self.0
            .lock()
            .unwrap()
            .push(format!("before {:?} {}", write.operation, write.table).to_lowercase());
        Ok(write)
    }
    fn after(&self, write: &RecordWrite, _: &WriteResult, _: Option<&str>) -> Result<(), AppError> {
        self.0
            .lock()
            .unwrap()
            .push(format!("after {:?} {}", write.operation, write.table).to_lowercase());
        Ok(())
    }
}

fn people(h: &mut Harness) {
    h.create_table("people", &[("id", "INTEGER", 1), ("name", "TEXT", 0), ("score", "REAL", 0)]);
    h.insert_row(
        "people",
        &[
            ("id", DataValue::Integer(2)),
            ("name", DataValue::Text("Beta".into())),
            ("score", DataValue::Real(2.5)),
        ],
    );
    h.insert_row(
        "people",
        &[
            ("id", DataValue::Integer(1)),
            ("name", DataValue::Text("Alpha".into())),
            ("score", DataValue::Null),
        ],
    );
}

// Shell test: "creates a table from the object browser and routes grid writes through
// record hooks".
#[gpui_kit::test]
fn creates_a_table_from_the_object_browser_and_routes_grid_writes_through_record_hooks(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    let log = Arc::new(Log(Mutex::new(vec![])));
    let _registration = register_hook(&h.label, log.clone());
    h.click("new-table");
    h.fill("Table name", "tasks");
    h.fill("Column 2:name", "title");
    h.click("submit-create-table");
    assert!(h.exists("table:tasks"), "the new table is listed");
    let selected = h.read_shell(|s, _| s.selection.clone()).expect("a selection");
    assert_eq!(selected.id, "tasks");
    let schema = crate::inspect_table(h.label.clone(), "tasks".into(), None).unwrap();
    let columns: Vec<(String, u32)> = schema
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.primary_key_position as u32))
        .collect();
    assert_eq!(columns, [("id".to_string(), 1), ("title".to_string(), 0)]);
    h.type_into("New id", "1");
    h.type_into("New title", "Write tests");
    h.press("enter");
    assert_eq!(
        *log.0.lock().unwrap(),
        ["before insert tasks", "after insert tasks"]
    );
    let page = h.read_page("tasks");
    assert_eq!(page.total, 1);
    assert_eq!(shown(&page.rows[0][1]), "Write tests");
}

// table-workflow: "reports invalid typed input without partially inserting a row".
#[gpui_kit::test]
fn reports_invalid_typed_input_without_partially_inserting_a_row(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.create_table("items", &[("id", "INTEGER", 1), ("label", "TEXT", 0)]);
    h.click("table:items");
    h.type_into("New id", "not-a-number");
    h.type_into("New label", "bad");
    h.click("insert-row");
    assert_eq!(h.label("draft-error"), "id requires an integer");
    assert_eq!(h.read_page("items").total, 0);
}

// table-workflow: "renders a typed table and persists sorted CRUD mutations" (the
// backend half; the UI renders the table once records are announced).
#[gpui_kit::test]
fn renders_a_typed_table_and_persists_sorted_crud_mutations(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    people(&mut h);
    h.click("table:people");
    assert_eq!(h.label("table-name"), "people");
    assert_eq!(h.label("record-count"), "2 records");
    assert_eq!(h.label("cell:0:1"), "Alpha");
    h.click("sort:name");
    h.click("sort:name");
    assert_eq!(h.label("cell:0:1"), "Beta");
    let page = crate::read_table_page(
        h.label.clone(),
        "people".into(),
        0,
        100,
        vec![crate::data::Sort { column: "name".into(), descending: false }],
        vec![],
        None,
    )
    .unwrap();
    assert_eq!(page.rows.iter().map(|r| shown(&r[1])).collect::<Vec<_>>(), ["Alpha", "Beta"]);
}
