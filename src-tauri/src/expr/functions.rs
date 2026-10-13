//! The built-in function table. Arguments are evaluated lazily so `if` runs
//! only the chosen branch and the `*of` aggregates can evaluate per item.
use super::ast::{error_at, Ast, ExprError};
use super::evaluate::{eval_node, Env, EvalResult};
use super::format::{format_value, json_text};
use super::regex::{compile_safe_regex, MAX_REGEX_INPUT};
use super::values::{
    compare_values, datetime, format_iso_date, format_iso_datetime, js_number_to_string, js_trim,
    normalize_number, num_value, parse_iso_date, round_to, to_text, type_name, utc_ms, ParsedDate,
};
use chrono::{Datelike, Timelike};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

type R<T> = Result<T, ExprError>;

/// The lazily evaluated arguments of one function call.
pub struct Call<'a, 'e> {
    pub node: &'a Ast,
    pub name: &'a str,
    pub args: &'a [Ast],
    pub env: &'a Env<'e>,
}

impl Call<'_, '_> {
    fn count(&self) -> usize {
        self.args.len()
    }

    fn arg(&self, i: usize) -> EvalResult {
        eval_node(&self.args[i], self.env)
    }

    fn arg_for_item(&self, i: usize, item: &Value) -> EvalResult {
        eval_node(&self.args[i], &self.env.item(item))
    }
}

pub struct FnDef {
    names: &'static [&'static str],
    min: usize,
    max: usize,
    /// Index of the argument evaluated once per list item (`sumof(rows, qty * price)`).
    pub per_item_arg: Option<usize>,
    pub imp: fn(&Call) -> EvalResult,
}

fn fail<T>(c: &Call, i: usize, message: impl AsRef<str>) -> R<T> {
    let node = c.args.get(i).unwrap_or(c.node);
    Err(error_at(node, format!("{}: {}", c.name, message.as_ref())))
}

fn num(c: &Call, i: usize) -> R<Option<f64>> {
    let v = c.arg(i)?;
    match v {
        Value::Null => Ok(None),
        Value::Number(n) => Ok(n.as_f64()),
        other => fail(
            c,
            i,
            format!("expected a number but got {}", type_name(&other)),
        ),
    }
}

fn int(c: &Call, i: usize, fallback: Option<f64>) -> R<Option<f64>> {
    if i >= c.count() && fallback.is_some() {
        return Ok(fallback);
    }
    let v = num(c, i)?;
    if let Some(n) = v {
        if !(n.is_finite() && n.fract() == 0.0) {
            return fail(
                c,
                i,
                format!("expected a whole number but got {}", js_number_to_string(n)),
            );
        }
    }
    Ok(v)
}

fn text(c: &Call, i: usize) -> R<Option<String>> {
    let v = c.arg(i)?;
    match v {
        Value::Null => Ok(None),
        Value::Array(_) | Value::Object(_) => {
            fail(c, i, format!("expected text but got {}", type_name(&v)))
        }
        other => Ok(Some(to_text(&other))),
    }
}

fn date(c: &Call, i: usize) -> R<Option<ParsedDate>> {
    let v = c.arg(i)?;
    if v.is_null() {
        return Ok(None);
    }
    match v.as_str().and_then(parse_iso_date) {
        Some(parsed) => Ok(Some(parsed)),
        None => fail(c, i, format!("expected a date but got {}", json_text(&v))),
    }
}

fn flatten(values: Vec<Value>, out: &mut Vec<Value>) {
    for v in values {
        match v {
            Value::Array(items) => flatten(items, out),
            other => out.push(other),
        }
    }
}

fn all_args(c: &Call) -> R<Vec<Value>> {
    let args = (0..c.count()).map(|i| c.arg(i)).collect::<R<Vec<_>>>()?;
    let mut out = Vec::new();
    flatten(args, &mut out);
    Ok(out)
}

fn numbers(c: &Call, values: &[Value]) -> R<Vec<f64>> {
    let mut out = Vec::new();
    for v in values {
        match v {
            Value::Null => {}
            Value::Number(n) => out.push(n.as_f64().unwrap_or(0.0)),
            other => {
                return fail(
                    c,
                    0,
                    format!("expected numbers but got {}", type_name(other)),
                )
            }
        }
    }
    Ok(out)
}

fn sum_of(values: &[f64]) -> f64 {
    values.iter().fold(0.0, |acc, n| normalize_number(acc + n))
}

