//! Run mode captures (`runtime-*`, `guided-*`, `form-views-*`, `nesting-02`). Port of the
//! Runtime captures of scripts/screenshot/specs/{forms,guided,form-views,nesting}.spec.tsx.
//!
//! The Design studio captures (`forms-01-designer`, `form-views-07-properties`,
//! `nesting-01-child-form-picker`, `grid-*`, `app-qa-06/07`) are in `design.rs`; the report
//! captures (`nesting-03/04`) belong to Reports. Where the React specs designed forms in
//! the studio, these generate the app or write the definitions through the ConfigStore.

use crate::data::DataValue;
use crate::ui::runtime::tests::{
    generate, nav, seed, task_form, uid, wait, with_real_runner,
};
use crate::ui::runtime::tests_misc::table_with_keys;
use crate::ui::screenshot::context;
use crate::ui::shell::ModeId;
use crate::ui::testing::{Harness, UiContext};
use serde_json::{Value, json};

const WINDOW: (f32, f32) = (1280.0, 1000.0);

pub(super) fn int(n: i64) -> DataValue {
    DataValue::Integer(n)
}

pub(super) fn text(s: &str) -> DataValue {
    DataValue::Text(s.into())
}

pub(super) fn keyed(column: &str, target: &str) -> Value {
    json!([{"columns": [column], "targetTable": target, "targetColumns": ["id"],
            "onUpdate": null, "onDelete": null}])
}

pub(super) fn id_column() -> Value {
    json!({"name": "id", "declaredType": "INTEGER", "primaryKeyPosition": 1})
}

pub(super) fn column(name: &str, declared: &str) -> Value {
    json!({"name": name, "declaredType": declared})
}

