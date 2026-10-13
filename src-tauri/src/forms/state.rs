//! Form expressions and validation: conditions, computed values, per-control and whole-form
//! validation, and container-inherited visibility/enabled state (port of
//! `src/runtime/formState.ts`).

use super::fields::{field_text, mask_problem, rich_text_plain, value_text};
use super::schema::{has_enabled_state, is_input_kind};
use super::values::{RecordValues, is_blank};
use crate::design::{Control, ControlKind, Form};
use crate::expr::{self, EvaluateOptions, Scope};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

/// Expression scope for forms: `record`, `form` (form state), `app` (app state including
/// user and role), and `value` while validating one control.
#[derive(Clone, Debug)]
pub struct FormScope {
    scope: Scope,
}

impl FormScope {
    pub fn new(record: RecordValues, form: Map<String, Value>, app: Map<String, Value>) -> Self {
        let mut scope = Scope::new();
        scope.insert("record".into(), Value::Object(record));
        scope.insert("form".into(), Value::Object(form));
        scope.insert("app".into(), Value::Object(app));
        Self { scope }
    }

    /// An empty form state and the given app state.
    pub fn of(record: RecordValues, app: Map<String, Value>) -> Self {
        Self::new(record, Map::new(), app)
    }

    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    pub fn record(&self) -> &RecordValues {
        match self.scope.get("record") {
            Some(Value::Object(map)) => map,
            _ => unreachable!("a form scope always has a record"),
        }
    }

    /// The same scope with `value` set (validating one control).
    pub fn with_value(&self, value: Value) -> Self {
        let mut scope = self.scope.clone();
        scope.insert("value".into(), value);
        Self { scope }
    }

    /// The same scope with another record.
    pub fn with_record(&self, record: RecordValues) -> Self {
        let mut scope = self.scope.clone();
        scope.insert("record".into(), Value::Object(record));
        Self { scope }
    }
}

fn present(src: Option<&str>) -> Option<&str> {
    src.filter(|s| !s.trim().is_empty())
}

/// Names an expression in this form may use; `record.<column>` limits record fields to
/// known columns.
pub fn known_names(columns: &[String]) -> Vec<String> {
    let mut names: Vec<String> = if columns.is_empty() {
        vec!["record".into()]
    } else {
        columns.iter().map(|c| format!("record.{c}")).collect()
    };
    names.extend(["form", "app", "value"].map(String::from));
    names
}

/// Diagnostics text for an expression field in the designer ("" when valid).
pub fn expression_problem(src: Option<&str>, columns: &[String], names: Option<&[String]>) -> String {
    let Some(src) = present(src) else {
        return String::new();
    };
    let names = names.map_or_else(|| known_names(columns), <[String]>::to_vec);
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    expr::check(src, Some(&refs))
        .iter()
        .map(|d| d.message.clone())
        .collect::<Vec<_>>()
        .join("; ")
}

/// A condition's outcome plus the evaluation error, if any (the value is then false).
#[derive(Clone, Debug, PartialEq)]
pub struct ConditionResult {
    pub value: bool,
    pub error: Option<String>,
}

/// Like [`condition`], but reports why an expression failed so the UI can show it.
pub fn condition_result(src: Option<&str>, scope: &Scope, fallback: bool) -> ConditionResult {
    let Some(src) = present(src) else {
        return ConditionResult {
            value: fallback,
            error: None,
        };
    };
    match expr::evaluate_boolean(src, scope, EvaluateOptions::default()) {
        Ok(value) => ConditionResult { value, error: None },
        Err(e) => ConditionResult {
            value: false,
            error: Some(e.message),
        },
    }
}

/// Evaluates a condition; a blank expression gives `fallback`, an error gives false.
pub fn condition(src: Option<&str>, scope: &Scope, fallback: bool) -> bool {
    condition_result(src, scope, fallback).value
}

#[derive(Clone, Debug, PartialEq)]
pub struct Computed {
    pub value: Value,
    pub error: Option<String>,
}

pub fn compute(src: Option<&str>, scope: &Scope) -> Computed {
    let Some(src) = present(src) else {
        return Computed {
            value: Value::Null,
            error: None,
        };
    };
    match expr::evaluate(src, scope, EvaluateOptions::default()) {
        Ok(value) => Computed { value, error: None },
        Err(e) => Computed {
            value: Value::Null,
            error: Some(e.message),
        },
    }
}

