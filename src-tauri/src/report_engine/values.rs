//! Values of text components: running sums and conditional formatting (PRD §15).

use super::component::{Comp, ConditionStyle, Kind};
use super::render::{RenderContext, ScopeData};
use crate::expr;
use chrono::{DateTime, Utc};
use serde_json::Value;

pub const ERROR_TEXT: &str = "#Error";
pub const RUNNING_NOT_NUMBER: &str = "A running sum needs a number";

/// Accumulated value of one running-sum component at one band instance.
#[derive(Debug, Clone, PartialEq)]
pub enum RunningValue {
    Value(f64),
    Error(String),
}

/// Sum to 15 significant digits, like `expr` arithmetic.
fn add(a: f64, b: f64) -> f64 {
    let n = a + b;
    if n == 0.0 || !n.is_finite() {
        return 0.0;
    }
    format!("{:.14e}", n).parse().unwrap_or(n)
}

/// A number as an expression value: whole numbers become integers.
pub fn num_value(n: f64) -> Value {
    if !n.is_finite() {
        return Value::Null;
    }
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        return Value::from(n as i64);
    }
    serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number)
}

/// Adds one instance's value of a running-sum component to `previous`. Null adds
/// nothing, booleans count as 1 or 0, and anything else that isn't a number is an error
/// that stays until the sum restarts.
pub fn accumulate(
    previous: Option<&RunningValue>,
    c: &Comp,
    scope: &ScopeData,
    now: Option<DateTime<Utc>>,
) -> RunningValue {
    if let Some(RunningValue::Error(e)) = previous {
        return RunningValue::Error(e.clone());
    }
    let sum = match previous {
        Some(RunningValue::Value(v)) => *v,
        _ => 0.0,
    };
    let expression = c.expression();
    if expression.trim().is_empty() {
        return RunningValue::Value(sum);
    }
    match scope.eval(expression, &[], now) {
        Err(e) => RunningValue::Error(e.message),
        Ok(Value::Null) => RunningValue::Value(sum),
        Ok(Value::Bool(b)) => RunningValue::Value(add(sum, if b { 1.0 } else { 0.0 })),
        Ok(Value::Number(n)) => match n.as_f64().filter(|v| v.is_finite()) {
            Some(v) => RunningValue::Value(add(sum, v)),
            None => RunningValue::Error(RUNNING_NOT_NUMBER.into()),
        },
        Ok(_) => RunningValue::Error(RUNNING_NOT_NUMBER.into()),
    }
}

/// Value and printed text of a text component. A running sum takes its value from
/// `ctx.running` when the band instance carries one (detail and group bands); elsewhere
/// it prints the plain value. Errors print `#Error` and add a diagnostic.
pub fn component_value(c: &Comp, ctx: &RenderContext) -> (Value, String) {
    if let Kind::StaticText { text } = &c.kind {
        return (Value::String(text.clone()), text.clone());
    }
    let running = if c.running_sum.is_some() {
        ctx.running.and_then(|m| m.get(&c.id))
    } else {
        None
    };
    let expression = c.expression();
    let result: Result<Value, String> = match running {
        Some(RunningValue::Error(e)) => Err(e.clone()),
        Some(RunningValue::Value(v)) => Ok(num_value(*v)),
        None if expression.trim().is_empty() => return (Value::Null, String::new()),
        None => ctx.scope.eval(expression, &[], ctx.now()).map_err(|e| e.message),
    };
    let formatted = result.and_then(|value| {
        let format = c.format().filter(|f| !f.is_empty()).or_else(|| default_number_format(c, &value, ctx));
        expr::format_value(&value, format)
            .map(|text| (value, text))
            .map_err(|e| e.0)
    });
    formatted.unwrap_or_else(|message| {
        ctx.diagnose(&c.id, &message);
        (Value::Null, ERROR_TEXT.to_string())
    })
}

/// Two-decimal format for a bound number field without a format whose column holds
/// fractions, so detail amounts line up with formatted subtotals (20.00, 75.50).
fn default_number_format(c: &Comp, value: &Value, ctx: &RenderContext) -> Option<&'static str> {
    if !matches!(c.kind, Kind::Field { .. }) || !value.is_number() {
        return None;
    }
    let expression = c.expression().trim();
    let column = expression
        .strip_prefix("record.[")
        .and_then(|r| r.strip_suffix(']'))
        .or_else(|| expression.strip_prefix("record.").filter(|r| !r.contains(['.', '[', '(', ' '])))?;
    let fractional = ctx.env.fractional_columns.contains(column);
    fractional.then_some("#,##0.00")
}

/// Style of the first conditional formatting rule whose `when` holds, else `None`.
pub fn condition_style<'a>(c: &'a Comp, value: &Value, ctx: &RenderContext) -> Option<&'a ConditionStyle> {
    for rule in &c.conditions {
        if rule.when.trim().is_empty() {
            continue;
        }
        match ctx.scope.eval_bool(&rule.when, &[("value", value.clone())], ctx.now()) {
            Ok(true) => return Some(&rule.style),
            Ok(false) => {}
            Err(e) => ctx.diagnose(&c.id, &format!("Condition: {}", e.message)),
        }
    }
    None
}

/// The component with its matching condition's style applied, and the text it prints.
pub fn styled_component(c: &Comp, ctx: &RenderContext) -> (Comp, String) {
    let (value, text) = component_value(c, ctx);
    let style = if c.conditions.is_empty() {
        None
    } else {
        condition_style(c, &value, ctx)
    };
    let Some(style) = style else {
        return (c.clone(), text);
    };
    let mut merged = c.clone();
    if let Some(bold) = style.bold {
        merged.style.bold = Some(bold);
    }
    if let Some(gray) = style.gray {
        merged.style.gray = Some(gray);
    }
    if let Some(fill) = style.fill {
        merged.style.fill = fill;
    }
    (merged, text)
}
