//! Ports of `report-model.test.ts`, `report-parameters.test.ts` and `report-dataset.test.ts`.

use super::data::{cell_value, load_dataset, result_rows, DatasetSource};
use super::fixtures::norm;
use super::model::{
    band_entries, clamp_geometry, duplicate_report, field_expression, find_component, is_page_band, new_component,
    new_group, new_report, report_problems, with_band, BandKey, Geometry,
};
use super::parameters::{parse_parameter, report_parameters, Raw};
use crate::archive::{DocumentConfig, QueryParameter, SavedQuery};
use crate::data::logical::LogicalType;
use crate::data::{Column, DataValue, NamedValue, Page};
use crate::manager::AppError;
use crate::queries::QueryRun;
use serde_json::{json, Map, Value};
use std::cell::RefCell;

#[test]
fn creates_a_report_with_a4_defaults_and_lists_bands_in_print_order() {
    let mut report = new_report("Sales");
    assert_eq!(report.page.dimensions(), (595.0, 842.0));
    assert_eq!(report.page.content_width(), 523.0);
    let outer = new_group("record.region");
    let inner = new_group("record.city");
    report.bands.groups = vec![outer, inner.clone()];
    let labels: Vec<String> = band_entries(&report).into_iter().map(|b| b.label).collect();
    assert_eq!(
        labels,
        [
            "Report header",
            "Page header",
            "Region group header",
            "City group header",
            "Detail",
            "City group footer",
            "Region group footer",
            "Page footer",
            "Report footer",
        ]
    );
    let mut changed = report.clone();
    with_band(&mut changed, &BandKey::GroupFooter(inner.id.clone()), |b| b.height = 99.0);
    assert_eq!(changed.bands.groups[1].footer.height, 99.0);
    assert_eq!(report.bands.groups[1].footer.height, 20.0);
    with_band(&mut changed, &BandKey::Detail, |b| b.keep_together = true);
    assert_eq!(changed.bands.detail.height, 18.0);
    assert!(changed.bands.detail.keep_together);
    assert!(changed.bands.detail.components.is_empty());
    assert_eq!(BandKey::GroupFooter("x".into()).id(), "groupFooter:x");
}

#[test]
fn duplicates_with_fresh_ids_for_the_report_groups_components_and_table_columns() {
    let mut report = new_report("Sales");
    report.bands.groups = vec![new_group("record.region")];
    let mut table = new_component("table", 523.0, (0.0, 0.0));
    table
        .extra
        .insert("columns".into(), json!([{ "id": "col", "header": "A", "expression": "record.a", "width": 1 }]));
    report.bands.detail.components = vec![table.clone()];
    let copy = duplicate_report(&report, "Sales copy");
    assert_eq!(copy.name, "Sales copy");
    assert_ne!(copy.id, report.id);
    assert_ne!(copy.bands.groups[0].id, report.bands.groups[0].id);
    let copied = &copy.bands.detail.components[0];
    assert_ne!(copied.id, table.id);
    assert_ne!(copied.extra["columns"][0]["id"], json!("col"));
    assert_eq!(find_component(&copy, &copied.id).unwrap().0, BandKey::Detail);
}

#[test]
fn builds_field_expressions_and_keeps_new_and_moved_components_in_the_band() {
    assert_eq!(field_expression("amount"), "record.amount");
    assert_eq!(field_expression("Unit Price"), "record.[Unit Price]");
    let c = new_component("staticText", 523.0, (500.0, 3.0));
    assert_eq!((c.x, c.y, c.w, c.h), (403.0, 3.0, 120.0, 16.0));
    let g = clamp_geometry(Geometry { x: -4.4, y: 50.0, w: 600.0, h: 0.0 }, 523.0, 30.0);
    assert_eq!(g, Geometry { x: 0.0, y: 29.0, w: 523.0, h: 1.0 });
}