/// Customers, orders and order items (the sales fixture of the React specs).
pub(super) fn seed_sales<C: UiContext>(h: &mut Harness<C>) {
    table_with_keys(h, "customers", json!([id_column(), column("name", "TEXT"), column("city", "TEXT")]), json!([]));
    table_with_keys(
        h,
        "orders",
        json!([
            id_column(), json!({"name": "customer_id", "declaredType": "INTEGER", "nullable": false}), column("status", "TEXT"),
            column("amount", "INTEGER"), column("note", "TEXT"),
        ]),
        keyed("customer_id", "customers"),
    );
    table_with_keys(
        h,
        "order_items",
        json!([id_column(), column("order_id", "INTEGER"), column("product", "TEXT"), column("quantity", "INTEGER")]),
        keyed("order_id", "orders"),
    );
    h.database_changed();
    for (id, name, city) in [(1, "Northstar Goods", "Portland"), (2, "Juniper Supply", "Austin")] {
        h.insert_row("customers", &[("id", int(id)), ("name", text(name)), ("city", text(city))]);
    }
    let orders = [(1, 1, "paid", 340), (2, 1, "open", 120), (3, 2, "open", 75), (4, 2, "paid", 210), (5, 1, "open", 60)];
    for (id, customer, status, amount) in orders {
        h.insert_row(
            "orders",
            &[("id", int(id)), ("customer_id", int(customer)), ("status", text(status)), ("amount", int(amount))],
        );
    }
    let items = [(1, 1, "Ceramic pour-over set", 1), (2, 1, "Oak serving board", 2), (3, 2, "Linen napkins", 6)];
    for (id, order, product, quantity) in items {
        h.insert_row(
            "order_items",
            &[("id", int(id)), ("order_id", int(order)), ("product", text(product)), ("quantity", int(quantity))],
        );
    }
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn runtime_forms() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, WINDOW);
    h.click("new-document");
    seed_sales(&mut h);
    generate(&mut h);
    wait(&mut h);
    seed(&mut h, |config| {
        let form = config.design.forms.iter_mut().find(|f| f.name == "Orders").expect("Orders form");
        let amount = form.controls.iter_mut().find(|c| c.label == "Amount").expect("Amount control");
        amount.validation.expression = Some("value > 0".into());
        amount.validation.message = Some("Amount must be positive.".into());
    });

    let orders = nav(&mut h, "Orders");
    h.click(orders);
    wait(&mut h);
    assert!(h.exists("row:4"));
    assert_eq!(h.label("cell:0:customer-id"), "Northstar Goods");
    h.capture(
        "runtime-01-list",
        &[
            "The Runtime navigation lists Customers and Orders with Orders current.",
            "The Orders list shows five records with customer names instead of raw ids.",
        ],
    );

    h.click("new-record");
    wait(&mut h);
    h.fill("field:amount", "-5");
    h.click("create");
    wait(&mut h);
    assert_eq!(h.label("field:customer:error"), "Customer is required.");
    assert_eq!(h.label("field:amount:error"), "Amount must be positive.");
    h.capture(
        "runtime-02-create-validation",
        &[
            "The New Orders form shows a searchable Customer relationship selector.",
            "Required and rule validation messages appear under Customer and Amount.",
        ],
    );

    h.click("field:customer");
    wait(&mut h);
    h.click("field:customer:option:juniper-supply");
    h.fill("field:amount", "99");
    h.fill("field:status", "open");
    h.click("create");
    wait(&mut h);
    assert_eq!(h.label("field:customer"), "Juniper Supply");
    h.click("related:order-items:add");
    wait(&mut h);
    h.fill("field:product", "Stoneware mug");
    h.fill("field:quantity", "4");
    h.click("create");
    wait(&mut h);
    assert_eq!(h.label("related:order-items:cell:0:product"), "Stoneware mug");
    assert_eq!(h.label("back-crumb"), "Orders list");
    h.capture(
        "runtime-03-master-detail",
        &[
            "The order detail shows Juniper Supply, amount 99, and only Edit/Delete in its header.",
            "One back link to the Orders list sits above the card; 'Record created.' shows once inside it.",
            "The Order items list shows the new Stoneware mug row and its own 'Record created.' message.",
        ],
    );

    h.click("edit");
    wait(&mut h);
    h.fill("field:note", "Call before delivery");
    h.capture(
        "runtime-04-edit",
        &["The Edit Orders form is prefilled and shows Save and Cancel actions."],
    );
    h.click("save");
    wait(&mut h);
    assert_eq!(h.read_page("orders").rows[5][4], text("Call before delivery"));
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn runtime_guided() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, WINDOW);
    h.click("new-document");
    table_with_keys(&mut h, "customers", json!([id_column(), column("name", "TEXT")]), json!([]));
    table_with_keys(
        &mut h,
        "orders",
        json!([id_column(), column("name", "TEXT"), column("customer_id", "INTEGER")]),
        keyed("customer_id", "customers"),
    );
    table_with_keys(
        &mut h,
        "order_lines",
        json!([id_column(), column("name", "TEXT"), column("order_id", "INTEGER")]),
        keyed("order_id", "orders"),
    );
    h.database_changed();
    for (id, name) in [(1, "Northstar Goods"), (2, "Juniper Supply")] {
        h.insert_row("customers", &[("id", int(id)), ("name", text(name))]);
    }
    for (id, name, customer) in [(10, "Spring restock", 1), (11, "Gift sets", 1), (12, "Trial order", 2)] {
        h.insert_row("orders", &[("id", int(id)), ("name", text(name)), ("customer_id", int(customer))]);
    }
    for (id, name) in [(100, "Ceramic pour-over set"), (101, "Oak serving board")] {
        h.insert_row("order_lines", &[("id", int(id)), ("name", text(name)), ("order_id", int(10))]);
    }
    h.switch_mode(ModeId::Run);
    wait(&mut h);
    assert_eq!(h.label("generate-app"), "Generate app from tables");
    h.capture(
        "guided-01-build-your-app",
        &[
            "The empty Runtime page shows a 'Build your app' hint saying the application has no pages yet.",
            "The hint offers a 'Generate app from tables' primary button, with no blank starter form below it.",
        ],
    );

    h.click("generate-app");
    wait(&mut h);
    for label in ["Customers", "Orders", "Order lines"] {
        let id = nav(&mut h, label);
        assert!(h.exists(id), "{label}");
    }
    assert!(!h.exists("generate-app"));
    let orders = nav(&mut h, "Orders");
    h.click(orders);
    wait(&mut h);
    assert_eq!(h.label("cell:0:customer-id"), "Northstar Goods");
    h.capture(
        "guided-02-generated-list",
        &[
            "The Runtime navigation lists Customers, Orders, and Order lines generated from the tables.",
            "The Orders list shows three orders with customer names instead of raw customer ids.",
        ],
    );

    h.click("row:0");
    wait(&mut h);
    assert_eq!(h.label("field:customer"), "Northstar Goods");
    assert_eq!(h.label("related:order-lines:cell:0:name"), "Ceramic pour-over set");
    assert_eq!(h.label("related:order-lines:cell:1:name"), "Oak serving board");
    h.capture(
        "guided-03-detail-related-list",
        &[
            "The Spring restock order detail shows its customer as Northstar Goods.",
            "An Order lines related list below the fields shows the two lines of this order.",
        ],
    );
}