fn average(values: &[f64]) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    num_value(normalize_number(sum_of(values) / values.len() as f64))
}

fn extreme(c: &Call, values: Vec<Value>, sign: i32) -> EvalResult {
    let mut best: Option<Value> = None;
    for v in values {
        if v.is_null() {
            continue;
        }
        if matches!(v, Value::Array(_) | Value::Object(_)) {
            return fail(c, 0, format!("can't compare {} values", type_name(&v)));
        }
        let Some(current) = &best else {
            best = Some(v);
            continue;
        };
        let Some(cmp) = compare_values(&v, current) else {
            return fail(
                c,
                0,
                format!(
                    "can't compare {} with {}",
                    type_name(&v),
                    type_name(current)
                ),
            );
        };
        if cmp as i32 * sign > 0 {
            best = Some(v);
        }
    }
    Ok(best.unwrap_or(Value::Null))
}

fn list_arg(c: &Call) -> R<Option<Vec<Value>>> {
    match c.arg(0)? {
        Value::Null => Ok(None),
        Value::Array(items) => Ok(Some(items)),
        other => fail(
            c,
            0,
            format!("expected a list but got {}", type_name(&other)),
        ),
    }
}

fn per_item(c: &Call) -> R<Option<Vec<Value>>> {
    let Some(list) = list_arg(c)? else {
        return Ok(None);
    };
    let values = list.iter().map(|item| c.arg_for_item(1, item));
    Ok(Some(values.collect::<R<Vec<_>>>()?))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Year,
    Month,
    Week,
    Day,
    Hour,
    Minute,
    Second,
}

pub fn parse_unit(raw: &str) -> Option<Unit> {
    Some(match js_trim(raw).to_lowercase().as_str() {
        "year" | "years" | "yyyy" | "yy" | "y" => Unit::Year,
        "month" | "months" | "mm" | "m" => Unit::Month,
        "week" | "weeks" | "ww" | "wk" | "w" => Unit::Week,
        "day" | "days" | "dd" | "d" => Unit::Day,
        "hour" | "hours" | "hh" | "h" => Unit::Hour,
        "minute" | "minutes" | "mi" | "n" => Unit::Minute,
        "second" | "seconds" | "ss" | "s" => Unit::Second,
        _ => return None,
    })
}

fn unit_arg(c: &Call) -> R<Option<Unit>> {
    let Some(raw) = text(c, 0)? else {
        return Ok(None);
    };
    match parse_unit(&raw) {
        Some(unit) => Ok(Some(unit)),
        None => fail(
            c,
            0,
            format!("unknown unit '{raw}' (use year, month, week, day, hour, minute or second)"),
        ),
    }
}

fn unit_ms(unit: Unit) -> i64 {
    match unit {
        Unit::Second => 1000,
        Unit::Minute => 60_000,
        Unit::Hour => 3_600_000,
        _ => 86_400_000,
    }
}

fn date_diff(unit: Unit, a: i64, b: i64) -> i64 {
    let (da, db) = (datetime(a), datetime(b));
    match unit {
        Unit::Year => i64::from(db.year() - da.year()),
        Unit::Month => {
            i64::from((db.year() - da.year()) * 12) + i64::from(db.month0())
                - i64::from(da.month0())
        }
        Unit::Week => date_diff(Unit::Day, a, b) / 7,
        _ => b.div_euclid(unit_ms(unit)) - a.div_euclid(unit_ms(unit)),
    }
}

/// Moves `ms` by `n` units, or `None` when the result is out of range.
fn date_add(unit: Unit, n: f64, ms: i64) -> Option<i64> {
    if n.abs() > 1e12 {
        return None;
    }
    let n = n as i64;
    match unit {
        Unit::Year | Unit::Month => {
            let d = datetime(ms);
            let step = if unit == Unit::Year {
                n.checked_mul(12)?
            } else {
                n
            };
            let total = i64::from(d.year()) * 12 + i64::from(d.month0()) + step;
            let (year, month) = (total.div_euclid(12), total.rem_euclid(12));
            let last_day = datetime(utc_ms(year, month + 1, 0, 0, 0, 0)?).day();
            let day = i64::from(d.day().min(last_day));
            utc_ms(
                year,
                month,
                day,
                i64::from(d.hour()),
                i64::from(d.minute()),
                i64::from(d.second()),
            )
        }
        Unit::Week => ms.checked_add(n.checked_mul(7 * unit_ms(Unit::Day))?),
        other => ms.checked_add(n.checked_mul(unit_ms(other))?),
    }
}

