//! Value helpers: number normalization and printing, ISO date parsing, and
//! ordering. Values are `serde_json::Value`; numbers are read as `f64`.
use chrono::{DateTime, Datelike, Duration, NaiveDate, Timelike, Utc};
use serde_json::{Number, Value};
use std::cmp::Ordering;

const SIGNIFICANT_DIGITS: usize = 15;

/// Rounds a computed number to 15 significant digits so binary floating
/// point noise never shows up (0.1 + 0.2 is 0.3).
pub fn normalize_number(n: f64) -> f64 {
    if !n.is_finite() {
        return n;
    }
    if n == 0.0 {
        return 0.0;
    }
    format!("{:.*e}", SIGNIFICANT_DIGITS - 1, n)
        .parse()
        .unwrap_or(n)
}

/// Rounds half away from zero to `digits` decimal places, without float drift.
pub fn round_to(n: f64, digits: f64) -> f64 {
    let scale = 10f64.powf(digits);
    let scaled = normalize_number(n * scale);
    let rounded = if scaled == 0.0 {
        0.0
    } else {
        scaled.signum() * scaled.abs().round()
    };
    normalize_number(rounded / scale)
}

/// Builds a number value. Whole numbers become integers so they compare equal
/// to integer JSON numbers; a non-finite number becomes null.
pub fn num_value(n: f64) -> Value {
    if !n.is_finite() {
        return Value::Null;
    }
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        return Value::from(n as i64);
    }
    Number::from_f64(n).map_or(Value::Null, Value::Number)
}

pub fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Array(_) => "list",
        Value::String(_) => "text",
        Value::Object(_) => "record",
        Value::Number(_) => "number",
        Value::Bool(_) => "boolean",
    }
}

pub(crate) fn is_container(v: &Value) -> bool {
    matches!(v, Value::Array(_) | Value::Object(_))
}

/// JavaScript whitespace, as matched by `\s` and `String.prototype.trim`.
pub(crate) fn is_js_space(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
}

pub(crate) fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

/// Prints a number the way JavaScript's `String(n)` does.
pub fn js_number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let sci = format!("{:e}", x.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let sign_e = if e < 0 { '-' } else { '+' };
        if k == 1 {
            format!("{digits}e{sign_e}{}", e.abs())
        } else {
            format!("{}.{}e{sign_e}{}", &digits[..1], &digits[1..], e.abs())
        }
    };
    format!("{sign}{body}")
}

pub fn to_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Number(n) => js_number_to_string(normalize_number(n.as_f64().unwrap_or(0.0))),
        Value::Bool(b) => b.to_string(),
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

pub(crate) fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedDate {
    /// Milliseconds since the Unix epoch, in UTC.
    pub ms: i64,
    pub has_time: bool,
    pub iso: String,
}

/// UTC milliseconds for a calendar position. Month and day may overflow and
/// roll over, as in `Date.UTC`. Returns `None` outside the supported years.
pub fn utc_ms(
    year: i64,
    month0: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
) -> Option<i64> {
    let year = year.checked_add(month0.div_euclid(12))?;
    let month = month0.rem_euclid(12) as u32 + 1;
    let first = NaiveDate::from_ymd_opt(i32::try_from(year).ok()?, month, 1)?;
    let date = first.checked_add_signed(Duration::try_days(day.checked_sub(1)?)?)?;
    let midnight = date.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis();
    let time = hour
        .checked_mul(3_600_000)?
        .checked_add(minute.checked_mul(60_000)?)?
        .checked_add(second.checked_mul(1000)?)?;
    midnight.checked_add(time)
}

pub(crate) fn datetime(ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ms).unwrap_or_default()
}

pub fn format_iso_date(ms: i64) -> String {
    let d = datetime(ms);
    format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())
}

