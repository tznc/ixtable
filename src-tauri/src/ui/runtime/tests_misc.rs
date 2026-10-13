//! Tabs, composite keys and the relational journey (port of tests/integration/form-tabs
//! .test.tsx, composite-keys.test.tsx and the relational parts of guided-crud.test.tsx).
//! Imports are explicit.

use super::tests::{at, grid, nav, seed, uid, wait};
use crate::data::DataValue;
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use crate::validation::Severity;
use gpui_kit::TestAppContext;
use serde_json::{Value, json};

fn table_with_keys(h: &mut Harness, name: &str, columns: Value, foreign_keys: Value) {
    let columns: Vec<Value> = columns
        .as_array()
        .expect("columns")
        .iter()
        .map(|c| {
            let mut column = json!({
                "nullable": true, "primaryKeyPosition": 0, "unique": false,
                "defaultExpression": null, "generatedExpression": null,
            });
            for (key, value) in c.as_object().expect("column") {
                column[key] = value.clone();
            }
            column
        })
        .collect();
    let spec = json!({
        "name": name, "columns": columns, "foreignKeys": foreign_keys, "checks": [],
        "withoutRowid": false,
    });
    crate::create_database_table(h.label.clone(), serde_json::from_value(spec).expect("table spec"))
        .expect("create table");
}

fn design_errors(h: &Harness) -> Vec<String> {
    crate::design::validate_design(h.label.clone())
        .expect("validate design")
        .into_iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| i.message)
        .collect()
}

fn rows(h: &Harness, table: &str) -> Vec<Vec<DataValue>> {
    h.read_page(table).rows
}

#[gpui_kit::test]
fn builds_a_relational_crud_app_from_two_tables(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    table_with_keys(
        &mut h,
        "customers",
        json!([
            {"name": "id", "declaredType": "INTEGER", "nullable": false, "primaryKeyPosition": 1},
            {"name": "name", "declaredType": "TEXT"},
        ]),
        json!([]),
    );
    table_with_keys(
        &mut h,
        "orders",
        json!([
            {"name": "id", "declaredType": "INTEGER", "nullable": false, "primaryKeyPosition": 1},
            {"name": "name", "declaredType": "TEXT"},
            {"name": "customer_id", "declaredType": "INTEGER"},
        ]),
        json!([{"columns": ["customer_id"], "targetTable": "customers", "targetColumns": ["id"],
                "onUpdate": null, "onDelete": null}]),
    );
    h.database_changed();
    h.switch_mode(ModeId::Run);
    h.click("generate-app");
    wait(&mut h);
    assert!(design_errors(&h).is_empty(), "{:?}", design_errors(&h));

    let customers = nav(&mut h, "Customers");
    h.click(customers.clone());
    wait(&mut h);
    h.click("new-record");
    wait(&mut h);
    h.fill("field:name", "Acme");
    h.click("create");
    wait(&mut h);
    assert_eq!(rows(&h, "customers").len(), 1);
    assert_eq!(h.label("record-title"), "Customers");

    let orders = nav(&mut h, "Orders");
    h.click(orders);
    wait(&mut h);
    h.click("new-record");
    wait(&mut h);
    h.click("field:customer");
    wait(&mut h);
    h.click("field:customer:option:acme");
    h.fill("field:name", "First order");
    h.click("create");
    wait(&mut h);
    assert_eq!(
        rows(&h, "orders"),
        vec![vec![DataValue::Integer(1), DataValue::Text("First order".into()), DataValue::Integer(1)]]
    );

    h.click(customers);
    wait(&mut h);
    h.click("row:0");
    wait(&mut h);
    assert_eq!(h.label("related:orders:cell:0:name"), "First order");
}

