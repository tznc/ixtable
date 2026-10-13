//! Port of tests/unit/forms-generate.test.ts.

use super::{GenerateOptions, generate_crud_forms, humanize, kind_for_declared};
use crate::data::{Column, LogicalType, TableSchema};
use crate::design::{Control, ControlKind, FormMode, NavKind, SourceKind};
use crate::forms::schema::is_input_kind;
use crate::forms::testkit::{auto, col, fk, not_null, pk, table};

fn customers() -> TableSchema {
    table(
        "customers",
        vec![
            auto(not_null(pk(col("id", "INTEGER"), 1))),
            not_null(col("name", "TEXT")),
        ],
        vec![],
    )
}

fn orders() -> TableSchema {
    let mut paid = not_null(col("paid", "BOOLEAN"));
    paid.default_value = Some("0".into());
    table(
        "orders",
        vec![
            auto(not_null(pk(col("id", "INTEGER"), 1))),
            not_null(col("customer_id", "INTEGER")),
            col("placed_on", "DATE"),
            col("total", "DECIMAL(10,2)"),
            paid,
            col("photo", "BLOB"),
        ],
        vec![fk(&["customer_id"], "customers", &["id"])],
    )
}

fn items() -> TableSchema {
    table(
        "order_items",
        vec![
            pk(col("id", "INTEGER"), 1),
            col("order_id", "INTEGER"),
            col("product", "TEXT"),
            col("qty", "INT"),
        ],
        vec![fk(&["order_id"], "orders", &["id"])],
    )
}

fn thresholds() -> TableSchema {
    table(
        "reorder_thresholds",
        vec![
            not_null(pk(col("product_id", "INTEGER"), 1)),
            not_null(pk(col("location_id", "INTEGER"), 2)),
            col("label", "TEXT"),
        ],
        vec![],
    )
}

fn counts() -> TableSchema {
    table(
        "stock_counts",
        vec![
            pk(col("id", "INTEGER"), 1),
            not_null(col("product_id", "INTEGER")),
            not_null(col("location_id", "INTEGER")),
            col("counted", "INTEGER"),
        ],
        vec![fk(
            &["product_id", "location_id"],
            "reorder_thresholds",
            &["product_id", "location_id"],
        )],
    )
}

fn options<'a>(targets: &[&'a TableSchema], children: &[&'a TableSchema]) -> GenerateOptions<'a> {
    GenerateOptions {
        targets: targets.iter().map(|t| (t.name.clone(), *t)).collect(),
        children: children.to_vec(),
        ..Default::default()
    }
}

fn control_for<'a>(controls: &'a [Control], column: &str) -> &'a Control {
    controls
        .iter()
        .find(|c| c.binding.as_ref().is_some_and(|b| b.column == column))
        .unwrap_or_else(|| panic!("no control bound to {column}"))
}

fn declared(declared: &str, logical: Option<&str>) -> Option<ControlKind> {
    let logical: Option<LogicalType> = logical.map(|l| l.parse().unwrap());
    kind_for_declared(declared, logical.as_ref())
}

#[test]
fn maps_declared_types_to_control_kinds() {
    assert_eq!(declared("VARCHAR(20)", None), Some(ControlKind::Text));
    assert_eq!(declared("BIGINT", None), Some(ControlKind::Number));
    assert_eq!(declared("NUMERIC", None), Some(ControlKind::Decimal));
    assert_eq!(declared("TIMESTAMP", None), Some(ControlKind::Datetime));
    assert_eq!(declared("TIME", None), Some(ControlKind::Time));
    assert_eq!(declared("BLOB", None), None);
    assert_eq!(humanize("order_items"), "Order items");
}