pub fn format_iso_datetime(ms: i64) -> String {
    let d = datetime(ms);
    format!(
        "{}T{:02}:{:02}:{:02}",
        format_iso_date(ms),
        d.hour(),
        d.minute(),
        d.second()
    )
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn digits(&mut self, n: usize) -> Option<i64> {
        let slice = self.bytes.get(self.pos..self.pos + n)?;
        if !slice.iter().all(u8::is_ascii_digit) {
            return None;
        }
        self.pos += n;
        Some(
            slice
                .iter()
                .fold(0, |acc, d| acc * 10 + i64::from(d - b'0')),
        )
    }

    fn eat(&mut self, byte: u8) -> bool {
        let hit = self.bytes.get(self.pos) == Some(&byte);
        if hit {
            self.pos += 1;
        }
        hit
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }
}

struct Zone {
    sign: i64,
    minutes: i64,
}

/// Parses `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM[:SS]` (optionally with `Z` or an
/// offset, which is converted to UTC). Returns `None` when the text is not a
/// real calendar date.
pub fn parse_iso_date(text: &str) -> Option<ParsedDate> {
    let mut c = Cursor {
        bytes: text.as_bytes(),
        pos: 0,
    };
    let year = c.digits(4)?;
    if !c.eat(b'-') {
        return None;
    }
    let month = c.digits(2)?;
    if !c.eat(b'-') {
        return None;
    }
    let day = c.digits(2)?;
    let mut time = None;
    let mut zone = None;
    if c.pos < text.len() {
        if !(c.eat(b'T') || c.eat(b' ')) {
            return None;
        }
        let hour = c.digits(2)?;
        if !c.eat(b':') {
            return None;
        }
        let minute = c.digits(2)?;
        let mut second = 0;
        if c.eat(b':') {
            second = c.digits(2)?;
            if c.eat(b'.') {
                let before = c.pos;
                while c.peek().is_some_and(|b| b.is_ascii_digit()) {
                    c.pos += 1;
                }
                if c.pos == before {
                    return None;
                }
            }
        }
        time = Some((hour, minute, second));
        let rest = text.get(c.pos..)?;
        let trimmed = rest.trim_start_matches(is_js_space);
        c.pos = text.len() - trimmed.len();
        if c.pos < text.len() {
            zone = Some(parse_zone(&mut c)?);
            if c.pos != text.len() {
                return None;
            }
        }
    }
    let (hour, minute, second) = time.unwrap_or((0, 0, 0));
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32)?;
    let mut ms = utc_ms(year, month - 1, day, hour, minute, second)?;
    if let Some(z) = zone {
        ms -= z.sign * z.minutes * 60_000;
    }
    let has_time = time.is_some();
    let iso = if has_time {
        format_iso_datetime(ms)
    } else {
        format_iso_date(ms)
    };
    Some(ParsedDate { ms, has_time, iso })
}

fn parse_zone(c: &mut Cursor) -> Option<Zone> {
    if c.eat(b'Z') {
        return Some(Zone {
            sign: 1,
            minutes: 0,
        });
    }
    let sign = match c.peek()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    c.pos += 1;
    let hours = c.digits(2)?;
    c.eat(b':');
    let minutes = c.digits(2)?;
    Some(Zone {
        sign,
        minutes: hours * 60 + minutes,
    })
}

/// Orders two non-null scalar values. Returns `None` when they are different
/// kinds and cannot be ordered. Text that is a date in both operands compares
/// as a date, so `2026-01-31` equals `2026-01-31T00:00:00`.
pub fn compare_values(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64()?, y.as_f64()?);
            Some(if x == y {
                Ordering::Equal
            } else if x < y {
                Ordering::Less
            } else {
                Ordering::Greater
            })
        }
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        (Value::String(x), Value::String(y)) => {
            if x == y {
                return Some(Ordering::Equal);
            }
            if let (Some(da), Some(db)) = (parse_iso_date(x), parse_iso_date(y)) {
                return Some(da.ms.cmp(&db.ms));
            }
            // JavaScript orders text by UTF-16 code unit
            Some(x.encode_utf16().cmp(y.encode_utf16()))
        }
        _ => None,
    }
}