#[gpui_kit::test]
fn generates_a_valid_app_for_nested_and_self_referencing_tables(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let linked = |h: &mut Harness, name: &str, link: Option<(&str, &str)>| {
        let mut columns = vec![
            json!({"name": "id", "declaredType": "INTEGER", "primaryKeyPosition": 1, "nullable": false}),
            json!({"name": "name", "declaredType": "TEXT"}),
        ];
        let mut keys = vec![];
        if let Some((column, target)) = link {
            columns.push(json!({"name": column, "declaredType": "INTEGER"}));
            keys.push(json!({"columns": [column], "targetTable": target, "targetColumns": ["id"],
                             "onUpdate": null, "onDelete": null}));
        }
        table_with_keys(h, name, Value::Array(columns), Value::Array(keys));
    };
    linked(&mut h, "customers", None);
    linked(&mut h, "orders", Some(("customer_id", "customers")));
    linked(&mut h, "order_lines", Some(("order_id", "orders")));
    linked(&mut h, "employees", Some(("manager_id", "employees")));
    h.database_changed();
    h.switch_mode(ModeId::Run);
    h.click("generate-app");
    wait(&mut h);
    let employees = nav(&mut h, "Employees");
    assert!(h.exists(employees));
    assert!(design_errors(&h).is_empty(), "{:?}", design_errors(&h));
    let forms: Vec<String> = h.config().design.forms.iter().map(|f| f.name.clone()).collect();
    assert!(forms.contains(&"Orders (related)".to_string()), "{forms:?}");
    assert!(forms.contains(&"Employees (related)".to_string()), "{forms:?}");
}

#[gpui_kit::test]
fn generates_selectors_and_related_lists_over_a_multi_column_foreign_key(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    table_with_keys(
        &mut h,
        "thresholds",
        json!([
            {"name": "product_id", "declaredType": "INTEGER", "nullable": false, "primaryKeyPosition": 1},
            {"name": "location_id", "declaredType": "INTEGER", "nullable": false, "primaryKeyPosition": 2},
            {"name": "label", "declaredType": "TEXT"},
        ]),
        json!([]),
    );
    table_with_keys(
        &mut h,
        "counts",
        json!([
            {"name": "id", "declaredType": "INTEGER", "nullable": false, "primaryKeyPosition": 1},
            {"name": "product_id", "declaredType": "INTEGER"},
            {"name": "location_id", "declaredType": "INTEGER"},
            {"name": "counted", "declaredType": "INTEGER"},
        ]),
        json!([{"columns": ["product_id", "location_id"], "targetTable": "thresholds",
                "targetColumns": ["product_id", "location_id"], "onUpdate": null, "onDelete": null}]),
    );
    h.database_changed();
    for (product, location, label) in [(1, 1, "Hammer at depot"), (1, 2, "Hammer at store")] {
        h.insert_row(
            "thresholds",
            &[
                ("product_id", DataValue::Integer(product)),
                ("location_id", DataValue::Integer(location)),
                ("label", DataValue::Text(label.into())),
            ],
        );
    }
    h.switch_mode(ModeId::Run);
    h.click("generate-app");
    wait(&mut h);
    assert!(design_errors(&h).is_empty(), "{:?}", design_errors(&h));

    let counts = nav(&mut h, "Counts");
    h.click(counts);
    wait(&mut h);
    h.click("new-record");
    wait(&mut h);
    h.click("field:threshold");
    wait(&mut h);
    h.click("field:threshold:option:hammer-at-store");
    h.fill("field:counted", "4");
    h.click("create");
    wait(&mut h);
    assert_eq!(
        rows(&h, "counts"),
        vec![vec![
            DataValue::Integer(1),
            DataValue::Integer(1),
            DataValue::Integer(2),
            DataValue::Integer(4)
        ]]
    );
    assert_eq!(h.label("field:threshold"), "Hammer at store");

    let thresholds = nav(&mut h, "Thresholds");
    h.click(thresholds);
    wait(&mut h);
    h.click("row:0");
    wait(&mut h);
    assert_eq!(h.label("related:counts:empty"), "No counts yet.");
    h.click("back-crumb-button");
    wait(&mut h);
    h.click("row:1");
    wait(&mut h);
    assert_eq!(h.label("related:counts:cell:0:counted"), "4");
}