#[test]
fn builds_a_list_form_and_a_detail_form_from_the_same_primitives() {
    let (customers, orders, items) = (customers(), orders(), items());
    let crud = generate_crud_forms(&orders, &options(&[&customers], &[&items]));
    let (list, detail, navigation) = (crud.list, crud.detail, crud.navigation);
    assert_eq!(list.modes, [FormMode::List]);
    assert_eq!(list.detail_form_id.as_deref(), Some(detail.id.as_str()));
    assert_eq!(
        list.list_columns,
        ["id", "customer_id", "placed_on", "total", "paid"]
    );
    assert_eq!(
        detail.modes,
        [FormMode::Detail, FormMode::Create, FormMode::Edit]
    );
    let source = detail.source.as_ref().unwrap();
    assert_eq!(source.kind, SourceKind::Table);
    assert_eq!(source.table.as_deref(), Some("orders"));
    assert_eq!(navigation.kind, NavKind::Form);
    assert_eq!(navigation.target_id.as_deref(), Some(list.id.as_str()));
    assert_eq!(navigation.label, "Orders");

    let id = control_for(&detail.controls, "id");
    assert_eq!(id.kind, ControlKind::Number);
    assert!(id.read_only && !id.validation.required);
    let customer = control_for(&detail.controls, "customer_id");
    assert_eq!(customer.kind, ControlKind::Relationship);
    assert_eq!(customer.label, "Customer");
    assert!(customer.validation.required);
    let rel = customer.relationship.as_ref().unwrap();
    assert_eq!(
        (rel.table.as_str(), rel.value_column.as_str(), rel.display_column.as_str()),
        ("customers", "id", "name")
    );
    assert_eq!(control_for(&detail.controls, "placed_on").kind, ControlKind::Date);
    assert_eq!(control_for(&detail.controls, "total").kind, ControlKind::Decimal);
    let paid = control_for(&detail.controls, "paid");
    assert_eq!(paid.kind, ControlKind::Boolean);
    assert!(!paid.validation.required);
    assert!(
        detail
            .controls
            .iter()
            .all(|c| c.binding.as_ref().is_none_or(|b| b.column != "photo"))
    );
    let list_control = detail
        .controls
        .iter()
        .find(|c| c.kind == ControlKind::RelatedList)
        .unwrap();
    assert_eq!(list_control.label, "Order items");
    let related = list_control.related.as_ref().unwrap();
    assert_eq!(
        (
            related.table.as_str(),
            related.foreign_key.as_str(),
            related.parent_column.as_str()
        ),
        ("order_items", "order_id", "id")
    );
    assert_eq!(related.columns, ["id", "product", "qty"]);
    let cells: Vec<(u16, u16)> = detail
        .controls
        .iter()
        .filter(|c| is_input_kind(c.kind))
        .take(3)
        .map(|c| (c.placement.column, c.placement.row))
        .collect();
    assert_eq!(cells, [(1, 1), (7, 1), (1, 2)]);
}

#[test]
fn prefers_the_logical_type_inspect_table_reports_over_the_declared_type() {
    assert_eq!(declared("character varying", Some("text")), Some(ControlKind::Text));
    assert_eq!(declared("double precision", Some("real")), Some(ControlKind::Decimal));
    assert_eq!(declared("numeric(10,2)", Some("decimal(10,2)")), Some(ControlKind::Decimal));
    assert_eq!(
        declared("timestamp without time zone", Some("timestamp")),
        Some(ControlKind::Datetime)
    );
    assert_eq!(declared("time without time zone", Some("time")), Some(ControlKind::Time));
    assert_eq!(declared("point", Some("integer")), Some(ControlKind::Number));
    assert_eq!(declared("bytea", Some("blob")), None);
    assert_eq!(declared("uuid", Some("uuid")), Some(ControlKind::Text));
    assert_eq!(declared("INTEGER", None), Some(ControlKind::Number));
}

#[test]
fn treats_only_a_key_the_catalog_reports_as_auto_increment_as_automatic() {
    let pg = |declared: &str, logical: &str, auto_increment: bool| {
        let mut id: Column = not_null(pk(col("id", declared), 1));
        id.logical_type = logical.parse().unwrap();
        id.auto_increment = auto_increment;
        let mut name = col("name", "text");
        name.logical_type = LogicalType::Text;
        table("t", vec![id, name], vec![])
    };
    let id_control = |schema: &TableSchema| {
        let detail = generate_crud_forms(schema, &GenerateOptions::default()).detail;
        control_for(&detail.controls, "id").clone()
    };
    let automatic = id_control(&pg("bigint", "integer", true));
    assert!(automatic.read_only && !automatic.validation.required);
    let plain = id_control(&pg("integer", "integer", false));
    assert!(plain.validation.required && !plain.read_only);
    assert!(!id_control(&pg("INTEGER", "integer", false)).read_only);
    assert!(!id_control(&pg("text", "text", false)).read_only);
}

#[test]
fn targets_the_parents_primary_key_when_a_foreign_key_names_no_target_columns() {
    let thresholds = thresholds();
    let mut unnamed = counts();
    unnamed.foreign_keys = vec![fk(&["product_id", "location_id"], "reorder_thresholds", &[])];
    let detail = generate_crud_forms(&unnamed, &options(&[&thresholds], &[])).detail;
    let rel = detail
        .controls
        .iter()
        .find(|c| c.kind == ControlKind::Relationship)
        .unwrap()
        .relationship
        .clone()
        .unwrap();
    assert_eq!(rel.value_column, "product_id");
    assert_eq!(
        rel.keys
            .iter()
            .map(|k| (k.column.as_str(), k.target.as_str()))
            .collect::<Vec<_>>(),
        [("product_id", "product_id"), ("location_id", "location_id")]
    );
    let codes = table("codes", vec![pk(col("code", "TEXT"), 1), col("label", "TEXT")], vec![]);
    let uses = table(
        "uses",
        vec![pk(col("id", "INTEGER"), 1), col("code_ref", "TEXT")],
        vec![fk(&["code_ref"], "codes", &[])],
    );
    let single = generate_crud_forms(&uses, &options(&[&codes], &[])).detail;
    let rel = single
        .controls
        .iter()
        .find(|c| c.kind == ControlKind::Relationship)
        .unwrap()
        .relationship
        .clone()
        .unwrap();
    assert_eq!(
        (rel.table.as_str(), rel.value_column.as_str(), rel.display_column.as_str()),
        ("codes", "code", "label")
    );
    assert!(rel.keys.is_empty() && rel.filter.is_none());
}