fn in_supported_years(ms: i64) -> bool {
    chrono::DateTime::from_timestamp_millis(ms).is_some_and(|d| (1..=9999).contains(&d.year()))
}

fn with_text(c: &Call, f: impl FnOnce(&str) -> EvalResult) -> EvalResult {
    match text(c, 0)? {
        None => Ok(Value::Null),
        Some(s) => f(&s),
    }
}

fn s(v: String) -> EvalResult {
    Ok(Value::String(v))
}

fn condition(c: &Call, i: usize, v: &Value) -> R<bool> {
    match v {
        Value::Null => Ok(false),
        Value::Bool(b) => Ok(*b),
        other => fail(
            c,
            i,
            format!(
                "condition must be true or false but got {}",
                type_name(other)
            ),
        ),
    }
}

fn f_if(c: &Call) -> EvalResult {
    let cond = c.arg(0)?;
    if condition(c, 0, &cond)? {
        return c.arg(1);
    }
    if c.count() > 2 {
        c.arg(2)
    } else {
        Ok(Value::Null)
    }
}

fn f_coalesce(c: &Call) -> EvalResult {
    for i in 0..c.count() {
        let v = c.arg(i)?;
        if !v.is_null() {
            return Ok(v);
        }
    }
    Ok(Value::Null)
}

fn f_isnull(c: &Call) -> EvalResult {
    Ok(Value::Bool(c.arg(0)?.is_null()))
}

fn f_isblank(c: &Call) -> EvalResult {
    let v = c.arg(0)?;
    Ok(Value::Bool(match &v {
        Value::Null => true,
        Value::String(t) => js_trim(t).is_empty(),
        _ => false,
    }))
}

fn f_len(c: &Call) -> EvalResult {
    with_text(c, |t| Ok(num_value(t.chars().count() as f64)))
}

fn f_lower(c: &Call) -> EvalResult {
    with_text(c, |t| s(t.to_lowercase()))
}

fn f_upper(c: &Call) -> EvalResult {
    with_text(c, |t| s(t.to_uppercase()))
}

fn f_trim(c: &Call) -> EvalResult {
    with_text(c, |t| s(js_trim(t).to_string()))
}

fn count_arg(c: &Call) -> R<Option<usize>> {
    let Some(n) = int(c, 1, Some(1.0))? else {
        return Ok(None);
    };
    if n < 0.0 {
        return fail(c, 1, "count can't be negative");
    }
    Ok(Some(n as usize))
}

fn f_left(c: &Call) -> EvalResult {
    with_text(c, |t| match count_arg(c)? {
        None => Ok(Value::Null),
        Some(n) => s(t.chars().take(n).collect()),
    })
}

fn f_right(c: &Call) -> EvalResult {
    with_text(c, |t| match count_arg(c)? {
        None => Ok(Value::Null),
        Some(n) => {
            let skip = t.chars().count().saturating_sub(n);
            s(t.chars().skip(skip).collect())
        }
    })
}

fn f_mid(c: &Call) -> EvalResult {
    with_text(c, |t| {
        let start = int(c, 1, None)?;
        let length = int(c, 2, Some(f64::INFINITY))?;
        let (Some(start), Some(length)) = (start, length) else {
            return Ok(Value::Null);
        };
        if start < 1.0 {
            return fail(c, 1, "start position begins at 1");
        }
        if length < 0.0 {
            return fail(c, 2, "length can't be negative");
        }
        let count = length.min(t.chars().count() as f64) as usize;
        s(t.chars().skip((start - 1.0) as usize).take(count).collect())
    })
}

fn f_concat(c: &Call) -> EvalResult {
    let mut out = String::new();
    for i in 0..c.count() {
        out.push_str(&text(c, i)?.unwrap_or_default());
    }
    s(out)
}

fn caseless(c: &Call, test: impl FnOnce(&str, &str) -> bool) -> EvalResult {
    with_text(c, |t| match text(c, 1)? {
        None => Ok(Value::Null),
        Some(sub) => Ok(Value::Bool(test(&t.to_lowercase(), &sub.to_lowercase()))),
    })
}

fn f_contains(c: &Call) -> EvalResult {
    caseless(c, |t, sub| t.contains(sub))
}

fn f_startswith(c: &Call) -> EvalResult {
    caseless(c, |t, sub| t.starts_with(sub))
}

fn f_endswith(c: &Call) -> EvalResult {
    caseless(c, |t, sub| t.ends_with(sub))
}

