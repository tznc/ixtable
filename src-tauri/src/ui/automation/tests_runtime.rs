//! Port of `tests/integration/automation-runtime.test.tsx`: triggers that navigate in the
//! Runtime, and the custom concurrency action. The React test chose the concurrency policy in
//! the Entities tab; here the definitions are written through the backend (the tab itself is
//! covered by `data::tests_settings`). Imports are explicit.

use crate::data::{DataValue, NamedValue};
use crate::records::{self, RecordWriteMeta};
use crate::ui::shell::ModeId;
use crate::ui::testing::{Harness, shown};
use gpui_kit::TestAppContext;
use serde_json::json;
use std::time::Duration;

fn text(value: &str) -> DataValue {
    DataValue::Text(value.into())
}

fn named(column: &str, value: DataValue) -> NamedValue {
    NamedValue { column: column.into(), value }
}

fn setup(h: &mut Harness, extra: impl FnOnce(&mut crate::archive::DocumentConfig)) {
    h.create_table("orders", &[("id", "INTEGER", 1), ("status", "TEXT", 0)]);
    h.create_table("audit", &[("id", "INTEGER", 1), ("message", "TEXT", 0)]);
    h.insert_row("orders", &[("id", DataValue::Integer(1)), ("status", text("open"))]);
    let mut config = h.config();
    extra(&mut config);
    crate::update_document_config(h.label.clone(), config).expect("update config");
    // The shell picks the backend edit up on its next store update.
    h.switch_mode(ModeId::App);
    h.switch_mode(ModeId::Run);
    // Pages for both tables, so a navigate step has somewhere to go.
    h.click("generate-app");
    wait(h);
}

fn wait(h: &mut Harness) {
    for _ in 0..16 {
        h.advance(Duration::from_millis(20));
        std::thread::sleep(Duration::from_millis(5));
        h.settle();
    }
}

fn column(h: &Harness, table: &str, index: usize) -> Vec<String> {
    h.read_page(table).rows.iter().map(|r| shown(&r[index])).collect()
}

#[gpui_kit::test]
fn follows_navigate_and_set_state_steps_from_a_sync_trigger_in_the_runtime(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    setup(&mut h, |config| {
        config.actions = serde_json::from_value(json!([{
            "id": "go", "name": "Show audit", "onError": "stop",
            "steps": [
                {"id": "s1", "kind": "setState", "scope": "app", "key": "lastOrder", "value": "record.id"},
                {"id": "s2", "kind": "createRecord", "table": "audit",
                 "values": {"message": "'order ' & app.lastOrder & ' by ' & app.user.name"}},
                {"id": "s3", "kind": "navigate", "target": {"kind": "table", "id": "audit"}},
            ],
        }]))
        .expect("actions");
        config.triggers = serde_json::from_value(json!([{
            "id": "t1", "name": "Open audit", "table": "orders", "event": "created", "actionId": "go",
            "mode": "sync", "enabled": true, "maxAttempts": 1, "backoffMs": 0,
        }]))
        .expect("triggers");
    });
    wait(&mut h);
    records::insert(&h.label, "orders", vec![named("status", text("new"))]).expect("insert");
    h.records_changed();
    wait(&mut h);
    // The steps ran: setState fed the createRecord expression, and the row was written.
    assert_eq!(column(&h, "audit", 1).len(), 1);
    assert_eq!(column(&h, "audit", 1), ["order 2 by Developer"]);
    // The navigate step opened the audit page in the Runtime, with no "open it in the Runtime" notice.
    let body = crate::ui::runtime::tests::run_body(&mut h);
    let page = h.cx.read(|cx| body.read(cx).runtime().read(cx).page().cloned()).expect("a page is open");
    assert_eq!((page.kind.key(), page.id.as_str()), ("table", "audit"));
    assert!(!h.notice().unwrap_or_default().contains("Open it in the Runtime"));
}

#[gpui_kit::test]
fn actions_open_popup_forms_and_get_the_value_a_close_form_step_returns(cx: &mut TestAppContext) {
    use crate::automation_runner::{ActionHost, NavigationTarget, host::SessionHost};
    use std::sync::Arc;
    let mut h = Harness::new_document(cx);
    setup(&mut h, |_| {});
    let form = h.config().design.forms.iter().find(|f| f.name == "Orders").map(|f| f.id.clone())
        .unwrap_or_else(|| h.config().design.forms[0].id.clone());
    let host = Arc::new(SessionHost::new(&h.label, Arc::new(|_| true), Arc::new(|_, _| {}), Arc::new(|| {})));
    let target = NavigationTarget::new("form", &form);
    // The action thread blocks until the popup closes.
    let (opener, closer) = (host.clone(), host.clone());
    let t = target.clone();
    let opened = std::thread::spawn(move || opener.open_popup(&t));
    wait(&mut h);
    assert!(h.exists("popup:1"));
    let closing = std::thread::spawn(move || closer.close_form(&json!("picked")));
    wait(&mut h);
    closing.join().expect("close thread");
    assert!(!h.exists("popup:1"));
    assert_eq!(opened.join().expect("open thread").expect("popup host"), Some(json!("picked")));
    // Dismissing the popup returns null.
    let dismissed = std::thread::spawn(move || host.open_popup(&target));
    wait(&mut h);
    h.click("popup-close:2");
    wait(&mut h);
    assert_eq!(dismissed.join().expect("thread").expect("popup host"), Some(serde_json::Value::Null));
}

#[gpui_kit::test]
fn runs_the_custom_concurrency_action_instead_of_writing_a_custom_action_entity(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    setup(&mut h, |config| {
        config.actions = serde_json::from_value(json!([{
            "id": "guard", "name": "Guard order changes", "onError": "rollback",
            "steps": [
                {"id": "g1", "kind": "createRecord", "table": "audit",
                 "values": {"message": "params.operation & ': ' & old.status & ' → ' & record.status"}},
                {"id": "g2", "kind": "updateRecord", "table": "orders", "match": "current",
                 "values": {"status": "upper(record.status)"}},
            ],
        }]))
        .expect("actions");
        for entity in config.entities.iter_mut().filter(|e| e.table == "orders") {
            entity.concurrency = "customAction".into();
            entity.action_id = Some("guard".into());
        }
    });
    let saved = h.config();
    assert_eq!(saved.entities.iter().find(|e| e.table == "orders").and_then(|e| e.action_id.clone()).as_deref(), Some("guard"));

    let meta = RecordWriteMeta { expected: Some(vec![named("status", text("open"))]), ..RecordWriteMeta::default() };
    records::update(&h.label, "orders", vec![named("status", text("paid"))], vec![DataValue::Integer(1)], meta)
        .expect("update");
    assert_eq!(column(&h, "orders", 1), ["PAID"]);
    assert_eq!(column(&h, "audit", 1), ["update: open → paid"]);

    // A write that bypasses the record path without the values it started from is refused.
    let blind = crate::recordstore::commands::update_row(
        h.label.clone(),
        "orders".into(),
        vec![named("status", text("x"))],
        vec![DataValue::Integer(1)],
        None,
        None,
    );
    assert_eq!(blind.map(|_| ()).unwrap_err().code, "EXPECTED_REQUIRED");
}
