//! Conversions between expression values (JSON) and RecordStore values, plus the
//! stable hash behind default idempotency keys (formerly `src/automation/values.ts`).

use crate::data::{DataValue, NamedValue};
use serde_json::{Map, Number, Value};

/// A whole float becomes an integer, like the expression language's numbers.
fn number(f: f64) -> Value {
    if !f.is_finite() {
        return Value::Null;
    }
    if f.fract() == 0.0 && f.abs() < 9.0e15 {
        return Value::from(f as i64);
    }
    Number::from_f64(f).map_or(Value::Null, Value::Number)
}

/// Converts a value from the expression language into a RecordStore `DataValue`.
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

/// Converts a `DataValue` into a plain value for expression scopes.
pub fn from_data_value(value: &DataValue) -> Value {
    match value {
        DataValue::Null => Value::Null,
        DataValue::Integer(i) => Value::from(*i),
        DataValue::Real(f) => number(*f),
        DataValue::Boolean(b) => Value::Bool(*b),
        DataValue::Text(s)
        | DataValue::Blob(s)
        | DataValue::Date(s)
        | DataValue::Timestamp(s)
        | DataValue::Decimal(s)
        | DataValue::Time(s) => Value::String(s.clone()),
    }
}

pub fn to_named_values(values: &Map<String, Value>) -> Vec<NamedValue> {
    values
        .iter()
        .map(|(column, value)| NamedValue {
            column: column.clone(),
            value: to_data_value(value),
        })
        .collect()
}

pub fn named_to_object(values: &[NamedValue]) -> Map<String, Value> {
    values
        .iter()
        .map(|v| (v.column.clone(), from_data_value(&v.value)))
        .collect()
}

pub fn row_to_object(columns: &[String], row: &[DataValue]) -> Map<String, Value> {
    columns
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let value = row.get(i).map_or(Value::Null, from_data_value);
            (name.clone(), value)
        })
        .collect()
}

/// JSON text with sorted object keys.
pub fn stable_json(value: &Value) -> String {
    match value {
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(stable_json).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", Value::String(k.clone()), stable_json(&map[k])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        other => other.to_string(),
    }
}

/// Stable FNV-1a hash (8 hex digits) of a value with sorted object keys. It hashes
/// UTF-16 code units, so keys match the ones the TypeScript runner produced.
pub fn stable_hash(value: &Value) -> String {
    let mut hash: u32 = 0x811c_9dc5;
    for unit in stable_json(value).encode_utf16() {
        hash ^= u32::from(unit);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("{hash:08x}")
}

/// A value as JavaScript's `String(value)` prints it (null prints "null").
pub fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::String(s) => s.clone(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.as_i64().is_none() => number(f).to_string(),
            _ => n.to_string(),
        },
        Value::Array(items) => items
            .iter()
            .map(|v| {
                if v.is_null() {
                    String::new()
                } else {
                    js_string(v)
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// `String(value ?? "")`.
pub fn text_of(value: &Value) -> String {
    if value.is_null() {
        String::new()
    } else {
        js_string(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn converts_values_both_ways() {
        assert_eq!(to_data_value(&json!(null)), DataValue::Null);
        assert_eq!(to_data_value(&json!(true)), DataValue::Boolean(true));
        assert_eq!(to_data_value(&json!(3)), DataValue::Integer(3));
        assert_eq!(to_data_value(&json!(2.5)), DataValue::Real(2.5));
        assert_eq!(to_data_value(&json!(4.0)), DataValue::Integer(4));
        assert_eq!(to_data_value(&json!("x")), DataValue::Text("x".into()));
        assert_eq!(
            to_data_value(&json!({"a": 1})),
            DataValue::Text("{\"a\":1}".into())
        );
        assert_eq!(from_data_value(&DataValue::Real(10.0)), json!(10));
        assert_eq!(
            from_data_value(&DataValue::Date("2024-01-31".into())),
            json!("2024-01-31")
        );
        assert_eq!(from_data_value(&DataValue::Null), Value::Null);
    }

    #[test]
    fn hash_ignores_key_order_and_is_eight_hex_digits() {
        let a = stable_hash(&json!({"b": 1, "a": [1, "x"]}));
        assert_eq!(a, stable_hash(&json!({"a": [1, "x"], "b": 1})));
        assert_eq!(a.len(), 8);
        assert_ne!(a, stable_hash(&json!({"a": [1, "y"], "b": 1})));
    }

    #[test]
    fn prints_values_like_javascript() {
        assert_eq!(js_string(&json!(1)), "1");
        assert_eq!(js_string(&json!(1.5)), "1.5");
        assert_eq!(js_string(&json!(null)), "null");
        assert_eq!(text_of(&json!(null)), "");
        assert_eq!(js_string(&json!([1, null, "a"])), "1,,a");
    }
}
