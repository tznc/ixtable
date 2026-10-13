//! Record values: conversion between JSON values (the expression scope), form values and
//! typed `DataValue`s (port of `src/runtime/values.ts` and `src/automation/values.ts`).

use crate::data::{Column, DataValue, LogicalType, NamedValue};
use serde_json::{Map, Number, Value};

pub type RecordValues = Map<String, Value>;

/// A finite number as JSON; non-finite numbers become null.
pub fn number_value(n: f64) -> Value {
    if !n.is_finite() {
        return Value::Null;
    }
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        return Value::from(n as i64);
    }
    Number::from_f64(n).map_or(Value::Null, Value::Number)
}

/// The JSON number as f64.
pub fn as_number(value: &Value) -> Option<f64> {
    value.as_f64()
}

/// A RecordStore value as a plain JSON value for expression scopes.
pub fn from_data_value(value: &DataValue) -> Value {
    match value {
        DataValue::Null => Value::Null,
        DataValue::Integer(i) => Value::from(*i),
        DataValue::Real(f) => number_value(*f),
        DataValue::Boolean(b) => Value::Bool(*b),
        DataValue::Text(s)
        | DataValue::Blob(s)
        | DataValue::Date(s)
        | DataValue::Timestamp(s)
        | DataValue::Decimal(s)
        | DataValue::Time(s) => Value::String(s.clone()),
    }
}

/// A JSON value from the expression language as a RecordStore `DataValue`.
pub fn to_data_value(value: &Value) -> DataValue {
    match value {
        Value::Null => DataValue::Null,
        Value::Bool(b) => DataValue::Boolean(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => DataValue::Integer(i),
            None => match n.as_f64() {
                Some(f) if f.is_finite() && f.fract() == 0.0 && f.abs() < 9.0e15 => {
                    DataValue::Integer(f as i64)
                }
                Some(f) if f.is_finite() => DataValue::Real(f),
                _ => DataValue::Null,
            },
        },
        Value::String(s) => DataValue::Text(s.clone()),
        other => DataValue::Text(other.to_string()),
    }
}

pub fn row_object(columns: &[String], row: &[DataValue]) -> RecordValues {
    columns
        .iter()
        .enumerate()
        .map(|(i, name)| {
            (
                name.clone(),
                row.get(i).map_or(Value::Null, from_data_value),
            )
        })
        .collect()
}

pub fn is_blank(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        _ => false,
    }
}

/// Converts a form value into a typed DataValue using the column's declared type.
pub fn to_column_value(value: &Value, declared_type: &str) -> DataValue {
    if is_blank(value) {
        return DataValue::Null;
    }
    let declared = declared_type.to_ascii_uppercase();
    let numeric_type = ["REAL", "FLOA", "DOUB", "NUM", "DEC", "MONEY"]
        .iter()
        .any(|t| declared.contains(t));
    if declared.contains("BOOL") {
        let truthy = match value {
            Value::Bool(b) => *b,
            Value::String(s) => s == "true",
            Value::Number(n) => n.as_f64() == Some(1.0),
            _ => false,
        };
        return DataValue::Boolean(truthy);
    }
    let is_int = declared.contains("INT");
    if is_int || numeric_type {
        let number = match value {
            Value::Number(n) => n.as_f64(),
            Value::Bool(b) => Some(f64::from(u8::from(*b))),
            other => crate::forms::fields::value_text(other)
                .replace(',', "")
                .trim()
                .parse::<f64>()
                .ok(),
        };
        return match number.filter(|n| n.is_finite()) {
            None => DataValue::Text(crate::forms::fields::value_text(value)),
            Some(n) if is_int && n.fract() == 0.0 => DataValue::Integer(n as i64),
            Some(n) => DataValue::Real(n),
        };
    }
    match value {
        Value::Bool(b) => DataValue::Integer(i64::from(*b)),
        Value::Number(n) => match n.as_i64() {
            Some(i) => DataValue::Integer(i),
            None => match n.as_f64() {
                Some(f) if f.fract() == 0.0 => DataValue::Integer(f as i64),
                Some(f) => DataValue::Real(f),
                None => DataValue::Null,
            },
        },
        other => {
            let text = crate::forms::fields::value_text(other);
            if declared.contains("TIMESTAMP") || declared.contains("DATETIME") {
                DataValue::Timestamp(text)
            } else if declared.contains("DATE") {
                DataValue::Date(text)
            } else {
                DataValue::Text(text)
            }
        }
    }
}

/// A column's name and declared type, enough to type a value for writing.
pub trait TypedColumn {
    fn column_name(&self) -> &str;
    fn declared(&self) -> &str;
}

impl TypedColumn for Column {
    fn column_name(&self) -> &str {
        &self.name
    }
    fn declared(&self) -> &str {
        &self.declared_type
    }
}