#[test]
fn converts_query_cells_to_plain_values() {
    let cells = [
        DataValue::Integer(42),
        DataValue::Real(1.5),
        DataValue::Boolean(true),
        DataValue::Date("2026-01-05".into()),
        DataValue::Null,
    ];
    let got: Vec<Value> = cells.iter().map(cell_value).collect();
    assert_eq!(got, [json!(42), json!(1.5), json!(true), json!("2026-01-05"), Value::Null]);
    let rows = result_rows(&["a".into(), "b".into()], &[vec![DataValue::Integer(1), DataValue::Null]]);
    assert_eq!(norm(Value::Array(rows.into_iter().map(Value::Object).collect())), json!([{ "a": 1, "b": null }]));
}

#[test]
fn flags_tables_in_page_headers_and_footers_only() {
    let mut report = new_report("R");
    let table = new_component("table", 523.0, (0.0, 0.0));
    report.bands.page_footer.components.push(table.clone());
    report.bands.detail.components.push(new_component("table", 523.0, (0.0, 0.0)));
    assert_eq!(
        report_problems(&report, &[]),
        [format!("Page footer component {}: tables are not supported in page headers or footers", table.id)]
    );
    assert!(is_page_band(&BandKey::PageHeader));
    assert!(!is_page_band(&BandKey::Detail));
}

// Parameters.

fn query(id: &str, parameters: Value) -> SavedQuery {
    serde_json::from_value(json!({ "id": id, "name": id, "sql": "SELECT 1", "parameters": parameters })).unwrap()
}

fn param(name: &str, logical_type: &str, required: bool) -> QueryParameter {
    serde_json::from_value(json!({ "name": name, "logicalType": logical_type, "required": required })).unwrap()
}

#[test]
fn collects_dataset_then_table_query_parameters_with_report_defaults_first() {
    let mut report = new_report("R");
    report.dataset_query_id = Some("ds".into());
    report.params.insert("region".into(), json!("West"));
    report.bands.detail.components = vec![serde_json::from_value(json!({
        "id": "t", "kind": "table", "queryId": "lines", "columns": [], "x": 0, "y": 0, "w": 100, "h": 20
    }))
    .unwrap()];
    let queries = vec![
        query(
            "ds",
            json!([
                { "name": "region", "logicalType": "text", "defaultValue": "East" },
                { "name": "since", "logicalType": "date", "defaultValue": "2026-01-01" }
            ]),
        ),
        query(
            "lines",
            json!([
                { "name": "region", "logicalType": "integer" },
                { "name": "limit", "logicalType": "integer", "required": true }
            ]),
        ),
        query("unused", json!([{ "name": "other", "logicalType": "text" }])),
    ];
    let got: Vec<(String, String, bool, Option<Value>)> = report_parameters(&report, &queries)
        .into_iter()
        .map(|p| (p.name, p.logical_type, p.required, p.default_value))
        .collect();
    assert_eq!(
        got,
        vec![
            ("region".to_string(), "text".to_string(), false, Some(json!("West"))),
            ("since".to_string(), "date".to_string(), false, Some(json!("2026-01-01"))),
            ("limit".to_string(), "integer".to_string(), true, None),
        ]
    );
    assert!(report_parameters(&new_report("Plain"), &[]).is_empty());
}

#[test]
fn parses_prompt_values_by_logical_type() {
    let text = |s: &str| Raw::Text(s.into());
    assert_eq!(parse_parameter(&param("x", "integer", false), &text(" 42 ")), Ok(json!(42)));
    assert_eq!(
        parse_parameter(&param("x", "integer", false), &text("4.5")),
        Err("x must be a whole number.".to_string())
    );
    assert_eq!(parse_parameter(&param("x", "number", false), &text("4.5")), Ok(json!(4.5)));
    assert_eq!(
        parse_parameter(&param("x", "number", false), &text("abc")),
        Err("x must be a number.".to_string())
    );
    assert_eq!(parse_parameter(&param("x", "text", false), &text("")), Ok(Value::Null));
    assert_eq!(
        parse_parameter(&param("x", "text", true), &text(" ")),
        Err("Enter a value for x.".to_string())
    );
    assert_eq!(parse_parameter(&param("x", "date", false), &text("2026-03-01")), Ok(json!("2026-03-01")));
    assert_eq!(parse_parameter(&param("x", "boolean", false), &Raw::Flag(false)), Ok(json!(false)));
}

