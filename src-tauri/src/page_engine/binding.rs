//! Which record an embedded detail or edit form shows (the async chain of
//! `EmbeddedForm.tsx`). A named record comes from the `recordId` expression; a blank
//! expression opens the first row of the form's source that passes its row filter. The form's
//! row filter holds for named records too.

use crate::design::{Form, SourceKind};
use crate::expr::{self, EvaluateOptions, Scope};
use crate::forms::data::{first_record_id, load_record, passes_filter, source_params};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Resolved {
    /// The record to show.
    Ready(Value),
    /// The source has no row to show.
    Empty,
    /// The record shown before was deleted.
    Gone,
    /// The named record does not pass the form's row filter.
    Outside,
}

/// The table a form reads, for key lookups (None for query sources).
pub fn form_table(form: &Form) -> Option<&str> {
    form.source.as_ref().filter(|s| s.kind == SourceKind::Table).and_then(|s| s.table.as_deref())
}

/// The record id expression in force: query-sourced forms take a whole row, so a stored key
/// expression is ignored.
pub fn effective_expression<'a>(form: &Form, record_id: Option<&'a str>) -> &'a str {
    if form_table(form).is_some() { record_id.map_or("", str::trim) } else { "" }
}

fn missing(value: &Value) -> bool {
    value.is_null() || value.as_str().is_some_and(str::is_empty)
}

/// Resolves the record for `form`. `previous` is the first-row record shown before (only
/// used without an expression): if it was deleted the result is `Gone`. `scope` holds `app`
/// and `params`. Errors are the message to show.
pub fn resolve(label: &str, form: &Form, expression: &str, previous: Option<&Value>, scope: &Scope) -> Result<Resolved, String> {
    let table = form_table(form);
    let previous = if expression.is_empty() { previous } else { None };
    if let (Some(previous), Some(table)) = (previous, table) {
        if load_record(label, table, previous).map_err(|e| e.message)?.is_none() {
            return Ok(Resolved::Gone);
        }
    }
    let record_id = if expression.is_empty() {
        let bound = source_params(form, scope)?;
        first_record_id(label, form, &bound, scope).map_err(|e| e.message)?
    } else {
        Some(expr::evaluate(expression, scope, EvaluateOptions::default()).map_err(|e| e.message)?)
    };
    let filtered = form.filter.as_deref().is_some_and(|f| !f.trim().is_empty());
    if let (false, Some(table), true, Some(id)) = (expression.is_empty(), table, filtered, record_id.as_ref().filter(|id| !missing(id))) {
        if let Some(hit) = load_record(label, table, id).map_err(|e| e.message)? {
            if !passes_filter(form, &hit.record, scope).map_err(|e| e.message)? {
                return Ok(Resolved::Outside);
            }
        }
    }
    Ok(match record_id {
        Some(id) if !missing(&id) => Resolved::Ready(id),
        _ => Resolved::Empty,
    })
}