fn f_replace(c: &Call) -> EvalResult {
    with_text(c, |t| {
        let (find, with) = (text(c, 1)?, text(c, 2)?);
        let (Some(find), Some(with)) = (find, with) else {
            return Ok(Value::Null);
        };
        s(if find.is_empty() {
            t.to_string()
        } else {
            t.replace(&find, &with)
        })
    })
}

fn f_regexmatch(c: &Call) -> EvalResult {
    with_text(c, |t| {
        let Some(pattern) = text(c, 1)? else {
            return Ok(Value::Null);
        };
        if t.encode_utf16().count() > MAX_REGEX_INPUT {
            return fail(
                c,
                0,
                format!("text is longer than {MAX_REGEX_INPUT} characters"),
            );
        }
        match compile_safe_regex(&pattern).and_then(|re| re.is_match(t)) {
            Ok(hit) => Ok(Value::Bool(hit)),
            Err(message) => fail(c, 1, message),
        }
    })
}

fn rounding(c: &Call, op: fn(f64) -> f64) -> EvalResult {
    let n = num(c, 0)?;
    let digits = int(c, 1, Some(0.0))?;
    let (Some(n), Some(digits)) = (n, digits) else {
        return Ok(Value::Null);
    };
    let scale = 10f64.powf(digits);
    Ok(num_value(normalize_number(
        op(normalize_number(n * scale)) / scale,
    )))
}

fn f_round(c: &Call) -> EvalResult {
    let n = num(c, 0)?;
    let digits = int(c, 1, Some(0.0))?;
    match (n, digits) {
        (Some(n), Some(digits)) => Ok(num_value(round_to(n, digits))),
        _ => Ok(Value::Null),
    }
}

fn f_floor(c: &Call) -> EvalResult {
    rounding(c, f64::floor)
}

fn f_ceil(c: &Call) -> EvalResult {
    rounding(c, f64::ceil)
}

fn f_abs(c: &Call) -> EvalResult {
    Ok(num(c, 0)?.map_or(Value::Null, |n| num_value(n.abs())))
}

fn f_min(c: &Call) -> EvalResult {
    extreme(c, all_args(c)?, -1)
}

fn f_max(c: &Call) -> EvalResult {
    extreme(c, all_args(c)?, 1)
}

fn f_sum(c: &Call) -> EvalResult {
    Ok(num_value(sum_of(&numbers(c, &all_args(c)?)?)))
}

fn f_avg(c: &Call) -> EvalResult {
    Ok(average(&numbers(c, &all_args(c)?)?))
}

fn f_count(c: &Call) -> EvalResult {
    let n = all_args(c)?.iter().filter(|v| !v.is_null()).count();
    Ok(num_value(n as f64))
}

fn f_sumof(c: &Call) -> EvalResult {
    let values = per_item(c)?.unwrap_or_default();
    Ok(num_value(sum_of(&numbers(c, &values)?)))
}

fn f_avgof(c: &Call) -> EvalResult {
    let values = per_item(c)?.unwrap_or_default();
    Ok(average(&numbers(c, &values)?))
}

fn f_minof(c: &Call) -> EvalResult {
    extreme(c, per_item(c)?.unwrap_or_default(), -1)
}

fn f_maxof(c: &Call) -> EvalResult {
    extreme(c, per_item(c)?.unwrap_or_default(), 1)
}

fn f_countof(c: &Call) -> EvalResult {
    let mut n = 0;
    for v in per_item(c)?.unwrap_or_default() {
        if !v.is_null() && !v.is_boolean() {
            return fail(
                c,
                1,
                format!("condition must be true or false but got {}", type_name(&v)),
            );
        }
        if v == Value::Bool(true) {
            n += 1;
        }
    }
    Ok(num_value(f64::from(n)))
}

fn f_today(c: &Call) -> EvalResult {
    s(format_iso_date(c.env.now_ms()))
}

fn f_now(c: &Call) -> EvalResult {
    s(format_iso_datetime(c.env.now_ms()))
}

fn f_date(c: &Call) -> EvalResult {
    let y = int(c, 0, None)?;
    let m = int(c, 1, None)?;
    let d = int(c, 2, None)?;
    let (Some(y), Some(m), Some(d)) = (y, m, d) else {
        return Ok(Value::Null);
    };
    let in_range = |n: f64| n.abs() < 1e12;
    let ms = if in_range(y) && in_range(m) && in_range(d) {
        utc_ms(y as i64, m as i64 - 1, d as i64, 0, 0, 0)
    } else {
        None
    };
    match ms.filter(|ms| in_supported_years(*ms)) {
        Some(ms) => s(format_iso_date(ms)),
        None => fail(c, 0, "year must be between 1 and 9999"),
    }
}