#[test]
fn prefers_a_multi_column_selector_when_its_first_column_also_has_its_own_key() {
    let products = table(
        "products",
        vec![pk(col("id", "INTEGER"), 1), col("name", "TEXT")],
        vec![],
    );
    let thresholds = thresholds();
    let mut both = counts();
    both.foreign_keys = vec![
        fk(&["product_id"], "products", &["id"]),
        fk(
            &["product_id", "location_id"],
            "reorder_thresholds",
            &["product_id", "location_id"],
        ),
    ];
    let detail = generate_crud_forms(&both, &options(&[&products, &thresholds], &[])).detail;
    let selectors: Vec<&Control> = detail
        .controls
        .iter()
        .filter(|c| c.kind == ControlKind::Relationship)
        .collect();
    assert_eq!(selectors.len(), 1);
    assert_eq!(selectors[0].relationship.as_ref().unwrap().table, "reorder_thresholds");
    assert!(
        detail
            .controls
            .iter()
            .all(|c| c.binding.as_ref().is_none_or(|b| b.column != "location_id"))
    );
}

#[test]
fn generates_one_selector_that_writes_every_column_of_a_multi_column_key() {
    let thresholds = thresholds();
    let counts = counts();
    let detail = generate_crud_forms(&counts, &options(&[&thresholds], &[])).detail;
    let selectors: Vec<&Control> = detail
        .controls
        .iter()
        .filter(|c| c.kind == ControlKind::Relationship)
        .collect();
    assert_eq!(selectors.len(), 1);
    let selector = selectors[0];
    assert_eq!(selector.binding.as_ref().unwrap().column, "product_id");
    assert_eq!(selector.label, "Reorder threshold");
    assert!(selector.validation.required);
    let rel = selector.relationship.as_ref().unwrap();
    assert_eq!(
        (rel.table.as_str(), rel.value_column.as_str(), rel.display_column.as_str()),
        ("reorder_thresholds", "product_id", "label")
    );
    assert_eq!(rel.keys.len(), 2);
    assert!(
        detail
            .controls
            .iter()
            .all(|c| c.binding.as_ref().is_none_or(|b| b.column != "location_id"))
    );
}

#[test]
fn generates_a_related_list_that_matches_on_every_key_column() {
    let (thresholds, counts) = (thresholds(), counts());
    let detail = generate_crud_forms(&thresholds, &options(&[], &[&counts])).detail;
    let related = detail
        .controls
        .iter()
        .find(|c| c.kind == ControlKind::RelatedList)
        .unwrap()
        .related
        .clone()
        .unwrap();
    assert_eq!(related.table, "stock_counts");
    assert_eq!(related.foreign_key, "product_id");
    assert_eq!(related.parent_column, "product_id");
    assert_eq!(
        related
            .keys
            .iter()
            .map(|k| (k.column.as_str(), k.target.as_str()))
            .collect::<Vec<_>>(),
        [("product_id", "product_id"), ("location_id", "location_id")]
    );
    assert_eq!(related.columns, ["id", "counted"]);
    assert_eq!(related.form_id, None);
}

#[test]
fn keeps_single_column_relationships_in_their_original_shape() {
    let (customers, orders, items) = (customers(), orders(), items());
    let detail = generate_crud_forms(&orders, &options(&[&customers], &[&items])).detail;
    for control in &detail.controls {
        assert!(control.relationship.as_ref().is_none_or(|r| r.keys.is_empty()));
        assert!(control.related.as_ref().is_none_or(|r| r.keys.is_empty()));
    }
}

#[test]
fn falls_back_to_the_key_column_when_the_lookup_table_is_unknown() {
    let detail = generate_crud_forms(&orders(), &GenerateOptions::default()).detail;
    let rel = detail
        .controls
        .iter()
        .find(|c| c.kind == ControlKind::Relationship)
        .unwrap()
        .relationship
        .clone()
        .unwrap();
    assert_eq!(rel.display_column, "id");
}
