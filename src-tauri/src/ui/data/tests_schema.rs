//! Schema designer flows (ports of tests/integration/schema-designer.test.tsx,
//! schema-designer-refresh.test.tsx and the designer part of table-workflow.test.tsx).
//! Imports are explicit: `gpui_kit::*` would shadow `#[test]`.

use super::testkit::{click_designer_in, data_mode, exists_in, inspect, label_in};
use crate::{
    data::DataValue,
    ui::testing::{shown, Harness},
};
use gpui_kit::{test::TestWindowExt as _, TestAppContext};
use serde_json::json;

fn table(h: &mut Harness, name: &str, columns: serde_json::Value) {
    let spec = json!({ "name": name, "columns": columns });
    crate::create_database_table(h.label.clone(), serde_json::from_value(spec).expect("spec"))
        .expect("create table");
    h.database_changed();
}

fn id_column() -> serde_json::Value {
    json!({ "name": "id", "declaredType": "INTEGER", "nullable": false, "primaryKeyPosition": 1 })
}

fn alter(h: &Harness, table: &str, operations: serde_json::Value) {
    crate::recordstore::commands::apply_table_changes(
        h.label.clone(),
        table.into(),
        serde_json::from_value(operations).expect("operations"),
    )
    .expect("alter table");
}

fn column_names(h: &Harness, table: &str) -> Vec<String> {
    inspect(h, table).columns.iter().map(|c| c.name.clone()).collect()
}

fn open_designer(h: &mut Harness, name: &str) {
    h.go(&format!("table:{name}"));
    h.go("design-table");
    assert_eq!(h.label("design-heading"), format!("Design {name}"));
}

fn apply_staged(h: &mut Harness) {
    click_designer_in(h, "Pending changes", "apply-changes");
}

// Confirms the impact dialog the way a user does: acknowledge, then confirm.
fn acknowledge_and_confirm(h: &mut Harness) {
    h.go("impact-ack");
    h.go("impact-confirm");
}

#[gpui_kit::test]
fn creates_a_table_with_a_composite_key_relationship_unique_check_and_index(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    table(&mut h, "regions", json!([
        { "name": "code", "declaredType": "TEXT", "nullable": false, "primaryKeyPosition": 1 },
        { "name": "title", "declaredType": "TEXT" },
    ]));
    h.insert_row("regions", &[("code", DataValue::Text("N".into()))]);

    h.go("new-table");
    h.enter("Table name", "sites");
    h.enter("Column 1:name", "region");
    h.pick("Column 1:type", "Text");
    h.enter("Column 2:name", "num");
    h.pick("Column 2:type", "Integer");
    h.go("Column 2 primary key");
    h.go("Add column");
    h.append("Column 3:name", "label");
    h.go("Column 3:unique");
    h.go("Add column");
    h.append("Column 4:name", "price");
    h.pick("Column 4:type", "Decimal (precision, scale)");

    // Both sections start collapsed with a hint.
    assert_eq!(h.label("create-relationships-summary"), "None yet. Expand to link this table to another.");
    assert!(!h.exists("Relationship columns: region"));
    h.go("create-relationships-toggle");
    h.go("create-constraints-toggle");
    h.go("Relationship columns: region");
    h.pick("Target table", "regions");
    h.pick("Target column for region", "code");
    h.pick("On delete", "CASCADE");
    h.go("Add relationship");
    h.append("Check expression", "num > 0");
    h.go("Add check");
    h.append("Index name", "sites_by_price");
    h.go("Index columns: price");
    h.go("Index columns: num");
    h.go("Add index");
    h.go("submit-create-table");

    assert_eq!(h.read_shell(|s, _| s.selection.clone()).map(|s| s.id).as_deref(), Some("sites"));
    let schema = inspect(&h, "sites");
    assert_eq!(schema.primary_key, ["region", "num"]);
    let types: Vec<String> = schema.columns.iter().map(|c| c.logical_type.to_string()).collect();
    assert_eq!(types, ["text", "integer", "text", "decimal(10,2)"]);
    let fk = &schema.foreign_keys[0];
    assert_eq!(fk.from_columns, ["region"]);
    assert_eq!(fk.target_table, "regions");
    assert_eq!(fk.target_columns, ["code"]);
    assert_eq!(fk.on_delete, "CASCADE");
    assert_eq!(schema.uniques[0].columns, ["label"]);
    assert_eq!(schema.checks[0].expression, "num > 0");
    assert_eq!(schema.indexes[0].name, "sites_by_price");
    assert_eq!(schema.indexes[0].columns, ["price", "num"]);
    assert!(!schema.indexes[0].unique);

    let insert = |values: Vec<(&str, DataValue)>| {
        crate::recordstore::commands::insert_row(
            h.label.clone(),
            "sites".into(),
            values
                .into_iter()
                .map(|(column, value)| crate::data::NamedValue { column: column.into(), value })
                .collect(),
            None,
        )
    };
    let missing = insert(vec![("region", DataValue::Text("missing".into())), ("num", DataValue::Integer(1))]);
    assert!(missing.unwrap_err().message.contains("Foreign key constraint failed"));
    let zero = insert(vec![("region", DataValue::Text("N".into())), ("num", DataValue::Integer(0))]);
    assert!(zero.unwrap_err().message.contains("Check constraint failed"));
    insert(vec![
        ("region", DataValue::Text("N".into())),
        ("num", DataValue::Integer(1)),
        ("price", DataValue::Text("12.5".into())),
    ])
    .expect("valid row");
    assert_eq!(shown(&h.read_page("sites").rows[0][3]), "12.50");
}

