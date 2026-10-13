//! Form events (port of tests/integration/form-events.test.tsx). The actions are run by a
//! test-double runner that does what the React test's declarative steps did (message,
//! createRecord, fail); the real runner is Automation's. Imports are explicit.

use super::tests::{nav, seed, wait, with_real_runner, with_runner};
use crate::data::{DataValue, NamedValue};
use crate::forms::actions::{ActionResult, NoticeTone};
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;
use serde_json::json;

fn audit(label: &str, message: String) {
    let values = vec![NamedValue { column: "message".into(), value: DataValue::Text(message) }];
    crate::records::insert(label, "audit", values).expect("audit row");
}

fn messages(h: &Harness) -> Vec<DataValue> {
    h.read_page("audit").rows.iter().map(|r| r[1].clone()).collect()
}

#[gpui_kit::test]
fn runs_on_load_on_current_before_update_and_after_update_actions(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.create_table("orders", &[("id", "INTEGER", 1), ("status", "TEXT", 0)]);
    h.create_table("audit", &[("id", "INTEGER", 1), ("message", "TEXT", 0)]);
    h.insert_row(
        "orders",
        &[("id", DataValue::Integer(1)), ("status", DataValue::Text("open".into()))],
    );
    h.switch_mode(ModeId::Run);
    h.click("generate-app");
    wait(&mut h);
    seed(&mut h, |config| {
        for (id, name) in
            [("greet", "Greet"), ("visit", "Log visit"), ("guard", "Guard status"), ("after", "Log save")]
        {
            config.actions.push(
                serde_json::from_value(json!({"id": id, "name": name, "onError": "stop", "steps": []}))
                    .expect("action"),
            );
        }
        let form = config.design.forms.iter_mut().find(|f| f.name == "Orders").expect("Orders form");
        form.events = serde_json::from_value(json!({
            "onLoad": "greet", "onCurrent": "visit", "beforeUpdate": "guard", "afterUpdate": "after",
        }))
        .expect("events");
    });
    let _runner = with_runner(|action, ctx| {
        let record = ctx.record.clone().unwrap_or_default();
        let text = |key: &str| record.get(key).map(|v| v.as_str().map_or(v.to_string(), str::to_string));
        match action {
            "greet" => ctx.host.notify("Hello Developer", NoticeTone::Info),
            "visit" => audit(&ctx.window_label, format!("current {}", text("id").unwrap_or_default())),
            "guard" if text("status").as_deref() == Some("bad") => {
                return ActionResult::failure("Status cannot be bad");
            }
            "after" => audit(&ctx.window_label, format!("saved {}", text("status").unwrap_or_default())),
            _ => {}
        }
        ActionResult::success()
    });
    let orders = nav(&mut h, "Orders");
    h.click(orders);
    wait(&mut h);
    h.click("row:0");
    wait(&mut h);
    assert_eq!(h.label("notice:info"), "Hello Developer");
    assert_eq!(messages(&h), [DataValue::Text("current 1".into())]);

    // Entering edit mode stays on the same record, so on current does not run again.
    h.click("edit");
    wait(&mut h);
    h.fill("field:status", "bad");
    h.click("save");
    wait(&mut h);
    assert_eq!(h.label("status:error"), "Status cannot be bad");
    assert_eq!(h.read_page("orders").rows[0][1], DataValue::Text("open".into()));

    h.fill("field:status", "paid");
    h.click("save");
    wait(&mut h);
    assert_eq!(h.read_page("orders").rows[0][1], DataValue::Text("paid".into()));
    assert_eq!(
        messages(&h),
        [DataValue::Text("current 1".into()), DataValue::Text("saved paid".into())]
    );
}

#[gpui_kit::test]
fn runs_declarative_form_events_through_the_automation_runner(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.create_table("orders", &[("id", "INTEGER", 1), ("status", "TEXT", 0)]);
    h.create_table("audit", &[("id", "INTEGER", 1), ("message", "TEXT", 0)]);
    h.insert_row(
        "orders",
        &[("id", DataValue::Integer(1)), ("status", DataValue::Text("open".into()))],
    );
    h.switch_mode(ModeId::Run);
    h.click("generate-app");
    wait(&mut h);
    let _runner = with_real_runner();
    seed(&mut h, |config| {
        let actions = json!([
            {"id": "greet", "name": "Greet", "onError": "stop", "steps": [
                {"id": "s1", "kind": "message", "text": "'Hello ' & app.user.name"}]},
            {"id": "visit", "name": "Log visit", "onError": "stop", "steps": [
                {"id": "s1", "kind": "createRecord", "table": "audit",
                 "values": {"message": "'current ' & record.id"}}]},
            {"id": "guard", "name": "Guard status", "onError": "stop", "steps": [
                {"id": "s1", "kind": "condition", "when": "record.status = 'bad'",
                 "then": [{"id": "s2", "kind": "fail", "message": "'Status cannot be bad'"}]}]},
            {"id": "after", "name": "Log save", "onError": "stop", "steps": [
                {"id": "s1", "kind": "createRecord", "table": "audit",
                 "values": {"message": "'saved ' & record.status"}}]},
        ]);
        config.actions = serde_json::from_value(actions).expect("actions");
        let form = config.design.forms.iter_mut().find(|f| f.name == "Orders").expect("Orders form");
        form.events = serde_json::from_value(json!({
            "onLoad": "greet", "onCurrent": "visit", "beforeUpdate": "guard", "afterUpdate": "after",
        }))
        .expect("events");
    });
    let orders = nav(&mut h, "Orders");
    h.click(orders);
    wait(&mut h);
    h.click("row:0");
    wait(&mut h);
    assert_eq!(h.label("notice:info"), "Hello Developer");
    assert_eq!(messages(&h), [DataValue::Text("current 1".into())]);

    h.click("edit");
    wait(&mut h);
    h.fill("field:status", "bad");
    h.click("save");
    wait(&mut h);
    assert_eq!(h.label("status:error"), "Status cannot be bad");
    assert_eq!(h.read_page("orders").rows[0][1], DataValue::Text("open".into()));

    h.fill("field:status", "paid");
    h.click("save");
    wait(&mut h);
    assert_eq!(h.read_page("orders").rows[0][1], DataValue::Text("paid".into()));
    assert_eq!(
        messages(&h),
        [DataValue::Text("current 1".into()), DataValue::Text("saved paid".into())]
    );
}