pub(super) fn field(label: &str, column: &str, kind: &str, row: u32, col: u32, span: u32) -> Value {
    json!({
        "id": uid(), "kind": kind, "label": label, "binding": {"column": column},
        "validation": {"required": column == "title"},
        "placement": {"column": col, "row": row, "columnSpan": span, "rowSpan": 1},
    })
}

pub(super) fn seed_task_table<C: UiContext>(h: &mut Harness<C>) {
    h.create_table(
        "tasks",
        &[("id", "INTEGER", 1), ("title", "TEXT", 0), ("owner", "TEXT", 0), ("priority", "INTEGER", 0)],
    );
    let rows = [
        ("Draft launch plan", "Ana", 1),
        ("Review supplier contract", "Ben", 2),
        ("Order shelving", "Chen", 3),
        ("Update price list", "Ana", 2),
    ];
    for (index, (title, owner, priority)) in rows.into_iter().enumerate() {
        h.insert_row(
            "tasks",
            &[("id", int(index as i64 + 1)), ("title", text(title)), ("owner", text(owner)), ("priority", int(priority))],
        );
    }
}

fn stacked_controls() -> Value {
    json!([
        field("Title", "title", "text", 1, 1, 12),
        field("Owner", "owner", "text", 2, 1, 6),
        field("Priority", "priority", "number", 2, 7, 6),
    ])
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn runtime_form_views() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, WINDOW);
    h.click("new-document");
    seed_task_table(&mut h);

    // A continuous form with an edited row and the new-record row.
    task_form(
        &mut h,
        json!({
            "name": "Tasks", "modes": ["continuous", "create", "edit"],
            "listColumns": ["title", "owner", "priority"],
            "controls": [
                field("Title", "title", "text", 1, 1, 6),
                field("Owner", "owner", "text", 1, 7, 4),
                field("Priority", "priority", "number", 1, 11, 2),
            ],
        }),
    );
    h.switch_mode(ModeId::Run);
    wait(&mut h);
    assert!(h.exists("crow:2") && h.exists("crow:new"));
    h.type_into("record-new:field:title", "Book delivery van");
    h.fill("record-2:field:title", "Sign supplier contract");
    assert!(h.exists("crow:2:save"));
    h.capture(
        "form-views-01-continuous",
        &[
            "Four task records repeat as rows with Title, Owner and Priority inputs side by side.",
            "Record 2 shows 'Sign supplier contract' with a marked left edge and Save and Undo buttons.",
            "The last row is a new-record row with 'Book delivery van' typed and an Add button.",
        ],
    );
    h.click("crow:2:save");
    wait(&mut h);
    assert_eq!(h.label("continuous-notice"), "Changes saved.");
    assert!(!h.exists("crow:2:save"));
    assert!(h.exists("crow:new:save"));
    h.capture(
        "form-views-02-continuous-saved",
        &[
            "A 'Changes saved.' status shows above the rows.",
            "Record 2 shows 'Sign supplier contract' without Save or Undo buttons.",
            "The new-record row still holds 'Book delivery van' with its Add button.",
        ],
    );

    // A split form with the navigation bar.
    task_form(
        &mut h,
        json!({
            "name": "Tasks", "modes": ["split", "detail", "create", "edit"], "navigationBar": true,
            "listColumns": ["title", "owner", "priority"], "controls": stacked_controls(),
        }),
    );
    h.switch_mode(ModeId::Data);
    h.switch_mode(ModeId::Run);
    wait(&mut h);
    h.click("row:2");
    wait(&mut h);
    assert_eq!(h.label("nav-position"), "Record 3 of 4");
    h.capture(
        "form-views-03-split",
        &[
            "The task list shows as a table above a detail pane, with the 'Order shelving' row highlighted.",
            "Above the detail pane a navigation bar shows first, previous, next, last and new buttons and 'Record 3 of 4'.",
            "The detail pane shows Order shelving, Chen and priority 3 read-only with Edit and Delete buttons.",
        ],
    );
    h.click("nav-last");
    wait(&mut h);
    assert_eq!(h.label("nav-position"), "Record 4 of 4");
    h.capture(
        "form-views-04-split-last",
        &[
            "The 'Update price list' row is highlighted in the list.",
            "The bar reads 'Record 4 of 4' with the next and last buttons greyed out.",
        ],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn runtime_popup_form() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, WINDOW);
    h.click("new-document");
    seed_task_table(&mut h);
    let _runner = with_real_runner();
    task_form(
        &mut h,
        json!({
            "name": "Tasks", "modes": ["detail", "create", "edit"],
            "listColumns": ["title", "owner", "priority"], "controls": stacked_controls(),
        }),
    );
    let (task, launcher, add) = (h.config().design.forms[0].id.clone(), uid(), uid());
    let planner = json!({
        "id": launcher, "name": "Planner", "source": null, "modes": ["create"],
        "layout": crate::ui::runtime::tests::grid(), "listColumns": [], "pageSize": 25, "rules": [],
        "controls": [
            {"id": uid(), "kind": "label", "label": "Intro", "text": "Plan this week's work.",
             "validation": {"required": false}, "placement": {"column": 1, "row": 1, "columnSpan": 12, "rowSpan": 1}},
            {"id": uid(), "kind": "button", "label": "Add a task", "validation": {"required": false},
             "placement": {"column": 1, "row": 2, "columnSpan": 3, "rowSpan": 1}, "actionId": add},
        ],
    });
    let action = json!({
        "id": add, "name": "Add a task", "onError": "stop", "steps": [
            {"id": uid(), "kind": "openForm", "formId": task, "mode": "create", "popup": true, "storeAs": "task"},
            {"id": uid(), "kind": "message", "text": "'Added ' & results.task.title & ' for ' & results.task.owner"},
        ],
    });
    seed(&mut h, move |config| {
        config.design.forms.push(serde_json::from_value(planner).expect("planner form"));
        config.design.navigation[0].kind = crate::design::NavKind::Form;
        config.design.navigation[0].target_id = Some(launcher);
        config.design.navigation[0].mode = Some(crate::design::FormMode::Create);
        config.actions.push(serde_json::from_value(action).expect("action"));
    });
    h.switch_mode(ModeId::Run);
    wait(&mut h);
    h.click("button:add-a-task");
    wait(&mut h);
    assert!(h.exists("popup:1"));
    h.fill("field:title", "Call the landlord");
    h.fill("field:owner", "Ben");
    h.capture(
        "form-views-05-popup",
        &[
            "A modal dialog titled 'New task' sits over a dimmed Planner page.",
            "The dialog shows Title 'Call the landlord', Owner 'Ben', and Cancel, Create and Close buttons.",
        ],
    );
    h.click_in("popup:1", "create");
    wait(&mut h);
    assert!(!h.exists("popup:1"));
    assert_eq!(h.label("notice:info"), "Added Call the landlord for Ben");
    h.capture(
        "form-views-06-popup-result",
        &["The dialog is gone and the Planner page shows 'Added Call the landlord for Ben'."],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn runtime_nesting() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, (1280.0, 1400.0));
    h.click("new-document");
    seed_sales(&mut h);
    table_with_keys(
        &mut h,
        "allocations",
        json!([id_column(), column("order_item_id", "INTEGER"), column("bin", "TEXT"), column("quantity", "INTEGER")]),
        keyed("order_item_id", "order_items"),
    );
    h.database_changed();
    for (id, item, bin) in [(1, 1, "Aisle 3 / Bin A1"), (2, 1, "Aisle 3 / Bin A2"), (3, 2, "Aisle 7 / Bin C4")] {
        h.insert_row(
            "allocations",
            &[("id", int(id)), ("order_item_id", int(item)), ("bin", text(bin)), ("quantity", int(1))],
        );
    }
    let ids: Vec<String> = (0..4).map(|_| uid()).collect();
    let names = ["Customers", "Orders", "Order items", "Allocations"];
    let tables = ["customers", "orders", "order_items", "allocations"];
    let columns: [[&str; 2]; 4] = [["name", "city"], ["status", "amount"], ["product", "quantity"], ["bin", "quantity"]];
    let keys = [Some("customer_id"), Some("order_id"), Some("order_item_id"), None];
    let form = |level: usize| {
        let mut controls: Vec<Value> = columns[level]
            .iter()
            .enumerate()
            .map(|(i, column)| {
                let label = format!("{}{}", column[..1].to_uppercase(), &column[1..]);
                field(&label, column, "text", 1, 1 + i as u32 * 6, 6)
            })
            .collect();
        if let Some(key) = keys[level] {
            controls.push(json!({
                "id": uid(), "kind": "relatedList", "label": names[level + 1],
                "related": {"table": tables[level + 1], "foreignKey": key, "parentColumn": "id",
                            "columns": [], "formId": ids[level + 1]},
                "placement": {"column": 1, "row": 2, "columnSpan": 12, "rowSpan": 1},
            }));
        }
        json!({
            "id": ids[level], "name": names[level], "source": {"kind": "table", "table": tables[level]},
            "modes": ["list", "detail", "create", "edit"], "listColumns": columns[level],
            "layout": crate::ui::runtime::tests::grid(), "rules": [], "pageSize": 25, "controls": controls,
        })
    };
    let forms: Vec<Value> = (0..4).map(form).collect();
    let (first, nav_id) = (ids[0].clone(), uid());
    seed(&mut h, move |config| {
        config.design.forms = forms.into_iter().map(|f| serde_json::from_value(f).expect("form")).collect();
        config.design.navigation = vec![
            serde_json::from_value(json!({"id": nav_id, "label": "Customers", "kind": "form", "targetId": first}))
                .expect("navigation item"),
        ];
        config.design.start_page = None;
    });
    h.switch_mode(ModeId::Run);
    wait(&mut h);
    let customers = nav(&mut h, "Customers");
    h.click(customers);
    wait(&mut h);
    h.click("row:0");
    wait(&mut h);
    assert_eq!(h.label("related:orders:cell:0:amount"), "340");
    h.click("related:orders:edit:0");
    wait(&mut h);
    assert_eq!(h.label("related:order-items:cell:0:product"), "Ceramic pour-over set");
    h.click("related:order-items:edit:0");
    wait(&mut h);
    assert_eq!(h.label("related:allocations:cell:1:bin"), "Aisle 3 / Bin A2");
    assert!(!h.exists("related:allocations:cell:2:bin"));
    h.capture(
        "nesting-02-runtime-three-levels",
        &[
            "Customer Northstar Goods shows its orders; order 1 is open for editing inside the list.",
            "Inside order 1, its order items list has the Ceramic pour-over set open for editing.",
            "Inside that item, the Allocations list shows bins A1 and A2 only.",
        ],
    );
}