/// Formats a value with the control's pattern, falling back to plain text.
pub fn formatted(value: &Value, pattern: Option<&str>) -> String {
    if value.is_null() {
        return String::new();
    }
    if let Some(pattern) = pattern.filter(|p| !p.is_empty()) {
        return expr::format_value(value, Some(pattern)).unwrap_or_else(|_| value_text(value));
    }
    match value {
        Value::Bool(b) => if *b { "Yes" } else { "No" }.to_string(),
        other => value_text(other),
    }
}

/// Whole-value regex match; an invalid pattern never blocks saving.
fn matches_pattern(pattern: &str, value: &Value) -> bool {
    let mut scope = Scope::new();
    scope.insert("v".into(), Value::String(value_text(value)));
    scope.insert("p".into(), Value::String(format!("^(?:{pattern})$")));
    match expr::evaluate("regexmatch(v, p)", &scope, EvaluateOptions::default()) {
        Ok(Value::Bool(hit)) => hit,
        _ => true,
    }
}

/// One control's validation message, or None when valid.
pub fn validate_control(control: &Control, value: &Value, scope: &FormScope) -> Option<String> {
    let rules = &control.validation;
    let custom = rules
        .message
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty());
    let label = if !control.label.is_empty() {
        control.label.clone()
    } else {
        control
            .binding
            .as_ref()
            .map(|b| b.column.clone())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "This field".into())
    };
    let message = |default: String| Some(custom.map_or(default, str::to_string));
    let blank = if control.kind == ControlKind::RichText {
        rich_text_plain(value).trim().is_empty()
    } else {
        is_blank(value)
    };
    if blank {
        return if rules.required {
            message(format!("{label} is required."))
        } else {
            None
        };
    }
    if let Some(mask) = control.input_mask.as_deref().filter(|m| !m.is_empty()) {
        if let Some(problem) = mask_problem(mask, value, &label) {
            return message(problem);
        }
    }
    let number = match value {
        Value::Number(n) => n.as_f64(),
        other => value_text(other).trim().parse::<f64>().ok(),
    }
    .filter(|n| n.is_finite());
    if let (Some(min), Some(n)) = (rules.min, number) {
        if n < min {
            return message(format!("{label} must be at least {}.", number_text(min)));
        }
    }
    if let (Some(max), Some(n)) = (rules.max, number) {
        if n > max {
            return message(format!("{label} must be at most {}.", number_text(max)));
        }
    }
    if let Some(pattern) = rules.pattern.as_deref().filter(|p| !p.is_empty()) {
        if !matches_pattern(pattern, value) {
            return message(format!("{label} is not in the expected format."));
        }
    }
    if let Some(expression) = present(rules.expression.as_deref()) {
        let with_value = scope.with_value(value.clone());
        if !condition(Some(expression), with_value.scope(), true) {
            return message(format!("{label} is not valid."));
        }
    }
    None
}

fn number_text(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

const FIELD_KINDS: [&str; 3] = ["richText", "attachment", "multiSelect"];

fn kind_key(kind: ControlKind) -> &'static str {
    match kind {
        ControlKind::RichText => "richText",
        ControlKind::Attachment => "attachment",
        ControlKind::MultiSelect => "multiSelect",
        _ => "",
    }
}

/// Text of a value for list cells: a field format's text, else the control's format.
pub fn cell_text(value: &Value, control: Option<&Control>, field_format: Option<&str>) -> String {
    let kind = field_format
        .filter(|f| !f.is_empty())
        .or_else(|| control.map(|c| kind_key(c.kind)).filter(|k| !k.is_empty()));
    match kind {
        Some(kind) if FIELD_KINDS.contains(&kind) => field_text(value, Some(kind)),
        _ => formatted(value, control.and_then(|c| c.format.as_deref())),
    }
}

/// Controls for which `own(control)` holds and holds for every ancestor container, so a
/// hidden or disabled section or tab group passes that state to everything inside it.
fn inherited(form: &Form, own: impl Fn(&Control) -> bool) -> HashSet<String> {
    let by_id: HashMap<&str, &Control> =
        form.controls.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut memo: HashMap<String, bool> = HashMap::new();
    fn holds(
        control: &Control,
        by_id: &HashMap<&str, &Control>,
        own: &dyn Fn(&Control) -> bool,
        memo: &mut HashMap<String, bool>,
        depth: u32,
    ) -> bool {
        if let Some(known) = memo.get(&control.id) {
            return *known;
        }
        let parent = control
            .parent
            .as_ref()
            .and_then(|p| by_id.get(p.id.as_str()));
        let result = own(control)
            && parent.is_none_or(|p| depth > 20 || holds(p, by_id, own, memo, depth + 1));
        memo.insert(control.id.clone(), result);
        result
    }
    form.controls
        .iter()
        .filter(|c| holds(c, &by_id, &own, &mut memo, 0))
        .map(|c| c.id.clone())
        .collect()
}

