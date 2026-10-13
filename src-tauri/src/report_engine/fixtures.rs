//! Shared report fixtures for engine, PDF and UI tests (ports `tests/unit/report-fixtures.ts`).

use super::{LayoutOptions, ReportDocument, Row};
use super::{Item, TextItem};
use crate::reports::Report;
use serde_json::{json, Map, Value};

/// A band definition.
pub fn band(height: f64, components: Vec<Value>, keep_together: bool) -> Value {
    json!({ "height": height, "keepTogether": keep_together, "components": components })
}

/// The base report as JSON; `patch` replaces bands by key (`detail`, `reportHeader`, ...).
pub fn base_json(patch: Vec<(&str, Value)>) -> Value {
    let mut report = json!({
        "id": "r1",
        "name": "Sales",
        "params": {},
        "page": {
            "size": "A4",
            "orientation": "portrait",
            "margins": { "top": 36, "right": 36, "bottom": 36, "left": 36 }
        },
        "bands": {
            "reportHeader": band(0.0, vec![], false),
            "pageHeader": band(0.0, vec![], false),
            "groups": [],
            "detail": band(0.0, vec![], false),
            "pageFooter": band(0.0, vec![], false),
            "reportFooter": band(0.0, vec![], false)
        }
    });
    for (key, value) in patch {
        report["bands"][key] = value;
    }
    report
}

pub fn report(value: &Value) -> Report {
    serde_json::from_value(value.clone()).expect("valid report")
}

/// A row from a JSON object.
pub fn row(value: Value) -> Row {
    match value {
        Value::Object(map) => map,
        other => panic!("row must be an object, got {other}"),
    }
}

pub fn rows(values: Value) -> Vec<Row> {
    values.as_array().expect("array").iter().cloned().map(row).collect()
}

pub fn orders() -> Vec<Row> {
    rows(json!([
        { "id": 1, "region": "West", "customer": "Acme", "amount": 120.5, "qty": 2, "price": 10 },
        { "id": 2, "region": "East", "customer": "Birch", "amount": 80, "qty": 1, "price": 80 },
        { "id": 3, "region": "West", "customer": "Cobalt", "amount": 42.25, "qty": 3, "price": 5 },
        { "id": 4, "region": "East", "customer": "Dune", "amount": 10, "qty": 4, "price": 2.5 },
        { "id": 5, "region": "North", "customer": "Elm", "amount": 300, "qty": 1, "price": 300 }
    ]))
}

/// Layout with default options.
pub fn lay(report: &Value, rows: &[Row]) -> ReportDocument {
    super::layout_report(&self::report(report), rows, &LayoutOptions::default())
}

pub fn lay_with(report: &Value, rows: &[Row], options: &LayoutOptions) -> ReportDocument {
    super::layout_report(&self::report(report), rows, options)
}

pub fn params(value: Value) -> Option<Map<String, Value>> {
    value.as_object().cloned()
}

/// Whole-number floats become integers so `json!` literals compare equal.
pub fn norm(value: Value) -> Value {
    match value {
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => json!(f as i64),
            _ => Value::Number(n),
        },
        Value::Array(a) => Value::Array(a.into_iter().map(norm).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, norm(v))).collect()),
        other => other,
    }
}

/// A document as comparable JSON.
pub fn doc_json(doc: &ReportDocument) -> Value {
    norm(serde_json::to_value(doc).expect("serializable"))
}

/// `(componentId, text, y)` of every text item, per page.
pub fn texts(doc: &ReportDocument) -> Vec<Vec<(String, String, f64)>> {
    doc.pages
        .iter()
        .map(|p| {
            p.items
                .iter()
                .filter_map(|i| match i {
                    super::Item::Text(t) => Some((t.component_id.clone(), t.text.clone(), t.y)),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

/// `(id, text, y)` triple for comparisons.
pub fn t(id: &str, text: &str, y: f64) -> (String, String, f64) {
    (id.to_string(), text.to_string(), y)
}

/// A static text component.
pub fn static_text(id: &str, text: &str, x: f64, y: f64, w: f64, h: f64) -> Value {
    json!({ "id": id, "kind": "staticText", "text": text, "x": x, "y": y, "w": w, "h": h })
}

/// A field (expression) component.
pub fn field(id: &str, expression: &str, x: f64, y: f64, w: f64, h: f64) -> Value {
    json!({ "id": id, "kind": "field", "expression": expression, "x": x, "y": y, "w": w, "h": h })
}

/// A calculated component.
pub fn calc(id: &str, expression: &str, x: f64, y: f64, w: f64, h: f64) -> Value {
    json!({ "id": id, "kind": "calculated", "expression": expression, "x": x, "y": y, "w": w, "h": h })
}

/// Items of one component with their page index.
pub fn of(doc: &ReportDocument, id: &str) -> Vec<(usize, Item)> {
    doc.pages
        .iter()
        .enumerate()
        .flat_map(|(page, p)| {
            p.items
                .iter()
                .filter(|i| i.component_id() == id)
                .map(move |i| (page, i.clone()))
        })
        .collect()
}

/// Text items of one component with their page index.
pub fn text_of(doc: &ReportDocument, id: &str) -> Vec<(usize, TextItem)> {
    of(doc, id)
        .into_iter()
        .filter_map(|(page, i)| match i {
            Item::Text(t) => Some((page, t)),
            _ => None,
        })
        .collect()
}
