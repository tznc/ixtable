//! Ports of tests/unit/schema-logical.test.ts and schema-columns.test.ts, and the
//! store-language parts of store-capabilities-ui.test.tsx.

use super::{
    columns::{column_check, draft_from_column},
    logical::{
        LogicalParts, format_logical, mode_label, parse_logical, store_label, store_types,
        value_from_text,
    },
    testkit::{column, schema},
};
use crate::data::DataValue;
use crate::recordstore::capabilities::{self, StoreCapabilities};

#[test]
fn parses_and_formats_decimal_precision_and_scale() {
    assert_eq!(
        parse_logical(Some("decimal(12, 3)")),
        LogicalParts {
            base: "decimal".into(),
            precision: 12,
            scale: 3
        }
    );
    let decimal = LogicalParts {
        base: "decimal".into(),
        precision: 8,
        scale: 1,
    };
    assert_eq!(format_logical(&decimal), "decimal(8,1)");
    let date = LogicalParts {
        base: "date".into(),
        precision: 10,
        scale: 2,
    };
    assert_eq!(format_logical(&date), "date");
}

#[test]
fn turns_typed_text_into_values_and_reports_values_that_do_not_fit() {
    assert_eq!(
        value_from_text("42", "qty", "integer").unwrap(),
        DataValue::Integer(42)
    );
    assert_eq!(
        value_from_text("12.50", "price", "decimal(10,2)").unwrap(),
        DataValue::Decimal("12.50".into())
    );
    assert_eq!(
        value_from_text("yes", "done", "boolean").unwrap(),
        DataValue::Boolean(true)
    );
    assert_eq!(
        value_from_text("NULL", "any", "text").unwrap(),
        DataValue::Null
    );
    assert_eq!(
        value_from_text("4.5", "qty", "integer").unwrap_err(),
        "qty requires an integer"
    );
    assert!(value_from_text("abc", "price", "decimal(10,2)")
        .unwrap_err()
        .contains("decimal number"));
    assert!(value_from_text("9007199254740993", "id", "integer")
        .unwrap_err()
        .contains("safe integer"));
    assert_eq!(
        value_from_text("not-a-number", "id", "integer").unwrap_err(),
        "id requires an integer"
    );
    assert_eq!(
        value_from_text("2.5", "score", "real").unwrap(),
        DataValue::Real(2.5)
    );
}

#[test]
fn speaks_store_capabilities_rather_than_sqlite_terms() {
    assert_eq!(mode_label("inPlace"), "Changes in place");
    assert_eq!(mode_label("rebuild"), "Requires table rebuild");
    assert_eq!(mode_label("unsupported"), "Not supported by this store");
}

#[test]
fn names_the_store_only_once_capabilities_are_known() {
    let postgres: StoreCapabilities = capabilities::postgres();
    assert_eq!(store_label(None), None);
    assert_eq!(store_label(Some(&postgres)).as_deref(), Some("PostgreSQL"));
    assert_eq!(
        store_label(Some(&capabilities::sqlite())).as_deref(),
        Some("SQLite")
    );
    assert_eq!(store_types(None), None);
    let types = store_types(Some(&postgres)).expect("types");
    assert!(types.contains(&"text".to_string()));
    assert!(types.contains(&"decimal".to_string()));
    assert!(!types.iter().any(|t| t.contains('(')));
}

#[test]
fn shows_checks_that_mention_only_this_column_as_the_columns_check() {
    let s = schema(
        "t",
        vec![column("qty", "integer"), column("max", "integer")],
        &["qty > 0", "qty <= max", "\"MAX\" < 'qty'"],
    );
    assert_eq!(column_check(&s, "qty"), "qty > 0");
    assert_eq!(column_check(&s, "max"), "\"MAX\" < 'qty'");
    assert_eq!(draft_from_column(&s.columns[0], &s).check, "qty > 0");
}

#[test]
fn joins_several_checks_the_way_the_store_compares_them_and_is_empty_without_any() {
    let columns = || vec![column("qty", "integer"), column("max", "integer")];
    assert_eq!(
        column_check(&schema("t", columns(), &["qty > 0", " QTY < 9 "]), "qty"),
        "(qty > 0) AND (QTY < 9)"
    );
    assert_eq!(column_check(&schema("t", columns(), &[]), "qty"), "");
}

#[gpui_kit::test]
fn decimal_precision_and_scale_inputs_are_labelled(cx: &mut gpui_kit::TestAppContext) {
    let mut h = crate::ui::testing::Harness::new_document(cx);
    h.go("new-table");
    assert!(!h.exists("Column 1:precision-label"));
    h.pick("Column 1:type", "Decimal (precision, scale)");
    assert_eq!(h.label("Column 1:precision-label"), "Precision");
    assert_eq!(h.label("Column 1:scale-label"), "Scale");
}
