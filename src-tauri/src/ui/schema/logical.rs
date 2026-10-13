//! Logical column types as the UI speaks them (port of `src/schema/logical.ts`).

use crate::data::{Column, DataValue};
use crate::recordstore::capabilities::StoreCapabilities;

/// Logical column types (PRD §11), in picker order: (value, label).
pub const LOGICAL_TYPES: &[(&str, &str)] = &[
    ("text", "Text"),
    ("integer", "Integer"),
    ("real", "Number (floating point)"),
    ("decimal", "Decimal (precision, scale)"),
    ("boolean", "Yes / No"),
    ("date", "Date"),
    ("time", "Time"),
    ("timestamp", "Date and time"),
    ("uuid", "UUID"),
    ("json", "JSON"),
    ("blob", "Binary"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogicalParts {
    pub base: String,
    pub precision: u32,
    pub scale: u32,
}

/// Splits `decimal(10,2)` into its base and numbers; anything unreadable is text.
pub fn parse_logical(logical: Option<&str>) -> LogicalParts {
    let fallback = LogicalParts {
        base: "text".into(),
        precision: 10,
        scale: 2,
    };
    let text = logical.unwrap_or("text").trim();
    let (head, args) = match text.find('(') {
        Some(open) if text.ends_with(')') => (text[..open].trim(), Some(&text[open + 1..text.len() - 1])),
        Some(_) => return fallback,
        None => (text, None),
    };
    if head.is_empty() || !head.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return fallback;
    }
    let numbers: Vec<Option<u32>> = match args {
        None => vec![],
        Some(args) => args
            .split(',')
            .map(|p| p.trim().parse::<u32>().ok())
            .collect(),
    };
    if args.is_some() && (numbers.is_empty() || numbers.len() > 2 || numbers.iter().any(Option::is_none)) {
        return fallback;
    }
    let precision = numbers.first().copied().flatten();
    let scale = numbers.get(1).copied().flatten();
    LogicalParts {
        base: head.to_lowercase(),
        precision: precision.unwrap_or(10),
        scale: scale.unwrap_or(if precision.is_some() { 0 } else { 2 }),
    }
}

pub fn format_logical(parts: &LogicalParts) -> String {
    if parts.base == "decimal" {
        format!("decimal({},{})", parts.precision, parts.scale)
    } else {
        parts.base.clone()
    }
}

/// The logical type of a column.
pub fn logical_of(column: &Column) -> String {
    column.logical_type.to_string()
}

fn is_unsigned_digits(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_digit())
}

/// `^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$`
fn is_decimal_text(text: &str) -> bool {
    let text = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(at) => (&text[..at], Some(&text[at + 1..])),
        None => (text, None),
    };
    if let Some(exponent) = exponent {
        let digits = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        if !is_unsigned_digits(digits) {
            return false;
        }
    }
    match mantissa.split_once('.') {
        Some((whole, fraction)) => {
            (whole.is_empty() || is_unsigned_digits(whole))
                && (fraction.is_empty() || is_unsigned_digits(fraction))
                && !(whole.is_empty() && fraction.is_empty())
        }
        None => is_unsigned_digits(mantissa),
    }
}

/// 2^53 - 1, the largest integer a JSON number holds exactly.
const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// Turns grid or form text into a typed value; an error is a readable message.
pub fn value_from_text(text: &str, column: &str, logical: &str) -> Result<DataValue, String> {
    if text == "NULL" {
        return Ok(DataValue::Null);
    }
    let base = parse_logical(Some(logical)).base;
    let trimmed = text.trim();
    match base.as_str() {
        "integer" => {
            let digits = trimmed.strip_prefix('-').unwrap_or(trimmed);
            if !is_unsigned_digits(digits) {
                return Err(format!("{column} requires an integer"));
            }
            match trimmed.parse::<i64>() {
                Ok(v) if v.abs() <= MAX_SAFE_INTEGER => Ok(DataValue::Integer(v)),
                _ => Err(format!("{column} is outside the safe integer range")),
            }
        }
        "real" => match trimmed.parse::<f64>() {
            Ok(v) if !trimmed.is_empty() && v.is_finite() && !trimmed.eq_ignore_ascii_case("nan") => {
                Ok(DataValue::Real(v))
            }
            _ => Err(format!("{column} requires a number")),
        },
        "decimal" => {
            if is_decimal_text(trimmed) {
                Ok(DataValue::Decimal(trimmed.to_string()))
            } else {
                Err(format!("{column} requires a decimal number"))
            }
        }
        "boolean" => match trimmed.to_lowercase().as_str() {
            "true" | "yes" | "1" => Ok(DataValue::Boolean(true)),
            "false" | "no" | "0" => Ok(DataValue::Boolean(false)),
            _ => Err(format!("{column} requires true or false")),
        },
        "date" => Ok(DataValue::Date(trimmed.to_string())),
        "time" => Ok(DataValue::Time(trimmed.to_string())),
        "timestamp" => Ok(DataValue::Timestamp(trimmed.to_string())),
        _ => Ok(DataValue::Text(text.to_string())),
    }
}

/// Store-neutral label for a change mode.
pub fn mode_label(mode: &str) -> &'static str {
    match mode {
        "inPlace" => "Changes in place",
        "rebuild" => "Requires table rebuild",
        _ => "Not supported by this store",
    }
}

/// The store's display name, or None while capabilities are loading.
pub fn store_label(capabilities: Option<&StoreCapabilities>) -> Option<String> {
    capabilities.map(|c| match c.store.as_str() {
        "postgres" => "PostgreSQL".to_string(),
        "sqlite" => "SQLite".to_string(),
        other => other.to_string(),
    })
}

/// Base names of the logical types the store maps (`decimal(p,s)` becomes `decimal`),
/// or None while capabilities are loading.
pub fn store_types(capabilities: Option<&StoreCapabilities>) -> Option<Vec<String>> {
    capabilities.map(|c| {
        c.logical_types
            .iter()
            .map(|t| parse_logical(Some(&t.logical_type.replace("(p,s)", ""))).base)
            .collect()
    })
}

/// The text a cell shows for a value (`showValue` in the React UI).
pub fn show_value(value: &DataValue) -> String {
    match value {
        DataValue::Null => "NULL".into(),
        DataValue::Blob(b) => format!("Blob ({} base64 chars)", b.chars().count()),
        DataValue::Integer(v) => v.to_string(),
        DataValue::Real(v) => {
            if v.fract() == 0.0 && v.abs() < 1e15 {
                format!("{}", *v as i64)
            } else {
                v.to_string()
            }
        }
        DataValue::Boolean(v) => v.to_string(),
        DataValue::Text(v)
        | DataValue::Date(v)
        | DataValue::Timestamp(v)
        | DataValue::Decimal(v)
        | DataValue::Time(v) => v.clone(),
    }
}
