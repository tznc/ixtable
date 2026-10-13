//! Expression-driven row filters and conditional styles (PRD §17.1; port of
//! `src/runtime/conditions.ts`). Evaluated with `crate::expr`; Rust only stores the text.

use crate::design::{ConditionalStyle, Tone};
use crate::expr::{self, EvaluateOptions, ExprError, Scope};
use serde_json::{Map, Value};
use std::sync::Arc;

fn present(src: Option<&str>) -> Option<&str> {
    src.filter(|s| !s.trim().is_empty())
}

/// True when the condition holds; blank, invalid or failing expressions give false.
fn holds(src: &str, scope: &Scope) -> bool {
    expr::evaluate_boolean(src, scope, EvaluateOptions::default()).unwrap_or(false)
}

fn is_known_tone(tone: &Tone) -> bool {
    !matches!(tone, Tone::Other(_))
}

/// Tone of the first matching rule (optionally only rules for `column`), else None.
pub fn tone_for(rules: &[ConditionalStyle], scope: &Scope, column: Option<&str>) -> Option<Tone> {
    rules
        .iter()
        .filter(|rule| column.is_none_or(|c| rule.column.as_deref() == Some(c)))
        .find(|rule| {
            present(Some(&rule.when)).is_some() && is_known_tone(&rule.tone) && holds(&rule.when, scope)
        })
        .map(|rule| rule.tone.clone())
}

/// CSS-style class name of a tone ("" when none), used as the test-visible tone marker.
pub fn tone_class(tone: Option<&Tone>) -> String {
    tone.map_or(String::new(), |t| format!("tone tone-{}", t.as_str()))
}

/// Names a row filter may use: the row as `record`, plus `parent`, `form`, `app`, `params`.
pub fn filter_names(columns: &[String]) -> Vec<String> {
    let mut names: Vec<String> = if columns.is_empty() {
        vec!["record".into()]
    } else {
        columns.iter().map(|c| format!("record.{c}")).collect()
    };
    names.extend(["parent", "form", "app", "params"].map(String::from));
    names
}

/// The part of `scope` a row filter reads: every name it references except the row itself
/// (`record`), copied into a pruned scope. Two scopes with equal inputs give equal JSON, so
/// callers key reloads on it. A filter that does not parse gives an empty scope.
pub fn filter_inputs(src: Option<&str>, scope: &Scope) -> Scope {
    let mut picked = Map::new();
    let Some(src) = present(src) else {
        return picked;
    };
    let Ok(mut names) = expr::try_referenced_names(src) else {
        return picked;
    };
    // Shorter paths first: once `parent` is copied whole, `parent.x` adds nothing.
    names.sort_by_key(|n| n.split('.').count());
    let mut taken: Vec<String> = vec![];
    for name in names {
        if name == "record" || name.starts_with("record.") {
            continue;
        }
        if taken.iter().any(|t| name.starts_with(&format!("{t}."))) {
            continue;
        }
        taken.push(name.clone());
        let path: Vec<&str> = name.split('.').collect();
        copy_path(&mut picked, scope, &path);
    }
    picked
}

fn copy_path(target: &mut Map<String, Value>, source: &Map<String, Value>, path: &[&str]) {
    let key = path[0];
    // A missing value is left out (JSON has no undefined).
    let Some(value) = source.get(key).cloned() else {
        return;
    };
    let nested = value.as_object();
    // A missing or scalar value is copied as is; reading past it fails the same way.
    if path.len() == 1 || nested.is_none() {
        target.insert(key.to_string(), value);
        return;
    }
    let entry = target
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if let (Some(inner), Some(inner_source)) = (entry.as_object_mut(), nested) {
        copy_path(inner, inner_source, &path[1..]);
    }
}

/// Stable key of [`filter_inputs`] ("" without a filter): reload a filtered list when it changes.
pub fn filter_inputs_key(src: Option<&str>, scope: &Scope) -> String {
    match present(src) {
        None => String::new(),
        Some(src) => serde_json::to_string(&Value::Array(vec![
            Value::String(src.into()),
            Value::Object(filter_inputs(Some(src), scope)),
        ]))
        .unwrap_or_default(),
    }
}

pub type RowPredicate = Arc<dyn Fn(&Map<String, Value>) -> bool + Send + Sync>;

/// Compiles a filter expression into a row predicate, or None when the filter is blank.
/// A syntax error is returned so the caller can show it; a row whose evaluation fails is
/// excluded (null and errors count as false, as in every other condition).
pub fn row_filter(src: Option<&str>, scope: &Scope) -> Result<Option<RowPredicate>, ExprError> {
    let Some(src) = present(src) else {
        return Ok(None);
    };
    let ast = expr::parse_cached(src)?;
    let base = scope.clone();
    Ok(Some(Arc::new(move |record| {
        let mut scope = base.clone();
        scope.insert("record".into(), Value::Object(record.clone()));
        expr::evaluate_ast_boolean(&ast, &scope, EvaluateOptions::default()).unwrap_or(false)
    })))
}

/// Rows read per chunk while scanning for filtered matches (`read_table_page` allows 1000).
pub const SCAN_CHUNK: usize = 500;
/// Hard stop, so a filter that matches almost nothing cannot scan a huge table forever.
pub const SCAN_LIMIT: usize = 50_000;

pub struct Scan<T> {
    pub matches: Vec<T>,
    pub exhausted: bool,
}

/// Reads a source in chunks and keeps the items that pass `keep`, until at least `need`
/// matches are found, the source ends (`exhausted`), or `SCAN_LIMIT` rows were read.
pub fn scan_filtered<T, E>(
    mut fetch: impl FnMut(usize, usize) -> Result<Vec<T>, E>,
    keep: impl Fn(&T) -> bool,
    need: usize,
    chunk: usize,
) -> Result<Scan<T>, E> {
    let mut matches = vec![];
    let mut scanned = 0;
    loop {
        let items = fetch(scanned, chunk)?;
        scanned += items.len();
        let read = items.len();
        matches.extend(items.into_iter().filter(|item| keep(item)));
        if read < chunk {
            return Ok(Scan {
                matches,
                exhausted: true,
            });
        }
        if matches.len() >= need || scanned >= SCAN_LIMIT {
            return Ok(Scan {
                matches,
                exhausted: false,
            });
        }
    }
}

pub struct ScanResult<T> {
    pub matches: Vec<T>,
    pub truncated: bool,
}

/// Scans a whole source (up to `SCAN_LIMIT` rows) and keeps every match, so a list can page
/// through the matches without rescanning. `truncated` means rows past the limit were not read.
pub fn paged_scan<T, E>(
    fetch: impl FnMut(usize, usize) -> Result<Vec<T>, E>,
    keep: impl Fn(&T) -> bool,
) -> Result<ScanResult<T>, E> {
    let scan = scan_filtered(fetch, keep, usize::MAX, SCAN_CHUNK)?;
    Ok(ScanResult {
        matches: scan.matches,
        truncated: !scan.exhausted,
    })
}

/// Shown above a filtered list whose scan stopped at `SCAN_LIMIT`.
pub const TRUNCATED_NOTICE: &str =
    "Filter applied to the first 50,000 rows; some matches may be missing.";

/// Pager text for a page of a list.
pub fn page_label(offset: u64, shown: u64, total: u64) -> String {
    if total == 0 {
        return "0 records".into();
    }
    format!("{}\u{2013}{} of {}", offset + 1, offset + shown, total)
}

#[cfg(test)]
#[path = "conditions_tests.rs"]
mod tests;
