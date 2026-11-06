//! Values of template sample data (`sampleData/*.xml`), read as model values.
use crate::access::model::*;
use crate::access::blob;
use crate::access::xml::Element;
use base64::Engine;

/// Converts one sample-data value to a model value for a column.
pub fn sample_value(col: &Column, el: &Element) -> Value {
    let text = el.text();
    match (&col.ty, &col.complex) {
        (_, Some(Complex::Attachment)) => {
            let get = |k: &str| el.child(k).map(|e| e.text()).unwrap_or_default();
            let content = decode_base64(&get("FileData"));
            let (ext, data) = blob::split_attachment(&content);
            let file_type = if get("FileType").is_empty() {
                ext
            } else {
                get("FileType")
            };
            Value::Attachments(vec![Attachment {
                file_name: get("FileName"),
                file_type,
                data,
            }])
        }
        (_, Some(Complex::MultiValue(ty))) => {
            let inner = el.child("Value").map(|v| v.text()).unwrap_or(text);
            Value::Multi(vec![scalar(*ty, inner.trim())])
        }
        (ColType::Ole | ColType::Binary, _) => Value::Binary(decode_base64(&text)),
        (ty, _) => scalar(*ty, &text),
    }
}

fn scalar(ty: ColType, text: &str) -> Value {
    let t = text.trim();
    match ty {
        ColType::Boolean => Value::Bool(matches!(t, "1" | "-1" | "true" | "True")),
        ColType::Byte | ColType::Integer | ColType::Long | ColType::BigInt => t
            .parse::<i64>()
            .map(Value::Int)
            .unwrap_or_else(|_| Value::Text(text.to_string())),
        ColType::Single | ColType::Double => t
            .parse::<f64>()
            .map(Value::Double)
            .unwrap_or_else(|_| Value::Text(text.to_string())),
        ColType::Currency | ColType::Numeric { .. } => Value::Decimal(t.to_string()),
        ColType::DateTime | ColType::ExtDateTime => Value::DateTime(t.to_string()),
        ColType::Guid => Value::Guid(t.trim_matches(|c| c == '{' || c == '}').to_lowercase()),
        _ => Value::Text(text.to_string()),
    }
}

fn decode_base64(s: &str) -> Vec<u8> {
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(clean)
        .unwrap_or_default()
}

