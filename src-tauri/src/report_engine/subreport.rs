//! Subreports (PRD Phase 7): a band prints another report once per band instance.

use super::component::{Band, Comp, Kind, Rep, SubreportLink};
use super::render::Diagnostics;
use super::Row;
use serde_json::Value;
use std::collections::HashMap;
use std::rc::Rc;

const EPS: f64 = 1e-6;

/// Subreports nest up to three levels below the report that is run (PRD Phase 7).
pub const MAX_SUBREPORT_DEPTH: usize = crate::reports::MAX_SUBREPORT_DEPTH;

pub const SUBREPORT_PAGE_BAND: &str = "Subreports are not supported in page headers or footers";
pub const SUBREPORT_EXTRA: &str = "A band prints only its first subreport";
pub const SUBREPORT_MISSING: &str = "Subreport data is not loaded";
pub const SUBREPORT_LOOP: &str = "The subreport prints a report that contains it";
pub const SUBREPORT_DEPTH: &str = "Subreports nest at most 3 levels deep";

/// A subreport definition with all its rows, ready for layout.
pub type Loaded = HashMap<String, (Rc<Rep>, Vec<Row>)>;

/// Splits a band around its subreport. The head holds every other component that starts
/// above the subreport's bottom edge and ends at the subreport's top, so components
/// beside it print with the head. The tail holds the components below it, moved up to
/// start at 0. Other subreports are dropped.
pub fn split_at_subreport(band: &Band, sub: &Comp) -> (Band, Band) {
    let bottom = sub.y + sub.h;
    let rest: Vec<&Comp> = band.components.iter().filter(|c| !c.is_subreport()).collect();
    let head = Band {
        height: sub.y,
        keep_together: band.keep_together,
        page_break_before: band.page_break_before,
        page_break_after: false,
        components: rest.iter().filter(|c| c.y < bottom - EPS).map(|c| (*c).clone()).collect(),
    };
    let tail = Band {
        height: (band.height - bottom).max(0.0),
        keep_together: band.keep_together,
        page_break_before: false,
        page_break_after: band.page_break_after,
        components: rest
            .iter()
            .filter(|c| c.y >= bottom - EPS)
            .map(|c| Comp {
                y: c.y - bottom,
                ..(*c).clone()
            })
            .collect(),
    };
    (head, tail)
}

// Link values match across number and text (1 = '1'); null never matches.
fn same_key(a: Option<&Value>, b: Option<&Value>) -> bool {
    fn text(v: &Value) -> Option<String> {
        match v {
            Value::Null => None,
            Value::String(s) => Some(s.clone()),
            other => Some(js_text(other)),
        }
    }
    match (a.and_then(text), b.and_then(text)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// `String(value)` for numbers and booleans.
fn js_text(v: &Value) -> String {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e21 => format!("{}", f as i64),
            Some(f) => format!("{f}"),
            None => n.to_string(),
        },
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

/// The subreport's rows linked to `parent`: all of them when it has no links.
pub fn linked_rows(links: &[SubreportLink], rows: &[Row], parent: &Value) -> Vec<Row> {
    let links: Vec<&SubreportLink> = links
        .iter()
        .filter(|l| !l.child.trim().is_empty() && !l.master.trim().is_empty())
        .collect();
    if links.is_empty() {
        return rows.to_vec();
    }
    let empty = Row::new();
    let record = parent.as_object().unwrap_or(&empty);
    rows.iter()
        .filter(|row| {
            links
                .iter()
                .all(|l| same_key(row.get(&l.child), record.get(&l.master)))
        })
        .cloned()
        .collect()
}

/// The report and linked rows a subreport prints, or `None` (with a diagnostic) when it
/// can't.
pub fn resolve_subreport(
    sub: &Comp,
    loaded: &Loaded,
    parent: &Value,
    stack: &[String],
    diagnostics: &Diagnostics,
) -> Option<(Rc<Rep>, Vec<Row>)> {
    let Kind::Subreport { report_id, links } = &sub.kind else {
        return None;
    };
    let data = loaded.get(report_id);
    let problem = if report_id.is_empty() {
        None
    } else if stack.contains(report_id) {
        Some(SUBREPORT_LOOP)
    } else if stack.len() > MAX_SUBREPORT_DEPTH {
        Some(SUBREPORT_DEPTH)
    } else if data.is_none() {
        Some(SUBREPORT_MISSING)
    } else {
        None
    };
    if let Some(problem) = problem {
        diagnostics.add(&sub.id, problem);
    }
    let data = data.filter(|_| problem.is_none())?;
    let rows = linked_rows(links, &data.1, parent);
    if rows.is_empty() {
        None
    } else {
        Some((data.0.clone(), rows))
    }
}