fn ticket_form(h: &mut Harness) {
    h.create_table(
        "tickets",
        &[("id", "INTEGER", 1), ("title", "TEXT", 0), ("priority", "INTEGER", 0), ("notes", "TEXT", 0)],
    );
    seed(h, |config| {
        let id = config.design.forms[0].id.clone();
        let (tabs, general, planning, section) = (uid(), uid(), uid(), uid());
        let field = |label: &str, column: &str, parent: Value, required: bool| {
            json!({
                "id": uid(), "kind": if column == "priority" { "number" } else { "text" },
                "label": label, "binding": {"column": column},
                "validation": {"required": required},
                "placement": {"column": 1, "row": 1, "columnSpan": 6, "rowSpan": 1},
                "parent": parent,
            })
        };
        let form = json!({
            "id": id, "name": "Ticket", "source": {"kind": "table", "table": "tickets"},
            "modes": ["create", "edit", "list", "detail"], "layout": grid(), "listColumns": [],
            "rules": [], "pageSize": 25,
            "controls": [
                {"id": tabs, "kind": "tabs", "label": "Ticket tabs", "validation": {"required": false},
                 "placement": at(1), "layout": grid(),
                 "tabs": [{"id": general, "label": "General"}, {"id": planning, "label": "Planning"}]},
                field("Title", "title", json!({"id": tabs, "tab": general}), false),
                field("Priority", "priority", json!({"id": tabs, "tab": planning}), true),
                {"id": section, "kind": "section", "label": "Follow-up", "validation": {"required": false},
                 "placement": at(2), "layout": grid(), "enabledWhen": "record.priority > 0"},
                field("Notes", "notes", json!({"id": section}), false),
            ],
        });
        config.design.forms[0] = serde_json::from_value(form).expect("ticket form");
        config.design.navigation[0].label = "Tickets".into();
        config.design.navigation[0].target_id = Some(id);
        config.design.navigation[0].mode = Some(crate::design::FormMode::Create);
    });
    h.switch_mode(ModeId::Run);
    wait(h);
}

#[gpui_kit::test]
fn switches_tabs_and_flags_a_tab_with_errors(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    ticket_form(&mut h);
    assert_eq!(h.label("tab:general"), "General");
    assert!(h.exists("field:title"));
    assert!(!h.exists("field:priority"));

    h.fill("field:title", "Printer jam");
    h.click("create");
    wait(&mut h);
    assert_eq!(h.label("tab:planning"), "Planning (has errors)");
    assert!(!h.exists("field:priority"));

    h.click("tab:planning");
    assert!(!h.exists("field:title"));
    assert_eq!(h.label("field:priority:error"), "Priority is required.");
    h.fill("field:priority", "2");
    h.click("tab:general");
    assert!(h.exists("field:title"));
    assert!(!h.exists("field:priority"));
    assert!(rows(&h, "tickets").is_empty());
}

#[gpui_kit::test]
fn keeps_the_stored_value_of_a_field_disabled_after_it_was_typed_in(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    ticket_form(&mut h);
    h.fill("field:title", "Jam");
    h.click("tab:planning");
    h.fill("field:priority", "2");
    h.fill("field:notes", "typed while enabled");
    h.fill("field:priority", "0");
    h.click("create");
    wait(&mut h);
    assert_eq!(
        rows(&h, "tickets"),
        vec![vec![
            DataValue::Integer(1),
            DataValue::Text("Jam".into()),
            DataValue::Integer(0),
            DataValue::Null
        ]]
    );

    // Editing: enable, type, disable again; the saved row keeps its stored value.
    h.click("edit");
    wait(&mut h);
    h.click("tab:planning");
    h.fill("field:priority", "3");
    h.fill("field:notes", "also typed");
    h.fill("field:priority", "0");
    h.click("save");
    wait(&mut h);
    assert_eq!(rows(&h, "tickets")[0][3], DataValue::Null);
}
