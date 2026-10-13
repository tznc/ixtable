//! Turns saved-query results into KPI values, table slices and filter parameters (port of
//! `src/dashboards/data.ts`). Chart series come from `report_engine::chart_geometry`, which
//! reports share.

use crate::dashboards::{DashboardComponent, DashboardFilter, FilterControl};
use crate::data::DataValue;
use crate::design::Tone;
use crate::expr::{self, EvaluateOptions};
use crate::forms::conditions::{row_filter, tone_for};
use crate::report_engine::chart_geometry::js_string;
use crate::report_engine::values::num_value;
pub use crate::report_engine::{
    Row,
    chart_geometry::{category_label, format_number, to_number},
    data::result_rows,
};
use crate::ui::query::results::show_value;
use serde_json::{Map, Value};

/// A KPI comparison: the other value, `value - other` (None when either is not a number),
/// the delta as shown, and the label.
#[derive(Debug, Clone, PartialEq)]
pub struct KpiComparisonValue {
    pub value: Value,
    pub delta: Option<f64>,
    pub text: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KpiValue {
    pub value: Value,
    pub text: String,
    pub comparison: Option<KpiComparisonValue>,
}

fn present(text: Option<&str>) -> Option<&str> {
    text.filter(|t| !t.trim().is_empty())
}

/// Evaluates a field or expression against the rows (`rows`, `params`, `app` scope).
fn measure(field: Option<&str>, expression: Option<&str>, rows: &[Row], scope: &Map<String, Value>) -> Result<Value, String> {
    if let Some(expression) = present(expression) {
        let mut scope = scope.clone();
        scope.insert("rows".into(), Value::Array(rows.iter().cloned().map(Value::Object).collect()));
        return expr::evaluate(expression, &scope, EvaluateOptions::default()).map_err(|e| e.message);
    }
    match field.filter(|f| !f.is_empty()) {
        Some(field) => Ok(rows.first().and_then(|r| r.get(field)).cloned().unwrap_or(Value::Null)),
        None => Ok(Value::Null),
    }
}

/// `Number(x.toPrecision(15))`: float noise from subtraction rounded away.
fn precision15(x: f64) -> f64 {
    format!("{x:.14e}").parse().unwrap_or(x)
}

/// KPI value and optional comparison (value minus comparison). Expression errors are returned.
pub fn kpi_value(component: &DashboardComponent, rows: &[Row], scope: &Map<String, Value>) -> Result<KpiValue, String> {
    let value = measure(component.value_field.as_deref(), component.expression.as_deref(), rows, scope)?;
    let format = component.format.as_deref();
    let mut result = KpiValue { text: format_number(&value, format), value, comparison: None };
    if let Some(comparison) = &component.comparison {
        if comparison.value_field.as_deref().is_some_and(|f| !f.is_empty()) || present(comparison.expression.as_deref()).is_some() {
            let other = measure(comparison.value_field.as_deref(), comparison.expression.as_deref(), rows, scope)?;
            let (a, b) = (to_number(Some(&result.value)), to_number(Some(&other)));
            let delta = a.zip(b).map(|(a, b)| precision15(a - b));
            let text = match delta {
                None => "—".to_string(),
                Some(d) => format!("{}{}", if d > 0.0 { "+" } else { "" }, format_number(&num_value(d), format)),
            };
            result.comparison = Some(KpiComparisonValue {
                value: other,
                delta,
                text,
                label: if comparison.label.is_empty() { "comparison".into() } else { comparison.label.clone() },
            });
        }
    }
    Ok(result)
}

fn blank(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        _ => false,
    }
}

/// Converts one filter's raw control value to query parameters (blank becomes null, which
/// leaves the query default). A filter without a parameter name contributes nothing.
pub fn filter_params(filter: &DashboardFilter, value: &Value) -> Map<String, Value> {
    let mut out = Map::new();
    if filter.param.is_empty() {
        return out;
    }
    if filter.control == FilterControl::DateRange {
        let side = |key: &str| match value.get(key) {
            Some(v) if !blank(v) => v.clone(),
            _ => Value::Null,
        };
        out.insert(format!("{}From", filter.param), side("from"));
        out.insert(format!("{}To", filter.param), side("to"));
        return out;
    }
    if blank(value) {
        out.insert(filter.param.clone(), Value::Null);
        return out;
    }
    let numeric = ["integer", "number", "decimal", "real"].contains(&filter.logical_type.as_str());
    if numeric || filter.control == FilterControl::Number {
        let number = to_number(Some(value)).map(|n| if filter.logical_type == "integer" { n.trunc() } else { n });
        out.insert(filter.param.clone(), number.map_or(Value::Null, num_value));
    } else {
        out.insert(filter.param.clone(), value.clone());
    }
    out
}