#[gpui_kit::test]
fn labels_each_staged_change_by_store_capability_and_previews_rebuilds_and_drops(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    table(&mut h, "parts", json!([id_column(), { "name": "label", "declaredType": "TEXT" }]));
    table(&mut h, "bins", json!([id_column()]));
    alter(
        &h,
        "bins",
        json!([
            { "operation": "add_column", "column": { "name": "part", "logicalType": "integer", "nullable": true } },
            { "operation": "add_foreign_key", "foreignKey": { "columns": ["part"], "targetTable": "parts", "targetColumns": ["id"] } },
        ]),
    );
    h.insert_row("parts", &[("id", DataValue::Integer(1)), ("label", DataValue::Text("bolt".into()))]);
    h.insert_row("bins", &[("part", DataValue::Integer(1))]);
    h.database_changed();
    open_designer(&mut h, "parts");

    h.append("New column:name", "note");
    h.go("Add column");
    h.go("Column label:required");
    h.go("stage-column:label");
    assert_eq!(label_in(&mut h, "Pending changes", "pending-mode:0"), "Changes in place");
    assert_eq!(label_in(&mut h, "Pending changes", "pending-mode:1"), "Requires table rebuild");

    apply_staged(&mut h);
    assert_eq!(h.label("impact-fact:rows"), "1");
    assert!(h.label("impact-fact:referencing").contains("bins (part): 1 row"));
    h.click("impact-show-sql");
    assert!(h.label("impact-sql").contains("ALTER TABLE \"_ixtable_rebuild_parts\" RENAME TO \"parts\""));
    // Without the acknowledgement the confirmation does nothing.
    h.go("impact-confirm");
    assert_eq!(column_names(&h, "parts"), ["id", "label"]);
    acknowledge_and_confirm(&mut h);
    let schema = inspect(&h, "parts");
    let columns: Vec<(String, bool)> = schema.columns.iter().map(|c| (c.name.clone(), c.nullable)).collect();
    assert_eq!(
        columns,
        [("id".to_string(), false), ("label".to_string(), false), ("note".to_string(), true)]
    );
    let row = &h.read_page("parts").rows[0];
    assert_eq!((shown(&row[0]), shown(&row[1]), &row[2]), ("1".to_string(), "bolt".to_string(), &DataValue::Null));
    assert_eq!(inspect(&h, "bins").foreign_keys[0].target_table, "parts");

    h.go("design-table");
    h.go("drop-table");
    assert_eq!(h.label("impact-dialog"), "Drop table parts?");
    assert_eq!(h.label("impact-fact:rows"), "1");
    h.click("impact-show-sql");
    assert_eq!(h.label("impact-sql"), "DROP TABLE \"parts\";");
    h.go("impact-cancel");
    assert!(!h.exists("impact-dialog"));
    assert_eq!(h.read_page("parts").total, 1);
}

