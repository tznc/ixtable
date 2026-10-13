//! Committed writes (port of the form tests of tests/integration/runtime-committed-write
//! .test.tsx). A sync trigger that fails after the write is a record hook that fails; the
//! `runAction` rollback test belongs to Automation (the action runner is not part of the
//! Runtime). Imports are explicit.

use super::tests::wait;
use crate::data::DataValue;
use crate::manager::AppError;
use crate::records::{RecordHook, RecordOperation, RecordWrite, WriteResult, register_hook};
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;
use std::sync::Arc;

/// Fails after the `operation` writes to orders, like a sync trigger running a failing action.
struct FailAfter(RecordOperation);

impl RecordHook for FailAfter {
    fn after(&self, write: &RecordWrite, _: &WriteResult, _: Option<&str>) -> Result<(), AppError> {
        if write.table == "orders" && write.operation == self.0 {
            return Err(AppError::new("ACTION_FAILED", "audit is down"));
        }
        Ok(())
    }
}

fn orders(h: &mut Harness) {
    h.create_table("orders", &[("id", "INTEGER", 1), ("status", "TEXT", 0), ("note", "TEXT", 0)]);
    h.insert_row(
        "orders",
        &[
            ("id", DataValue::Integer(1)),
            ("status", DataValue::Text("open".into())),
            ("note", DataValue::Text("n".into())),
        ],
    );
    h.switch_mode(ModeId::Run);
    h.click("generate-app");
    wait(h);
}

/// The text of whichever message line the record view shows.
fn message(h: &mut Harness) -> String {
    ["status:error", "notice:error", "status:info", "notice:info"]
        .iter()
        .find_map(|id| h.try_label(*id))
        .unwrap_or_default()
}

#[gpui_kit::test]
fn opens_a_created_record_as_saved_when_a_sync_trigger_fails_after_the_insert(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    orders(&mut h);
    let _hook = register_hook(&h.label, Arc::new(FailAfter(RecordOperation::Insert)));
    h.click("new-record");
    wait(&mut h);
    h.fill("field:status", "fresh");
    h.click("create");
    wait(&mut h);
    let text = message(&mut h);
    assert!(text.starts_with("Saved.") && text.contains("audit is down"), "{text}");
    // The record opens as saved: no Create button, an Edit button.
    assert!(!h.exists("create"));
    assert!(h.exists("edit"));
    assert_eq!(h.read_page("orders").rows.len(), 2);
}

#[gpui_kit::test]
fn reloads_an_updated_record_when_a_sync_trigger_fails_after_the_update(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    orders(&mut h);
    let _hook = register_hook(&h.label, Arc::new(FailAfter(RecordOperation::Update)));
    h.click("row:0");
    wait(&mut h);
    h.click("edit");
    wait(&mut h);
    h.fill("field:status", "paid");
    h.click("save");
    wait(&mut h);
    let text = message(&mut h);
    assert!(text.starts_with("Saved.") && text.contains("audit is down"), "{text}");
    assert_eq!(h.read_page("orders").rows[0][1], DataValue::Text("paid".into()));

    // The reloaded record carries the saved values, so a second edit is not a conflict.
    h.click("edit");
    wait(&mut h);
    h.fill("field:note", "second");
    h.click("save");
    wait(&mut h);
    assert_eq!(h.read_page("orders").rows[0][2], DataValue::Text("second".into()));
    let text = message(&mut h);
    assert!(!text.contains("CONFLICT") && !text.contains("changed since"), "{text}");
}
