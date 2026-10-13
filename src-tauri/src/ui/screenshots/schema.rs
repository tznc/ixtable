//! Schema design: the create-table form, the table designer, impact previews and the
//! relationship diagram (`schema-*`).

use crate::ui::data::testkit::{click_designer_in, inspect, make_table, seed_sales};
use crate::ui::screenshot::context;
use crate::ui::testing::Harness;
use serde_json::json;

// Tall enough for the whole designer without scrolling.
const TALL: (f32, f32) = (1440.0, 3000.0);

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn schema_designer() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, TALL);
    h.click("new-document");
    seed_sales(&mut h);

    h.click("new-table");
    h.enter("Table name", "shipments");
    h.enter("Column 1:name", "order_id");
    h.pick("Column 1:type", "Integer");
    h.enter("Column 2:name", "parcel");
    h.pick("Column 2:type", "Integer");
    h.go("Column 2 primary key");
    h.go("Add column");
    h.append("Column 3:name", "tracking");
    h.go("Column 3:unique");
    h.go("Add column");
    h.append("Column 4:name", "weight");
    h.pick("Column 4:type", "Decimal (precision, scale)");
    h.go("Relationship columns: order_id");
    h.pick("Target table", "orders");
    h.pick("Target column for order_id", "id");
    h.pick("On delete", "CASCADE");
    h.go("Add relationship");
    h.append("Check expression", "weight > 0");
    h.go("Add check");
    h.append("Index name", "by_tracking");
    h.go("Index columns: tracking");
    h.go("Add index");
    assert!(h.exists("create-table-heading"));
    h.capture(
        "schema-01-create-table",
        &[
            "The Create table form lists four typed columns with key, required, and unique toggles.",
            "The staged relationship, check, and index appear in their sections before creation.",
        ],
    );

    h.go("submit-create-table");
    assert_eq!(h.read_shell(|s, _| s.selection.clone()).map(|s| s.id).as_deref(), Some("shipments"));
    let schema = inspect(&h, "shipments");
    assert_eq!(schema.primary_key, ["order_id", "parcel"]);
    assert_eq!(schema.foreign_keys.iter().map(|f| f.target_table.as_str()).collect::<Vec<_>>(), ["orders"]);
    assert!(schema.indexes.iter().any(|i| i.name == "by_tracking"));

    h.click("table:orders");
    h.click("design-table");
    assert_eq!(h.label("design-heading"), "Design orders");
    h.append("New column:name", "due_on");
    h.go("Add column");
    h.go("Column note:required");
    h.go("stage-column:note");
    assert!(h.exists("pending-op:1"));
    h.capture(
        "schema-02-table-designer",
        &[
            "The designer shows the orders columns with type, required, unique, and default controls.",
            "Pending changes label the in-place column add and the rebuild for the required change.",
        ],
    );

    click_designer_in(&mut h, "Pending changes", "apply-changes");
    assert!(h.exists("impact-dialog"));
    h.capture(
        "schema-03-impact-preview",
        &[
            "The impact preview dialog lists affected rows and dependent tables.",
            "Generated SQL is visible and Apply stays disabled until the risk is acknowledged.",
        ],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn schema_relationships() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, (1440.0, 900.0));
    h.click("new-document");
    seed_sales(&mut h);
    make_table(
        &h,
        "shipments",
        json!([
            { "name": "order_id", "declaredType": "INTEGER", "nullable": false, "primaryKeyPosition": 1 },
            { "name": "parcel", "declaredType": "INTEGER", "nullable": false, "primaryKeyPosition": 2 },
            { "name": "tracking", "declaredType": "TEXT" },
        ]),
        json!([{ "columns": ["order_id"], "targetTable": "orders", "targetColumns": ["id"] }]),
    );
    h.database_changed();
    for table in ["customers", "orders", "order_items", "shipments"] {
        assert!(h.exists(format!("node:{table}")), "{table} is on the canvas");
    }
    h.capture(
        "schema-04-relationships",
        &[
            "All four tables appear with their key and foreign-key fields marked.",
            "Edges connect orders to customers, order_items to orders, and shipments to orders.",
            "Tables sit in dependency columns (customers, then orders, then order_items and shipments), so no edge runs behind a table.",
        ],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn schema_rename_and_relationship_previews() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, TALL);
    h.click("new-document");
    seed_sales(&mut h);
    crate::save_query(h.label.clone(), None, "Customer cities".into(), "SELECT name, city FROM customers".into(), None)
        .expect("save query");
    make_table(
        &h,
        "deliveries",
        json!([
            { "name": "id", "declaredType": "INTEGER", "primaryKeyPosition": 1 },
            { "name": "order_id", "declaredType": "INTEGER" },
            { "name": "courier", "declaredType": "TEXT" },
        ]),
        json!([]),
    );
    h.database_changed();
    assert!(h.exists("table:deliveries"));

    h.click("table:customers");
    h.click("design-table");
    h.enter("Table name", "clients");
    h.go("rename-table");
    click_designer_in(&mut h, "Pending changes", "apply-changes");
    assert_eq!(h.label("impact-fact:dependents"), "query \u{201c}Customer cities\u{201d}");
    assert!(h.label("impact-warning:0").contains("does not update definitions"));
    h.capture(
        "schema-05-rename-impact",
        &[
            "The rename dialog lists the query \u{201c}Customer cities\u{201d} as a definition that still uses customers.",
            "A warning says renaming does not update definitions; Apply stays disabled until acknowledged.",
        ],
    );
    h.click("impact-cancel");
    click_designer_in(&mut h, "Pending changes", "discard-changes");
    h.go("close-designer");

    h.click("new-relationship");
    h.pick("Referencing table", "deliveries");
    h.go("Relationship columns: order_id");
    h.pick("Target table", "orders");
    h.pick("Target column for order_id", "id");
    h.pick("On delete", "CASCADE");
    h.click("Preview relationship");
    assert_eq!(h.label("impact-dialog"), "Relate deliveries (order_id) to orders?");
    h.capture(
        "schema-06-relationship-preview",
        &[
            "The preview dialog asks 'Relate deliveries (order_id) to orders?' with the generated SQL.",
            "Create relationship stays disabled until the acknowledgement is checked; Cancel is offered.",
        ],
    );
    h.click("impact-cancel");
    assert!(h.exists("Preview relationship"));
    h.capture(
        "schema-07-relationship-editor-kept",
        &["After Cancel the New relationship editor is back with deliveries.order_id to orders.id and On delete CASCADE kept."],
    );
}
