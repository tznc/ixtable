//! Number and date format patterns (`#,##0.00`, `MMM d, yyyy`).
use super::values::{
    datetime, js_number_to_string, normalize_number, parse_iso_date, round_to, to_text, type_name,
};
use chrono::{Datelike, Timelike};
use serde_json::Value;
use std::fmt;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// A value that does not fit its format pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct FormatError(pub String);

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FormatError {}

/// Removes backslash escapes and quote characters from literal pattern text.
fn strip_literal(text: &str) -> String {
    let mut unescaped = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let escapes =
            matches!(chars.peek(), Some(n) if !matches!(n, '\n' | '\r' | '\u{2028}' | '\u{2029}'));
        if c == '\\' && escapes {
            unescaped.extend(chars.next());
        } else {
            unescaped.push(c);
        }
    }
    unescaped.retain(|c| c != '"' && c != '\'');
    unescaped
}

/// Finds the first `[#0][#0,]*(\.[#0]*)?` or `\.[#0]+` run, as char indices.
fn number_core(chars: &[char]) -> Option<(usize, usize)> {
    let is_digit = |c: Option<&char>| matches!(c, Some('#' | '0'));
    for i in 0..chars.len() {
        if is_digit(chars.get(i)) {
            let mut j = i + 1;
            while matches!(chars.get(j), Some('#' | '0' | ',')) {
                j += 1;
            }
            if chars.get(j) == Some(&'.') {
                j += 1;
                while is_digit(chars.get(j)) {
                    j += 1;
                }
            }
            return Some((i, j));
        }
        if chars[i] == '.' && is_digit(chars.get(i + 1)) {
            let mut j = i + 1;
            while is_digit(chars.get(j)) {
                j += 1;
            }
            return Some((i, j));
        }
    }
    None
}

fn group_thousands(digits: &str) -> String {
    let mut out = String::new();
    let len = digits.len();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn format_number(n: f64, pattern: &str) -> Result<String, FormatError> {
    let chars: Vec<char> = pattern.chars().collect();
    let Some((from, to)) = number_core(&chars) else {
        return Err(FormatError(format!("'{pattern}' is not a number format")));
    };
    let prefix = strip_literal(&chars[..from].iter().collect::<String>());
    let suffix = strip_literal(&chars[to..].iter().collect::<String>());
    let core: String = chars[from..to].iter().collect();
    let (int_pattern, frac_pattern) = core.split_once('.').unwrap_or((&core, ""));
    let mut value = n;
    if prefix.contains('%') || suffix.contains('%') {
        value = normalize_number(value * 100.0);
    }
    let min_int = int_pattern.matches('0').count();
    let min_frac = frac_pattern.matches('0').count();
    let max_frac = frac_pattern.chars().count();
    let rounded = round_to(value, max_frac as f64);
    let negative = rounded < 0.0;
    let fixed = format!("{:.*}", max_frac, rounded.abs());
    let (int_digits, frac_digits) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let mut int_digits = int_digits.to_string();
    let mut frac_digits = frac_digits.to_string();
    while frac_digits.len() > min_frac && frac_digits.ends_with('0') {
        frac_digits.pop();
    }
    if min_int == 0 && int_digits == "0" {
        int_digits.clear();
    }
    while int_digits.len() < min_int {
        int_digits.insert(0, '0');
    }
    if int_pattern.contains(',') {
        int_digits = group_thousands(&int_digits);
    }
    let body = if !frac_digits.is_empty() {
        format!("{int_digits}.{frac_digits}")
    } else if int_digits.is_empty() {
        "0".to_string()
    } else {
        int_digits
    };
    Ok(format!(
        "{}{prefix}{body}{suffix}",
        if negative { "-" } else { "" }
    ))
}

const DATE_TOKENS: [&str; 19] = [
    "yyyy", "yy", "MMMM", "MMM", "MM", "M", "dddd", "ddd", "dd", "d", "HH", "H", "hh", "h", "mm",
    "m", "ss", "s", "tt",
];

/// The token at `i`: a letter token or a quoted literal, in regex order.
fn date_token(chars: &[char], i: usize) -> Option<String> {
    let rest = &chars[i..];
    for token in DATE_TOKENS {
        let t: Vec<char> = token.chars().collect();
        if rest.starts_with(&t) {
            return Some(token.to_string());
        }
    }
    let quote = *rest.first()?;
    if quote == '\'' || quote == '"' {
        let close = rest[1..].iter().position(|c| *c == quote)?;
        return Some(rest[..close + 2].iter().collect());
    }
    None
}

pub fn format_date(iso: &str, pattern: &str) -> Result<String, FormatError> {
    let Some(parsed) = parse_iso_date(iso) else {
        return Err(FormatError(format!("'{iso}' is not a date")));
    };
    let d = datetime(parsed.ms);
    let month = d.month0() as usize;
    let weekday = d.weekday().num_days_from_sunday() as usize;
    let hours = d.hour();
    let hour12 = if hours % 12 == 0 { 12 } else { hours % 12 };
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let Some(token) = date_token(&chars, i) else {
            out.push(chars[i]);
            i += 1;
            continue;
        };
        i += token.chars().count();
        let piece = match token.as_str() {
            "yyyy" => format!("{:04}", d.year()),
            "yy" => format!("{:02}", d.year().rem_euclid(100)),
            "MMMM" => MONTHS[month].to_string(),
            "MMM" => MONTHS[month][..3].to_string(),
            "MM" => format!("{:02}", month + 1),
            "M" => (month + 1).to_string(),
            "dddd" => DAYS[weekday].to_string(),
            "ddd" => DAYS[weekday][..3].to_string(),
            "dd" => format!("{:02}", d.day()),
            "d" => d.day().to_string(),
            "HH" => format!("{hours:02}"),
            "H" => hours.to_string(),
            "hh" => format!("{hour12:02}"),
            "h" => hour12.to_string(),
            "mm" => format!("{:02}", d.minute()),
            "m" => d.minute().to_string(),
            "ss" => format!("{:02}", d.second()),
            "s" => d.second().to_string(),
            "tt" => if hours < 12 { "AM" } else { "PM" }.to_string(),
            quoted => quoted[1..quoted.len() - 1].to_string(),
        };
        out.push_str(&piece);
    }
    Ok(out)
}

/// Formats a value for display. Numbers use number patterns (`#,##0.00`,
/// `0%`, `$#,##0.00`); date text uses date patterns (`yyyy-MM-dd`,
/// `MMM d, yyyy`). Without a pattern the value is converted to plain text.
/// Null formats as an empty string.
pub fn format_value(value: &Value, pattern: Option<&str>) -> Result<String, FormatError> {
    if value.is_null() {
        return Ok(String::new());
    }
    let Some(pattern) = pattern.filter(|p| !p.is_empty()) else {
        return Ok(to_text(value));
    };
    match value {
        Value::Number(n) => format_number(n.as_f64().unwrap_or(0.0), pattern),
        Value::String(s) if parse_iso_date(s).is_some() => format_date(s, pattern),
        other => Err(FormatError(format!(
            "Can't apply format '{pattern}' to {}",
            json_text(other)
        ))),
    }
}

/// `JSON.stringify` for the values a format can reject.
pub(crate) fn json_text(v: &Value) -> String {
    match v {
        Value::Number(n) => js_number_to_string(n.as_f64().unwrap_or(0.0)),
        other => serde_json::to_string(other).unwrap_or_else(|_| type_name(other).to_string()),
    }
}