fn date_part(c: &Call, part: fn(&chrono::DateTime<chrono::Utc>) -> u32) -> EvalResult {
    Ok(date(c, 0)?.map_or(Value::Null, |d| num_value(f64::from(part(&datetime(d.ms))))))
}

fn f_year(c: &Call) -> EvalResult {
    Ok(date(c, 0)?.map_or(Value::Null, |d| num_value(f64::from(datetime(d.ms).year()))))
}

fn f_month(c: &Call) -> EvalResult {
    date_part(c, |d| d.month())
}

fn f_day(c: &Call) -> EvalResult {
    date_part(c, |d| d.day())
}

fn f_hour(c: &Call) -> EvalResult {
    date_part(c, |d| d.hour())
}

fn f_minute(c: &Call) -> EvalResult {
    date_part(c, |d| d.minute())
}

fn f_weekday(c: &Call) -> EvalResult {
    date_part(c, |d| d.weekday().num_days_from_sunday() + 1)
}

fn f_datediff(c: &Call) -> EvalResult {
    let unit = unit_arg(c)?;
    let a = date(c, 1)?;
    let b = date(c, 2)?;
    let (Some(unit), Some(a), Some(b)) = (unit, a, b) else {
        return Ok(Value::Null);
    };
    Ok(num_value(date_diff(unit, a.ms, b.ms) as f64))
}

fn f_dateadd(c: &Call) -> EvalResult {
    let unit = unit_arg(c)?;
    let n = int(c, 1, None)?;
    let d = date(c, 2)?;
    let (Some(unit), Some(n), Some(d)) = (unit, n, d) else {
        return Ok(Value::Null);
    };
    let Some(ms) = date_add(unit, n, d.ms).filter(|ms| in_supported_years(*ms)) else {
        return fail(c, 1, "result is outside years 1 to 9999");
    };
    let time_unit = matches!(unit, Unit::Hour | Unit::Minute | Unit::Second);
    s(if d.has_time || time_unit {
        format_iso_datetime(ms)
    } else {
        format_iso_date(ms)
    })
}

fn format_arg(c: &Call, value: &Value, i: usize) -> EvalResult {
    let pattern = text(c, i)?;
    let Some(pattern) = pattern.filter(|_| !value.is_null()) else {
        return Ok(Value::Null);
    };
    if matches!(value, Value::Array(_) | Value::Object(_)) {
        return fail(c, 0, format!("can't format {}", type_name(value)));
    }
    match format_value(value, Some(&pattern)) {
        Ok(out) => s(out),
        Err(e) => fail(c, 0, e.0),
    }
}

fn f_format(c: &Call) -> EvalResult {
    let v = c.arg(0)?;
    format_arg(c, &v, 1)
}

fn f_text(c: &Call) -> EvalResult {
    let v = c.arg(0)?;
    if c.count() > 1 {
        return format_arg(c, &v, 1);
    }
    match v {
        Value::Null => Ok(Value::Null),
        Value::Array(_) | Value::Object(_) => {
            fail(c, 0, format!("can't convert {} to text", type_name(&v)))
        }
        other => s(to_text(&other)),
    }
}

/// `[+-]?(\d+(\.\d*)?|\.\d+)(e[+-]?\d+)?`, case-insensitive.
fn is_number_text(t: &str) -> bool {
    let b = t.as_bytes();
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let int_len = digits(i);
    if int_len > 0 {
        i += int_len;
        if b.get(i) == Some(&b'.') {
            i += 1 + digits(i + 1);
        }
    } else if b.get(i) == Some(&b'.') && digits(i + 1) > 0 {
        i += 1 + digits(i + 1);
    } else {
        return false;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let exp = digits(j);
        if exp == 0 {
            return false;
        }
        i = j + exp;
    }
    i == b.len()
}

fn f_number(c: &Call) -> EvalResult {
    match c.arg(0)? {
        Value::Null => Ok(Value::Null),
        n @ Value::Number(_) => Ok(n),
        Value::Bool(b) => Ok(num_value(f64::from(u8::from(b)))),
        Value::String(t) => {
            let cleaned = js_trim(&t).replace(',', "");
            let parsed = is_number_text(&cleaned)
                .then(|| cleaned.parse::<f64>().ok())
                .flatten();
            Ok(parsed.map_or(Value::Null, num_value))
        }
        other => fail(
            c,
            0,
            format!("can't convert {} to a number", type_name(&other)),
        ),
    }
}