/// Named values for the given record fields, typed by the table columns.
pub fn named_values<C: TypedColumn>(
    values: &RecordValues,
    columns: &[&C],
    only: Option<&[String]>,
) -> Vec<NamedValue> {
    columns
        .iter()
        .filter(|c| values.contains_key(c.column_name()))
        .filter(|c| only.is_none_or(|o| o.iter().any(|n| n == c.column_name())))
        .map(|c| NamedValue {
            column: c.column_name().to_string(),
            value: to_column_value(&values[c.column_name()], c.declared()),
        })
        .collect()
}

/// Display text for a value (dates stay ISO, booleans as Yes/No).
pub fn display_text(value: &Value) -> String {
    match value {
        Value::Bool(b) => if *b { "Yes" } else { "No" }.to_string(),
        other => crate::forms::fields::value_text(other),
    }
}

/// Loose equality for record values coming from different sources (1 vs "1").
pub fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Null, _) | (_, Value::Null) => false,
        _ => {
            a == b
                || crate::forms::fields::value_text(a) == crate::forms::fields::value_text(b)
        }
    }
}

/// True when a column holds yes/no values: a bound boolean control, or a boolean logical or
/// declared type. Decided from definitions, never from the values.
pub fn is_boolean_column(has_boolean_control: bool, column: Option<&Column>) -> bool {
    if has_boolean_control {
        return true;
    }
    column.is_some_and(|c| {
        c.logical_type == LogicalType::Boolean || c.declared_type.to_ascii_uppercase().contains("BOOL")
    })
}

/// Yes/no of a stored boolean (SQLite keeps 0/1); None for an empty value.
pub fn boolean_value(value: &Value) -> Option<bool> {
    match value {
        Value::Null => None,
        Value::String(s) if s.is_empty() => None,
        Value::String(s) => Some(!["0", "false", "no"].contains(&s.trim().to_lowercase().as_str())),
        Value::Bool(b) => Some(*b),
        Value::Number(n) => Some(n.as_f64().is_some_and(|f| f != 0.0)),
        _ => Some(true),
    }
}

#[cfg(test)]
mod tests {
    use super::{boolean_value, named_values, same_value, to_column_value};
    use crate::data::DataValue;
    use serde_json::{Value, json};

    struct Col(&'static str, &'static str);
    impl super::TypedColumn for Col {
        fn column_name(&self) -> &str {
            self.0
        }
        fn declared(&self) -> &str {
            self.1
        }
    }

    #[test]
    fn converts_form_values_to_typed_column_values() {
        assert_eq!(to_column_value(&json!(""), "TEXT"), DataValue::Null);
        assert_eq!(to_column_value(&json!("12"), "INTEGER"), DataValue::Integer(12));
        assert_eq!(to_column_value(&json!("1,250.5"), "REAL"), DataValue::Real(1250.5));
        assert_eq!(to_column_value(&json!(true), "BOOLEAN"), DataValue::Boolean(true));
        assert_eq!(to_column_value(&json!(true), "TEXT"), DataValue::Integer(1));
        assert_eq!(
            to_column_value(&json!("2026-01-02"), "DATE"),
            DataValue::Date("2026-01-02".into())
        );
        let record = json!({"a": "x", "b": 2, "c": 3});
        let record = record.as_object().unwrap();
        let (a, b) = (Col("a", "TEXT"), Col("b", "INTEGER"));
        assert_eq!(
            named_values(record, &[&a, &b], None)
                .into_iter()
                .map(|v| (v.column, v.value))
                .collect::<Vec<_>>(),
            [
                ("a".to_string(), DataValue::Text("x".into())),
                ("b".to_string(), DataValue::Integer(2))
            ]
        );
    }

    #[test]
    fn compares_and_reads_loosely() {
        assert!(same_value(&json!(1), &json!("1")));
        assert!(!same_value(&Value::Null, &json!("")));
        assert_eq!(boolean_value(&json!("no")), Some(false));
        assert_eq!(boolean_value(&json!(1)), Some(true));
        assert_eq!(boolean_value(&Value::Null), None);
    }
    #[test]
    fn decides_boolean_columns_from_definitions() {
        use crate::data::{Column, LogicalType};
        let column = |declared: &str, logical: LogicalType| Column {
            name: "c".into(),
            declared_type: declared.into(),
            logical_type: logical,
            nullable: true,
            default_value: None,
            primary_key_position: 0,
            generated: false,
            unique: false,
            auto_increment: false,
        };
        assert!(super::is_boolean_column(true, None));
        assert!(super::is_boolean_column(false, Some(&column("INTEGER", LogicalType::Boolean))));
        assert!(super::is_boolean_column(false, Some(&column("tinyint(BOOL)", LogicalType::Integer))));
        assert!(!super::is_boolean_column(false, Some(&column("INTEGER", LogicalType::Integer))));
        assert!(!super::is_boolean_column(false, None));
    }
}
