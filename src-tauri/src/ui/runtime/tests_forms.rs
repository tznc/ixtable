//! Generated CRUD forms end to end (port of tests/integration/forms.test.tsx). The React test
//! generated the forms in the Design studio; here they come from "Generate app" and the
//! validation rule is set in the config. Imports are explicit.

use super::tests::{nav, seed, wait};
use super::tests_misc::table_with_keys;
use crate::data::DataValue;
use crate::ui::shell::ModeId;
use gpui_kit::TestAppContext;
use crate::ui::testing::Harness;
use serde_json::json;

#[gpui_kit::test]
fn generates_crud_forms_validates_creates_edits_and_manages_related_records(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    table_with_keys(
        &mut h,
        "customers",
        json!([
            {"name": "id", "declaredType": "INTEGER", "primaryKeyPosition": 1},
            {"name": "name", "declaredType": "TEXT", "nullable": false},
        ]),
        json!([]),
    );
    table_with_keys(
        &mut h,
        "orders",
        json!([
            {"name": "id", "declaredType": "INTEGER", "primaryKeyPosition": 1},
            {"name": "customer_id", "declaredType": "INTEGER", "nullable": false},
            {"name": "note", "declaredType": "TEXT"},
            {"name": "qty", "declaredType": "INTEGER"},
        ]),
        json!([{"columns": ["customer_id"], "targetTable": "customers", "targetColumns": ["id"],
                "onUpdate": null, "onDelete": null}]),
    );
    table_with_keys(
        &mut h,
        "order_items",
        json!([
            {"name": "id", "declaredType": "INTEGER", "primaryKeyPosition": 1},
            {"name": "order_id", "declaredType": "INTEGER"},
            {"name": "product", "declaredType": "TEXT"},
            {"name": "quantity", "declaredType": "INTEGER"},
        ]),
        json!([{"columns": ["order_id"], "targetTable": "orders", "targetColumns": ["id"],
                "onUpdate": null, "onDelete": null}]),
    );
    h.database_changed();
    for (id, name) in [(1, "Acme"), (2, "Globex")] {
        h.insert_row("customers", &[("id", DataValue::Integer(id)), ("name", DataValue::Text(name.into()))]);
    }
    h.switch_mode(ModeId::Run);
    h.click("generate-app");
    wait(&mut h);
    seed(&mut h, |config| {
        let form = config.design.forms.iter_mut().find(|f| f.name == "Orders").expect("Orders form");
        let qty = form.controls.iter_mut().find(|c| c.label == "Qty").expect("Qty control");
        qty.validation.expression = Some("value > 0".into());
        qty.validation.message = Some("Quantity must be positive.".into());
    });

    let orders = nav(&mut h, "Orders");
    h.click(orders);
    wait(&mut h);
    h.click("new-record");
    wait(&mut h);
    h.click("create");
    wait(&mut h);
    assert_eq!(h.label("field:customer:error"), "Customer is required.");

    // The search narrows the choices.
    h.click("field:customer");
    wait(&mut h);
    assert!(h.exists("field:customer:option:globex"));
    h.fill("field:customer:search", "Ac");
    wait(&mut h);
    assert!(!h.exists("field:customer:option:globex"));
    h.click("field:customer:option:acme");
    wait(&mut h);
    h.fill("field:qty", "-1");
    // The React test saw the message after tabbing out of the field. The test window does not
    // deliver the input's blur event when focus moves, so the message is asserted after Create
    // (the same validation; blur validation itself runs the same `validate_control`).
    h.click("field:note");
    wait(&mut h);
    h.click("create");
    wait(&mut h);
    assert_eq!(h.label("field:qty:error"), "Quantity must be positive.");
    h.fill("field:qty", "3");
    h.fill("field:note", "Rush");
    h.click("create");
    wait(&mut h);
    assert_eq!(
        h.read_page("orders").rows,
        vec![vec![
            DataValue::Integer(1),
            DataValue::Integer(1),
            DataValue::Text("Rush".into()),
            DataValue::Integer(3)
        ]]
    );
    assert_eq!(h.label("field:customer"), "Acme");

    h.click("edit");
    wait(&mut h);
    h.fill("field:note", "Normal");
    h.click("save");
    wait(&mut h);
    assert_eq!(h.read_page("orders").rows[0][2], DataValue::Text("Normal".into()));

    // Related records: add, edit, delete.
    h.click("related:order-items:add");
    wait(&mut h);
    h.fill("field:product", "Widget");
    h.fill("field:quantity", "2");
    h.click("create");
    wait(&mut h);
    assert_eq!(h.label("related:order-items:cell:0:product"), "Widget");
    assert_eq!(
        h.read_page("order_items").rows[0],
        vec![
            DataValue::Integer(1),
            DataValue::Integer(1),
            DataValue::Text("Widget".into()),
            DataValue::Integer(2)
        ]
    );
    h.click("related:order-items:edit:0");
    wait(&mut h);
    h.fill("field:quantity", "5");
    h.click("save");
    wait(&mut h);
    assert_eq!(h.label("related:order-items:cell:0:quantity"), "5");

    h.dialogs.queue_confirm(true);
    h.click("related:order-items:delete:0");
    wait(&mut h);
    assert_eq!(h.label("related:order-items:empty"), "No order items yet.");
    assert_eq!(h.read_page("order_items").total, 0);

    assert_eq!(h.label("back-crumb"), "Back to Orders list");
    h.click("back-crumb-button");
    wait(&mut h);
    assert!(h.exists("row:0"));
}