const MANY: usize = usize::MAX;

const fn def(
    names: &'static [&'static str],
    min: usize,
    max: usize,
    imp: fn(&Call) -> EvalResult,
) -> FnDef {
    FnDef {
        names,
        min,
        max,
        per_item_arg: None,
        imp,
    }
}

const fn per_item_def(names: &'static [&'static str], imp: fn(&Call) -> EvalResult) -> FnDef {
    FnDef {
        names,
        min: 2,
        max: 2,
        per_item_arg: Some(1),
        imp,
    }
}

static FUNCTIONS: &[FnDef] = &[
    def(&["if", "iif"], 2, 3, f_if),
    def(&["coalesce", "nz"], 1, MANY, f_coalesce),
    def(&["isnull"], 1, 1, f_isnull),
    def(&["isblank"], 1, 1, f_isblank),
    def(&["len"], 1, 1, f_len),
    def(&["lower"], 1, 1, f_lower),
    def(&["upper"], 1, 1, f_upper),
    def(&["trim"], 1, 1, f_trim),
    def(&["left"], 1, 2, f_left),
    def(&["right"], 1, 2, f_right),
    def(&["mid", "substr"], 2, 3, f_mid),
    def(&["concat"], 1, MANY, f_concat),
    def(&["contains"], 2, 2, f_contains),
    def(&["startswith"], 2, 2, f_startswith),
    def(&["endswith"], 2, 2, f_endswith),
    def(&["replace"], 3, 3, f_replace),
    def(&["regexmatch"], 2, 2, f_regexmatch),
    def(&["round"], 1, 2, f_round),
    def(&["floor"], 1, 2, f_floor),
    def(&["ceil", "ceiling"], 1, 2, f_ceil),
    def(&["abs"], 1, 1, f_abs),
    def(&["min"], 1, MANY, f_min),
    def(&["max"], 1, MANY, f_max),
    def(&["sum"], 1, MANY, f_sum),
    def(&["avg", "average"], 1, MANY, f_avg),
    def(&["count"], 1, MANY, f_count),
    per_item_def(&["sumof"], f_sumof),
    per_item_def(&["avgof"], f_avgof),
    per_item_def(&["minof"], f_minof),
    per_item_def(&["maxof"], f_maxof),
    per_item_def(&["countof"], f_countof),
    def(&["today"], 0, 0, f_today),
    def(&["now"], 0, 0, f_now),
    def(&["date"], 3, 3, f_date),
    def(&["year"], 1, 1, f_year),
    def(&["month"], 1, 1, f_month),
    def(&["day"], 1, 1, f_day),
    def(&["hour"], 1, 1, f_hour),
    def(&["minute"], 1, 1, f_minute),
    def(&["weekday"], 1, 1, f_weekday),
    def(&["datediff"], 3, 3, f_datediff),
    def(&["dateadd"], 3, 3, f_dateadd),
    def(&["format"], 2, 2, f_format),
    def(&["text"], 1, 2, f_text),
    def(&["number"], 1, 1, f_number),
];

fn table() -> &'static HashMap<&'static str, &'static FnDef> {
    static TABLE: OnceLock<HashMap<&'static str, &'static FnDef>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut map = HashMap::new();
        for f in FUNCTIONS {
            for name in f.names {
                map.insert(*name, f);
            }
        }
        map
    })
}

pub fn lookup_function(name: &str) -> Option<&'static FnDef> {
    table().get(name).copied()
}

/// Every function name and alias, sorted.
pub fn function_names() -> &'static [&'static str] {
    static NAMES: OnceLock<Vec<&'static str>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names: Vec<&'static str> = table().keys().copied().collect();
        names.sort_unstable();
        names
    })
}

pub fn arity_message(name: &str, def: &FnDef, count: usize) -> Option<String> {
    if count >= def.min && count <= def.max {
        return None;
    }
    let range = if def.min == def.max {
        def.min.to_string()
    } else if def.max == MANY {
        format!("at least {}", def.min)
    } else {
        format!("{} to {}", def.min, def.max)
    };
    let plural = if range == "1" { "" } else { "s" };
    Some(format!(
        "{name}() takes {range} argument{plural} but got {count}"
    ))
}