// Dataset loading.

struct Fake {
    restricted: bool,
    calls: RefCell<Vec<String>>,
}

fn column(name: &str) -> Column {
    Column {
        name: name.into(),
        declared_type: "INTEGER".into(),
        logical_type: LogicalType::Integer,
        nullable: true,
        default_value: None,
        primary_key_position: 0,
        generated: false,
        unique: false,
        auto_increment: false,
    }
}

impl DatasetSource for Fake {
    fn run_saved(&self, _: &SavedQuery, _: Vec<NamedValue>, _: u64, _: &str) -> Result<QueryRun, AppError> {
        unreachable!("tables only")
    }
    fn run_sql(&self, _: &str, _: u64, _: &str) -> Result<QueryRun, AppError> {
        self.calls.borrow_mut().push("execute_parameterized_query".into());
        if self.restricted {
            return Err(AppError::new("FORBIDDEN", "The role \"Clerk\" cannot run ad hoc SQL"));
        }
        Ok(QueryRun {
            columns: vec!["n".into()],
            rows: vec![vec![DataValue::Integer(1)]],
            truncated: false,
            row_limit: 5,
            elapsed_ms: 1,
        })
    }
    fn read_page(&self, _: &str, offset: u64, limit: u64) -> Result<Page, AppError> {
        self.calls.borrow_mut().push(format!("read_table_page {offset} {limit}"));
        let count = limit.min(2500u64.saturating_sub(offset));
        Ok(Page {
            columns: vec![column("n")],
            rows: (0..count).map(|i| vec![DataValue::Integer((offset + i) as i64)]).collect(),
            identities: vec![],
            total: 2500,
            offset,
            limit,
        })
    }
}

fn numbers_report() -> crate::reports::Report {
    let mut r = new_report("Numbers");
    r.table = Some("numbers".into());
    r
}

#[test]
fn reads_a_table_report_with_ad_hoc_sql_for_the_developer() {
    let source = Fake { restricted: false, calls: RefCell::new(vec![]) };
    let run = load_dataset(&source, &numbers_report(), &DocumentConfig::default(), &Map::new(), "run", 5)
        .unwrap()
        .unwrap();
    assert_eq!(run.rows.len(), 1);
    assert_eq!(*source.calls.borrow(), ["execute_parameterized_query"]);
}

#[test]
fn pages_through_the_table_when_a_runtime_role_may_not_run_ad_hoc_sql() {
    let source = Fake { restricted: true, calls: RefCell::new(vec![]) };
    let run = load_dataset(&source, &numbers_report(), &DocumentConfig::default(), &Map::new(), "run", 2200)
        .unwrap()
        .unwrap();
    assert_eq!(run.columns, ["n"]);
    assert_eq!(run.rows.len(), 2200);
    assert!(run.truncated);
    let calls = source.calls.borrow();
    let pages: Vec<&String> = calls.iter().filter(|c| c.starts_with("read_table_page")).collect();
    assert_eq!(pages, ["read_table_page 0 1000", "read_table_page 1000 1000", "read_table_page 2000 200"]);
}

#[test]
fn group_labels_name_the_field_not_the_expression() {
    let mut report = new_report("Sales");
    report.bands.groups = vec![new_group("record.order_status"), new_group("record.[Ship City]"), new_group("upper(record.x)"), new_group("")];
    let labels: Vec<String> = band_entries(&report).into_iter().map(|b| b.label).collect();
    assert_eq!(labels[2], "Order status group header");
    assert_eq!(labels[3], "Ship City group header");
    assert_eq!(labels[4], "Group 3 header");
    assert_eq!(labels[5], "Group 4 header");
    assert!(labels.iter().all(|l| !l.contains("record.") && !l.contains('(')));
}