/// Initial control values by filter id: each filter's default.
pub fn default_filter_values(filters: &[DashboardFilter]) -> Map<String, Value> {
    filters
        .iter()
        .map(|f| {
            let value = match &f.default_value {
                Some(v) if !v.is_null() => v.clone(),
                _ if f.control == FilterControl::DateRange => Value::Object(Map::new()),
                _ => Value::String(String::new()),
            };
            (f.id.clone(), value)
        })
        .collect()
}

/// The parameters every component query receives, keyed by parameter name.
pub fn dashboard_params(filters: &[DashboardFilter], values: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    for filter in filters {
        let value = values.get(&filter.id).filter(|v| !v.is_null()).or(filter.default_value.as_ref()).cloned().unwrap_or(Value::Null);
        out.extend(filter_params(filter, &value));
    }
    out
}

/// A text component's paragraphs: blank lines separate them.
pub fn text_paragraphs(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut current: Vec<&str> = vec![];
    for line in text.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                out.push(current.join("\n"));
                current.clear();
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        out.push(current.join("\n"));
    }
    out
}

/// Choices of a select filter from its options query: the first column's distinct values.
pub fn option_values(rows: &[Vec<DataValue>]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for row in rows {
        let Some(cell) = row.first().filter(|c| !matches!(c, DataValue::Null)) else {
            continue;
        };
        let text = js_string(&crate::forms::values::from_data_value(cell));
        if !out.contains(&text) {
            out.push(text);
        }
    }
    out
}

/// One page of a table component: shown cells and the tone of each.
#[derive(Debug, Clone, PartialEq)]
pub struct TableSlice {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub tones: Vec<Vec<Option<Tone>>>,
    /// Rows that pass the filter.
    pub kept: usize,
    pub pages: usize,
    /// The page shown (0-based), clamped to the last.
    pub page: usize,
}

/// A table component's page over a query result. The `filter` expression keeps result rows
/// (`record` is one row); paging counts only kept rows. Cells take the tone of the first
/// matching `styles` rule for their column. A filter that does not parse is returned as
/// `Filter: <message>`.
pub fn table_slice(
    component: &DashboardComponent,
    columns: &[String],
    rows: &[Vec<DataValue>],
    scope: &Map<String, Value>,
    page: usize,
) -> Result<TableSlice, String> {
    let size = component.page_size.unwrap_or(10).max(1) as usize;
    let shown: Vec<&String> = component.columns.iter().filter(|c| columns.contains(c)).collect();
    let indexes: Vec<usize> = if shown.is_empty() {
        (0..columns.len()).collect()
    } else {
        shown.iter().filter_map(|c| columns.iter().position(|x| x == *c)).collect()
    };
    let records = result_rows(columns, rows);
    let mut filter_scope = Map::new();
    filter_scope.insert("form".into(), Value::Object(Map::new()));
    filter_scope.insert("parent".into(), Value::Null);
    filter_scope.extend(scope.clone());
    let keep = row_filter(component.filter.as_deref(), &filter_scope).map_err(|e| format!("Filter: {}", e.message))?;
    let kept: Vec<usize> = records.iter().enumerate().filter(|(_, r)| keep.as_ref().is_none_or(|k| k(r))).map(|(i, _)| i).collect();
    let pages = kept.len().div_ceil(size).max(1);
    let current = page.min(pages - 1);
    let visible = &kept[(current * size).min(kept.len())..((current + 1) * size).min(kept.len())];
    let names: Vec<String> = indexes.iter().map(|&i| columns[i].clone()).collect();
    let cells = visible.iter().map(|&r| indexes.iter().map(|&i| show_value(&rows[r][i])).collect()).collect();
    let tones = visible
        .iter()
        .map(|&r| {
            names
                .iter()
                .map(|name| {
                    if component.styles.is_empty() {
                        return None;
                    }
                    let mut scope = scope.clone();
                    scope.insert("record".into(), Value::Object(records[r].clone()));
                    scope.insert("value".into(), records[r].get(name).cloned().unwrap_or(Value::Null));
                    tone_for(&component.styles, &scope, Some(name))
                })
                .collect()
        })
        .collect();
    Ok(TableSlice { columns: names, rows: cells, tones, kept: kept.len(), pages, page: current })
}