#[gpui_kit::test]
fn keeps_designer_drafts_through_a_metadata_reload_and_previews_renames_and_index_drops(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    table(&mut h, "parts", json!([id_column(), { "name": "label", "declaredType": "TEXT" }]));
    crate::recordstore::commands::create_index(
        h.label.clone(),
        serde_json::from_value(json!({ "name": "parts_label", "table": "parts", "columns": ["label"], "unique": false }))
            .expect("index"),
    )
    .expect("create index");
    crate::save_query(h.label.clone(), None, "Labels".into(), "SELECT label FROM parts".into(), None)
        .expect("save query");
    h.database_changed();
    open_designer(&mut h, "parts");
    assert!(h.label("designer-store").contains("SQLite record store."));

    h.append("Column label:default", "'none'");
    h.database_changed();
    assert_eq!(h.label("design-heading"), "Design parts");
    assert_eq!(h.find("Column label:default").value(), Some("'none'"));

    h.enter("Table name", "items");
    h.go("rename-table");
    assert_eq!(label_in(&mut h, "Pending changes", "pending-mode:0"), "Changes in place");
    apply_staged(&mut h);
    assert_eq!(h.label("impact-fact:dependents"), "query \u{201c}Labels\u{201d}");
    assert!(h.label("impact-warning:0").contains("does not update definitions"));
    h.go("impact-confirm");
    assert!(inspect(&h, "parts").name == "parts", "rename waits for the acknowledgement");
    h.go("impact-ack");
    h.go("impact-cancel");
    click_designer_in(&mut h, "Pending changes", "discard-changes");

    assert_eq!(h.label("designer-indexes-summary"), "parts_label");
    h.go("designer-indexes-toggle");
    h.go("drop-index:parts_label");
    assert_eq!(h.label("impact-dialog"), "Drop index parts_label?");
    h.click("impact-show-sql");
    assert_eq!(h.label("impact-sql"), "DROP INDEX \"parts_label\";");
    h.go("impact-confirm");
    h.go("impact-cancel");
    assert_eq!(inspect(&h, "parts").indexes.len(), 1);
    h.go("drop-index:parts_label");
    h.go("impact-ack");
    h.go("impact-confirm");
    assert!(inspect(&h, "parts").indexes.is_empty());
    assert_eq!(column_names(&h, "parts"), ["id", "label"]);
}

#[gpui_kit::test]
fn offers_the_referential_actions_of_the_store(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    table(&mut h, "parts", json!([id_column(), { "name": "label", "declaredType": "TEXT" }]));
    open_designer(&mut h, "parts");
    assert_eq!(h.label("designer-relationships-summary"), "None");
    h.go("designer-relationships-toggle");
    assert!(exists_in(&mut h, "designer-relationships-section", "On delete"));
    h.go("On delete");
    let labels = super::testkit::menu_labels(&mut h);
    assert_eq!(labels, ["NO ACTION", "RESTRICT", "CASCADE", "SET NULL", "SET DEFAULT"]);
}

fn two_tables(h: &mut Harness) {
    table(h, "parts", json!([id_column()]));
    table(h, "bins", json!([id_column(), { "name": "part", "declaredType": "INTEGER" }]));
}

fn fill_relationship(h: &mut Harness) {
    h.go("new-relationship");
    h.pick("Referencing table", "bins");
    h.go("Relationship columns: part");
    h.pick("Target table", "parts");
    h.pick("Target column for part", "id");
    h.pick("On delete", "CASCADE");
    h.go("Preview relationship");
}

#[gpui_kit::test]
fn creates_a_relationship_from_the_diagram_through_the_prefilled_relationship_editor(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    two_tables(&mut h);
    fill_relationship(&mut h);
    assert_eq!(h.label("impact-dialog"), "Relate bins (part) to parts?");
    h.go("impact-ack");
    h.go("impact-confirm");
    let fk = &inspect(&h, "bins").foreign_keys[0];
    assert_eq!(fk.from_columns, ["part"]);
    assert_eq!(fk.target_table, "parts");
    assert_eq!(fk.target_columns, ["id"]);
    assert_eq!(fk.on_delete, "CASCADE");
    assert!(!h.exists("impact-dialog"));
    assert!(!h.exists("Referencing table"));
}