/// Container visibility: a control is hidden when it or any ancestor container is hidden.
pub fn visible_controls(form: &Form, scope: &FormScope) -> HashSet<String> {
    inherited(form, |c| condition(c.visible_when.as_deref(), scope.scope(), true))
}

/// Container enabled state: a control is disabled when it or any ancestor container is.
/// `enabled_when` on a static kind (label, image) does not apply and is ignored.
pub fn enabled_controls(form: &Form, scope: &FormScope) -> HashSet<String> {
    inherited(form, |c| {
        !has_enabled_state(c.kind) || condition(c.enabled_when.as_deref(), scope.scope(), true)
    })
}

/// Validation messages by control id (in control order) and form-level messages.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormErrors {
    pub fields: Vec<(String, String)>,
    pub form: Vec<String>,
}

impl FormErrors {
    pub fn field(&self, control_id: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(id, _)| id == control_id)
            .map(|(_, m)| m.as_str())
    }
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty() && self.form.is_empty()
    }
}

pub fn has_errors(errors: &FormErrors) -> bool {
    !errors.is_empty()
}

/// Validates every visible, enabled bound control and the form-level rules. Like read-only
/// controls, disabled ones (or ones in a disabled container) are skipped: the user cannot
/// change them, so their errors could not be fixed.
pub fn validate_form(form: &Form, scope: &FormScope) -> FormErrors {
    let visible = visible_controls(form, scope);
    let enabled = enabled_controls(form, scope);
    let mut fields = vec![];
    for control in &form.controls {
        if !visible.contains(&control.id) || !enabled.contains(&control.id) {
            continue;
        }
        if !is_input_kind(control.kind) || control.read_only {
            continue;
        }
        let value = control
            .binding
            .as_ref()
            .and_then(|b| scope.record().get(&b.column))
            .cloned()
            .unwrap_or(Value::Null);
        if let Some(message) = validate_control(control, &value, scope) {
            fields.push((control.id.clone(), message));
        }
    }
    let messages = form
        .rules
        .iter()
        .filter(|rule| {
            present(Some(&rule.expression))
                .is_some_and(|e| !condition(Some(e), scope.scope(), true))
        })
        .map(|rule| {
            if rule.message.is_empty() {
                "The record is not valid.".to_string()
            } else {
                rule.message.clone()
            }
        })
        .collect();
    FormErrors {
        fields,
        form: messages,
    }
}

/// Columns the user cannot edit right now because their only controls are disabled (by
/// their own `enabled_when` or a container's). Their edits are left out of a save so the
/// stored value is kept.
pub fn disabled_columns(form: &Form, scope: &FormScope) -> HashSet<String> {
    let enabled = enabled_controls(form, scope);
    let mut editable = HashSet::new();
    let mut disabled = HashSet::new();
    for control in &form.controls {
        let Some(column) = control.binding.as_ref().map(|b| b.column.clone()) else {
            continue;
        };
        if column.is_empty() || !is_input_kind(control.kind) {
            continue;
        }
        if enabled.contains(&control.id) {
            editable.insert(column);
        } else {
            disabled.insert(column);
        }
    }
    disabled.retain(|c| !editable.contains(c));
    disabled
}

/// Initial values for create mode from controls' default-value expressions.
pub fn default_record(form: &Form, form_state: &Map<String, Value>, app: &Map<String, Value>) -> RecordValues {
    let mut record = RecordValues::new();
    for control in &form.controls {
        let Some(column) = control.binding.as_ref().map(|b| b.column.clone()) else {
            continue;
        };
        let Some(src) = present(control.default_value.as_deref()) else {
            continue;
        };
        if column.is_empty() {
            continue;
        }
        let scope = FormScope::new(record.clone(), form_state.clone(), app.clone());
        let value = compute(Some(src), scope.scope()).value;
        record.insert(column, value);
    }
    record
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
