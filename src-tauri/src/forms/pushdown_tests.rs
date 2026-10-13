//! Port of tests/unit/pushdown.test.ts. `Filter` has no PartialEq, so filters compare by Debug.

use super::{Pushdown, pushdown};
use crate::data::{Column, DataValue, Filter, FilterOperator, LogicalType};
use crate::expr::{self, EvaluateOptions, Scope};
use crate::forms::testkit::col;
use serde_json::{Value, json};

fn column(name: &str, logical: LogicalType) -> Column {
    Column {
        logical_type: logical,
        ..col(name, "")
    }
}

fn columns() -> Vec<Column> {
    vec![
        column("amount", LogicalType::Real),
        column("qty", LogicalType::Integer),
        column("status", LogicalType::Text),
        column("price", LogicalType::Decimal { precision: Some(10), scale: 2 }),
        column("active", LogicalType::Boolean),
        column("due", LogicalType::Date),
    ]
}

fn scope() -> Scope {
    json!({"app": {"min": 10, "statuses": ["won", "lost"]}, "params": {}, "form": {}, "parent": null})
        .as_object()
        .unwrap()
        .clone()
}

fn split(src: &str) -> Pushdown {
    pushdown(&expr::parse(src).unwrap(), &scope(), &columns())
}

fn f(column: &str, operator: FilterOperator, value: Option<DataValue>) -> Filter {
    Filter {
        column: column.into(),
        operator,
        value,
        values: None,
    }
}

fn same(actual: &[Filter], expected: &[Filter]) {
    assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
}

fn rest_of(src: &str) -> Option<String> {
    split(src)
        .rest
        .map(|r| src.chars().skip(r.start).take(r.end - r.start).collect())
}

#[test]
fn pushes_numeric_comparisons_with_constants_either_side() {
    same(
        &split("record.amount >= app.min * 2").filters,
        &[f("amount", FilterOperator::Gte, Some(DataValue::Integer(20)))],
    );
    same(
        &split("100.5 > record.qty").filters,
        &[f("qty", FilterOperator::Lt, Some(DataValue::Real(100.5)))],
    );
    assert!(split("record.amount >= 1").rest.is_none());
}

#[test]
fn pushes_text_equality_null_checks_in_lists_and_between() {
    same(
        &split("record.status = \"won\" and record.due is not null").filters,
        &[
            f("status", FilterOperator::Eq, Some(DataValue::Text("won".into()))),
            f("due", FilterOperator::IsNotNull, None),
        ],
    );
    let list = split("record.status in (app.statuses)").filters;
    assert_eq!(list.len(), 1);
    assert_eq!(
        format!("{:?}", list[0]),
        format!(
            "{:?}",
            Filter {
                column: "status".into(),
                operator: FilterOperator::In,
                value: None,
                values: Some(vec![DataValue::Text("won".into()), DataValue::Text("lost".into())]),
            }
        )
    );
    same(
        &split("record.qty between 1 and 5").filters,
        &[
            f("qty", FilterOperator::Gte, Some(DataValue::Integer(1))),
            f("qty", FilterOperator::Lte, Some(DataValue::Integer(5))),
        ],
    );
}

#[test]
fn keeps_what_sql_could_evaluate_differently_in_the_residual_expression() {
    for src in [
        "record.price > 10",
        "record.active = true",
        "record.status < \"m\"",
        "record.status = \"2026-01-31\"",
        "record.amount > \"10\"",
        "record.amount > null",
        "record.amount > record.qty",
        "record.amount % 2 = 0",
        "record.qty not in (1, 2)",
        "record.qty in (1, null)",
        "record.qty not between 1 and 5",
        "record.missing = 1",
        "record.amount > 1 or record.qty > 1",
        "not (record.amount > 1)",
        "record.amount > app.nothing.deeper",
    ] {
        let result = split(src);
        assert!(result.filters.is_empty(), "{src}");
        assert_eq!(result.rest.as_ref(), Some(&expr::parse(src).unwrap()), "{src}");
    }
}

fn passes(src: &str, record: &Value) -> bool {
    let mut scope = scope();
    scope.insert("record".into(), record.clone());
    expr::evaluate_boolean(src, &scope, EvaluateOptions::default()).unwrap_or(false)
}

#[test]
fn splits_a_conjunction_into_pushed_filters_and_a_residual_that_keeps_the_same_rows() {
    let src = "record.amount >= 5 and record.qty % 2 = 0 and record.status = \"won\"";
    let result = split(src);
    let names: Vec<&str> = result.filters.iter().map(|f| f.column.as_str()).collect();
    assert_eq!(names, ["amount", "status"]);
    assert_eq!(rest_of(src).as_deref(), Some("record.qty % 2 = 0"));
    let rest_src = rest_of(src).unwrap();
    for amount in [json!(4), json!(5), Value::Null] {
        for qty in [json!(2), json!(3), Value::Null] {
            for status in [json!("won"), json!("lost"), Value::Null] {
                let record = json!({"amount": amount, "qty": qty, "status": status});
                let whole = passes(src, &record);
                let pushed = amount.as_i64().is_some_and(|a| a >= 5) && status == json!("won");
                let residual = passes(&rest_src, &record);
                assert_eq!(pushed && residual, whole, "{record}");
            }
        }
    }
}