#[gpui_kit::test]
fn returns_to_the_relationship_editor_with_its_choices_when_the_preview_is_cancelled(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    two_tables(&mut h);
    fill_relationship(&mut h);
    assert_eq!(h.label("impact-dialog"), "Relate bins (part) to parts?");
    h.go("impact-cancel");
    assert!(!h.exists("impact-dialog"));
    let dialog = data_mode(&mut h);
    let relating = h.cx.read(|cx| dialog.read(cx).relating().cloned()).expect("the editor is still open");
    let draft = h.cx.read(|cx| relating.read(cx).draft().cloned()).expect("the choices are kept");
    assert_eq!(draft.columns, ["part"]);
    assert_eq!(draft.target_table, "parts");
    assert_eq!(draft.on_delete.as_deref(), Some("CASCADE"));
    // The editor is back with its choices: previewing again needs no new input.
    h.go("Preview relationship");
    assert_eq!(h.label("impact-dialog"), "Relate bins (part) to parts?");
    h.go("impact-cancel");
    h.go("relate-cancel");
    assert!(!h.exists("Referencing table"));
    assert!(inspect(&h, "bins").foreign_keys.is_empty());
}

#[gpui_kit::test]
fn keeps_an_existing_column_check_when_the_column_is_edited_in_the_designer(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    table(&mut h, "stock", json!([id_column(), { "name": "qty", "declaredType": "INTEGER", "check": "qty > 0" }]));
    open_designer(&mut h, "stock");
    assert_eq!(h.find("Column qty:check").value(), Some("qty > 0"));
    h.go("Column qty:required");
    h.go("stage-column:qty");
    apply_staged(&mut h);
    acknowledge_and_confirm(&mut h);
    let schema = inspect(&h, "stock");
    assert!(!schema.columns.iter().find(|c| c.name == "qty").unwrap().nullable);
    assert_eq!(schema.checks[0].expression, "qty > 0");

    h.go("design-table");
    assert_eq!(h.find("Column qty:check").value(), Some("qty > 0"));
    h.enter("Column qty:check", "qty < 10");
    h.go("stage-column:qty");
    apply_staged(&mut h);
    acknowledge_and_confirm(&mut h);
    assert_eq!(inspect(&h, "stock").checks[0].expression, "qty < 10");
}

#[gpui_kit::test]
fn disables_the_table_designer_while_its_schema_reloads(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    table(&mut h, "parts", json!([id_column()]));
    open_designer(&mut h, "parts");
    let mode = data_mode(&mut h);
    let designer = h.cx.read(|cx| mode.read(cx).designer().cloned()).expect("designer");
    assert!(!h.cx.read(|cx| designer.read(cx).locked()));

    // The reload starts but its answer has not arrived yet.
    let shell = h.shell();
    h.update(|_, cx| shell.update(cx, |s, cx| s.database_changed(cx)));
    assert!(!h.cx.read(|cx| mode.read(cx).schemas_fresh()));
    assert!(h.cx.read(|cx| designer.read(cx).locked()));
    h.update(|window, cx| window.render_frame(cx));
    assert_eq!(h.label("design-refreshing"), "Refreshing schema\u{2026}");

    h.settle();
    assert!(h.cx.read(|cx| mode.read(cx).schemas_fresh()));
    assert!(!h.cx.read(|cx| designer.read(cx).locked()));
    assert!(!h.exists("design-refreshing"));
}

#[gpui_kit::test]
fn designs_a_selected_table_with_staged_alterations_and_an_impact_preview_for_drops(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    table(&mut h, "inventory", json!([id_column(), { "name": "label", "declaredType": "TEXT" }]));
    open_designer(&mut h, "inventory");
    h.enter("Table name", "products");
    h.go("rename-table");
    h.enter("Column label:name", "title");
    h.go("stage-column:label");
    h.append("New column:name", "price");
    h.pick("New column:type", "Number (floating point)");
    h.go("Add column");
    for i in 0..3 {
        assert_eq!(label_in(&mut h, "Pending changes", &format!("pending-mode:{i}")), "Changes in place");
    }
    apply_staged(&mut h);
    assert!(!h.exists("design-heading"), "the designer closes once applied");
    assert!(h.exists("table:products"));
    assert_eq!(h.read_shell(|s, _| s.selection.clone()).map(|s| s.id).as_deref(), Some("products"));
    assert_eq!(column_names(&h, "products"), ["id", "title", "price"]);

    h.go("design-table");
    h.go("drop-column:price");
    apply_staged(&mut h);
    assert_eq!(h.label("impact-op-destructive:0"), "Destructive");
    h.go("impact-cancel");
    assert_eq!(column_names(&h, "products").len(), 3);
    apply_staged(&mut h);
    acknowledge_and_confirm(&mut h);
    assert_eq!(column_names(&h, "products"), ["id", "title"]);
}
