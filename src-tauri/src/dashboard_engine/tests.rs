//! Port of tests/unit/dashboard-model.test.ts: query results to chart data, KPI values,
//! filter parameters and the dashboard model. Imports are explicit.

use super::data::{
    Row, dashboard_params, default_filter_values, filter_params, kpi_value, result_rows, table_slice, text_paragraphs,
};
use super::model::{
    dashboard_issues, duplicate_dashboard, embeddable_modes, error_messages, new_component, new_dashboard, new_filter,
    with_layout,
};
use crate::archive::DocumentConfig;
use crate::dashboards::{
    ChartType, ComponentKind, Dashboard, DashboardComponent, DashboardFilter, FilterControl, KpiComparison,
};
use crate::data::DataValue;
use crate::design::{ConditionalStyle, FormMode, FormSource, Placement, SourceKind, Tone};
use crate::forms::schema::new_form;
use crate::report_engine::chart_geometry::{Series, SeriesSpec, chart_data, fold_series, scatter_data};
use serde_json::{Map, Value, json};

fn at(column: u16, row: u16, column_span: u16, row_span: u16) -> Placement {
    Placement { column, row, column_span, row_span, region: None }
}

fn chart(patch: impl FnOnce(&mut DashboardComponent)) -> DashboardComponent {
    let mut c = DashboardComponent {
        id: "c".into(),
        kind: ComponentKind::Chart,
        title: "Chart".into(),
        placement: at(1, 1, 6, 2),
        chart_type: Some(ChartType::Bar),
        x: Some("region".into()),
        y: vec!["amount".into()],
        ..Default::default()
    };
    patch(&mut c);
    c
}

fn rows() -> Vec<Row> {
    [
        json!({"region": "East", "year": 2025, "amount": 10, "cost": 4}),
        json!({"region": "West", "year": 2025, "amount": 5, "cost": 1}),
        json!({"region": "East", "year": 2026, "amount": 7, "cost": null}),
        json!({"region": null, "year": 2026, "amount": "3", "cost": 2}),
    ]
    .into_iter()
    .map(|v| v.as_object().cloned().expect("object"))
    .collect()
}

fn spec(c: &DashboardComponent) -> SeriesSpec<'_> {
    SeriesSpec { x: c.x.as_deref().unwrap_or(""), y: &c.y, group_by: c.group_by.as_deref() }
}

fn values(items: &[Option<f64>]) -> Vec<Option<f64>> {
    items.to_vec()
}

#[test]
fn converts_data_values_to_plain_row_objects() {
    let columns: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
    let rows = result_rows(&columns, &[vec![DataValue::Integer(2), DataValue::Text("x".into()), DataValue::Null]]);
    assert_eq!(Value::Object(rows[0].clone()), json!({"a": 2, "b": "x", "c": null}));
}

#[test]
fn sums_rows_per_category_with_one_series_per_value_field() {
    let c = chart(|c| c.y = vec!["amount".into(), "cost".into()]);
    let data = chart_data(&spec(&c), &rows());
    assert_eq!(data.categories, ["East", "West", "(blank)"]);
    assert_eq!(data.series[0], Series { name: "amount".into(), values: values(&[Some(17.0), Some(5.0), Some(3.0)]) });
    assert_eq!(data.series[1], Series { name: "cost".into(), values: values(&[Some(4.0), Some(1.0), Some(2.0)]) });
}

#[test]
fn splits_the_first_value_field_by_group() {
    let c = chart(|c| c.group_by = Some("year".into()));
    let data = chart_data(&spec(&c), &rows());
    assert_eq!(data.categories, ["East", "West", "(blank)"]);
    assert_eq!(data.series[0], Series { name: "2025".into(), values: values(&[Some(10.0), Some(5.0), None]) });
    assert_eq!(data.series[1], Series { name: "2026".into(), values: values(&[Some(7.0), None, Some(3.0)]) });
}

#[test]
fn folds_series_past_the_palette_into_other() {
    let series: Vec<Series> =
        (0..10).map(|i| Series { name: format!("s{i}"), values: vec![Some(i as f64), None] }).collect();
    let folded = fold_series(series);
    let names: Vec<&str> = folded.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["s0", "s1", "s2", "s3", "s4", "s5", "s6", "Other"]);
    assert_eq!(folded[7].values, values(&[Some(7.0 + 8.0 + 9.0), None]));
}

#[test]
fn builds_scatter_points_from_numeric_x_and_y_skipping_blanks() {
    let c = chart(|c| {
        c.chart_type = Some(ChartType::Scatter);
        c.x = Some("amount".into());
        c.y = vec!["cost".into()];
    });
    let data = scatter_data(&spec(&c), &rows());
    assert_eq!(data.len(), 1);
    assert_eq!(data[0].name, "cost");
    let points: Vec<(f64, f64, &str)> = data[0].points.iter().map(|p| (p.x, p.y, p.label.as_str())).collect();
    assert_eq!(points, [(10.0, 4.0, "10"), (5.0, 1.0, "5"), (3.0, 2.0, "3")]);
    let grouped = chart(|c| {
        c.chart_type = Some(ChartType::Scatter);
        c.x = Some("amount".into());
        c.y = vec!["cost".into()];
        c.group_by = Some("region".into());
    });
    let counts: Vec<(String, usize)> = scatter_data(&spec(&grouped), &rows()).iter().map(|s| (s.name.clone(), s.points.len())).collect();
    assert_eq!(counts, [("East".into(), 1), ("West".into(), 1), ("(blank)".into(), 1)]);
}

fn kpi(patch: impl FnOnce(&mut DashboardComponent)) -> DashboardComponent {
    let mut c = chart(|_| {});
    c.kind = ComponentKind::Kpi;
    patch(&mut c);
    c
}

fn scope(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("object scope")
}

#[test]
fn evaluates_an_expression_over_rows_and_params_with_a_format() {
    let c = kpi(|c| {
        c.expression = Some("sum(rows.amount) * params.rate".into());
        c.format = Some("#,##0.00".into());
    });
    let value = kpi_value(&c, &rows()[..3], &scope(json!({"params": {"rate": 100}}))).expect("kpi");
    assert_eq!(value.value.as_f64(), Some(2200.0));
    assert_eq!(value.text, "2,200.00");
    assert!(value.comparison.is_none());
}

#[test]
fn reads_a_field_from_the_first_row_and_compares() {
    let c = kpi(|c| {
        c.value_field = Some("amount".into());
        c.comparison = Some(KpiComparison { value_field: Some("cost".into()), expression: None, label: "cost".into() });
    });
    let value = kpi_value(&c, &rows(), &Map::new()).expect("kpi");
    assert_eq!(value.text, "10");
    let comparison = value.comparison.expect("a comparison");
    assert_eq!(comparison.value.as_f64(), Some(4.0));
    assert_eq!(comparison.delta, Some(6.0));
    assert_eq!((comparison.label.as_str(), comparison.text.as_str()), ("cost", "+6"));
    let empty = kpi(|c| c.value_field = Some("amount".into()));
    assert_eq!(kpi_value(&empty, &[], &Map::new()).expect("kpi").text, "—");
}

#[test]
fn returns_expression_errors_for_the_widget_to_show() {
    let c = kpi(|c| c.expression = Some("sum(rows.amount".into()));
    assert!(kpi_value(&c, &rows(), &Map::new()).is_err());
}

fn filter(patch: impl FnOnce(&mut DashboardFilter)) -> DashboardFilter {
    let mut f = new_filter("Region", "region");
    patch(&mut f);
    f
}

fn params(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("object")
}

#[test]
fn maps_blank_filter_values_to_null_so_the_query_default_applies() {
    assert_eq!(filter_params(&filter(|_| {}), &json!("")), params(json!({"region": null})));
    assert_eq!(filter_params(&filter(|_| {}), &json!("East")), params(json!({"region": "East"})));
    assert!(filter_params(&filter(|f| f.param = String::new()), &json!("East")).is_empty());
}

#[test]
fn converts_numbers_and_splits_date_ranges() {
    let number = filter(|f| {
        f.param = "n".into();
        f.control = FilterControl::Number;
        f.logical_type = "integer".into();
    });
    assert_eq!(filter_params(&number, &json!("4.7")), params(json!({"n": 4})));
    let text_number = filter(|f| {
        f.param = "n".into();
        f.control = FilterControl::Text;
        f.logical_type = "number".into();
    });
    assert_eq!(filter_params(&text_number, &json!("abc")), params(json!({"n": null})));
    let range = filter(|f| {
        f.param = "period".into();
        f.control = FilterControl::DateRange;
    });
    assert_eq!(
        filter_params(&range, &json!({"from": "2026-01-01", "to": ""})),
        params(json!({"periodFrom": "2026-01-01", "periodTo": null}))
    );
}

#[test]
fn starts_from_defaults_and_merges_all_filters() {
    let filters = vec![
        filter(|f| {
            f.id = "a".into();
            f.default_value = Some(json!("West"));
        }),
        filter(|f| {
            f.id = "b".into();
            f.param = "period".into();
            f.control = FilterControl::DateRange;
        }),
    ];
    let values = default_filter_values(&filters);
    assert_eq!(Value::Object(values.clone()), json!({"a": "West", "b": {}}));
    assert_eq!(
        Value::Object(dashboard_params(&filters, &values)),
        json!({"region": "West", "periodFrom": null, "periodTo": null})
    );
}

#[test]
fn places_new_components_at_the_first_free_grid_slot_with_kind_sizes() {
    let mut dashboard = new_dashboard("Sales");
    let first = new_component(ComponentKind::Kpi, &dashboard, |_| {});
    dashboard.components.push(first.clone());
    let second = new_component(ComponentKind::Kpi, &dashboard, |_| {});
    dashboard.components.push(second.clone());
    let bar = new_component(ComponentKind::Chart, &dashboard, |_| {});
    assert_eq!([first.placement, second.placement, bar.placement.clone()], [at(1, 1, 3, 1), at(4, 1, 3, 1), at(7, 1, 6, 2)]);
    assert_eq!([first.title, second.title, bar.title], ["KPI 1", "KPI 2", "Chart 1"]);
    assert_eq!(bar.chart_type, Some(ChartType::Bar));
}

#[test]
fn duplicates_with_fresh_ids_and_remaps_filter_components() {
    let mut dashboard = new_dashboard("Sales");
    let f = new_filter("Region", "region");
    dashboard.filters.push(f.clone());
    let component = new_component(ComponentKind::Filter, &dashboard, |c| c.filter_id = Some(f.id.clone()));
    dashboard.components.push(component);
    let copy = duplicate_dashboard(&dashboard, "Sales copy");
    assert_ne!(copy.id, dashboard.id);
    assert_ne!(copy.filters[0].id, f.id);
    assert_ne!(copy.components[0].id, dashboard.components[0].id);
    assert_eq!(copy.components[0].filter_id.as_ref(), Some(&copy.filters[0].id));
    assert_eq!(copy.components[0].placement, dashboard.components[0].placement);
}

#[test]
fn clamps_placements_when_the_grid_gets_narrower() {
    let mut dashboard = new_dashboard("Sales");
    let mut component = new_component(ComponentKind::Chart, &dashboard, |_| {});
    component.placement = at(7, 1, 6, 2);
    dashboard.components.push(component);
    let mut layout = dashboard.layout.clone();
    layout.columns.truncate(4);
    let narrow = with_layout(&dashboard, layout, &[]);
    assert_eq!(narrow.components[0].placement, at(1, 1, 4, 2));
}

#[test]
fn serializes_layouts_and_placements_exactly_like_a_form() {
    let layout = json!({
        "columns": [{"kind": "fixed", "value": 200}, {"kind": "fr", "value": 2, "min": 120}],
        "rows": [{"kind": "content"}],
        "columnGap": 8, "rowGap": 12, "padding": 4,
        "justifyItems": "start", "alignItems": "center",
        "namedRegions": [{"name": "hero", "column": 1, "row": 1, "columnSpan": 2, "rowSpan": 1}],
        "breakpoints": [{"minWidth": 600, "columns": [{"kind": "fr", "value": 1}]}],
        "gridTemplateColumns": "200px 2fr",
    });
    let placement = json!({"column": 2, "row": 3, "columnSpan": 1, "rowSpan": 2, "gridArea": "x"});
    let dashboard: Dashboard = serde_json::from_value(json!({
        "id": "d", "name": "Ops", "layout": layout,
        "components": [{"id": "k", "kind": "kpi", "placement": placement}],
    }))
    .expect("dashboard");
    let form: crate::design::Form = serde_json::from_value(json!({
        "id": "f", "name": "F", "layout": layout,
        "controls": [{"id": "k", "kind": "text", "label": "K", "placement": placement}],
    }))
    .expect("form");
    let text = |v: &dyn erased::Json| v.json();
    assert_eq!(text(&dashboard.layout), text(&form.layout));
    assert_eq!(text(&dashboard.components[0].placement), text(&form.controls[0].placement));
    assert!(!text(&dashboard.layout).contains("gridTemplate"));
    let round: Dashboard = serde_json::from_value(serde_json::to_value(&dashboard).expect("json")).expect("round trip");
    assert_eq!(round, dashboard);
    let old: Dashboard = serde_json::from_value(json!({"id": "x", "name": "Old"})).expect("old");
    assert_eq!(
        old,
        Dashboard { id: "x".into(), name: "Old".into(), layout: Default::default(), filters: vec![], components: vec![] }
    );
}

mod erased {
    /// A value as its JSON text.
    pub trait Json {
        fn json(&self) -> String;
    }
    impl<T: serde::Serialize> Json for T {
        fn json(&self) -> String {
            serde_json::to_string(self).expect("json")
        }
    }
}

#[test]
fn reports_grid_and_reference_problems() {
    let mut dashboard = new_dashboard("Sales");
    let mut kpi = new_component(ComponentKind::Kpi, &dashboard, |c| c.query_id = Some("missing".into()));
    kpi.id = "k".into();
    kpi.placement = at(11, 1, 3, 1);
    let mut filter = new_component(ComponentKind::Filter, &dashboard, |c| c.filter_id = Some("nope".into()));
    filter.id = "f".into();
    dashboard.components.push(kpi);
    dashboard.components.push(filter);
    let mut config = DocumentConfig::default();
    config.design.forms.clear();
    assert_eq!(
        error_messages(&dashboard_issues(&dashboard, &config)),
        [
            "grid placement exceeds declared columns",
            "\"KPI 1\" uses a saved query that does not exist",
            "\"Filter 1\" shows a filter that does not exist",
        ]
    );
}

#[test]
fn flags_a_table_style_rule_with_no_column() {
    let mut dashboard = new_dashboard("Sales");
    let mut table = new_component(ComponentKind::Table, &dashboard, |_| {});
    table.id = "t".into();
    table.title = "Orders".into();
    let good = ConditionalStyle { id: "a".into(), when: "value < 0".into(), tone: Tone::Negative, column: Some("amount".into()) };
    let bad = ConditionalStyle { id: "b".into(), when: "value > 9".into(), tone: Tone::Positive, column: None };
    table.styles = vec![good.clone(), bad];
    dashboard.components.push(table);
    let config = DocumentConfig::default();
    let message = "\"Orders\" has a conditional style with no column";
    let messages = |d: &Dashboard| dashboard_issues(d, &config).into_iter().map(|i| i.message).collect::<Vec<_>>();
    assert!(messages(&dashboard).contains(&message.to_string()));
    dashboard.components[0].styles = vec![good];
    assert!(!messages(&dashboard).contains(&message.to_string()));
}

#[test]
fn embeddable_modes_drop_create_and_edit_for_read_only_query_forms() {
    let table = new_form("T", Some(FormSource { kind: SourceKind::Table, table: Some("t".into()), ..Default::default() }));
    assert_eq!(
        embeddable_modes(Some(&table)),
        [FormMode::List, FormMode::Detail, FormMode::Create, FormMode::Edit]
    );
    let mut query = new_form("Q", Some(FormSource { kind: SourceKind::Query, query_id: Some("q".into()), ..Default::default() }));
    query.modes = vec![FormMode::List, FormMode::Edit];
    assert_eq!(embeddable_modes(Some(&query)), [FormMode::List]);
    assert_eq!(embeddable_modes(None).len(), 6);
}

#[test]
fn splits_text_into_paragraphs_at_blank_lines() {
    assert_eq!(text_paragraphs("First.\n\nSecond."), ["First.", "Second."]);
    assert_eq!(text_paragraphs("  \n\nOnly\nthis\n \n"), ["Only\nthis"]);
    assert!(text_paragraphs("").is_empty());
}

// ---- Port of the TableBody cases of tests/unit/dashboard-conditions.test.tsx ----------

fn sales() -> (Vec<String>, Vec<Vec<DataValue>>) {
    (
        vec!["region".into(), "total".into()],
        vec![
            vec![DataValue::Text("East".into()), DataValue::Real(350.0)],
            vec![DataValue::Text("North".into()), DataValue::Real(50.0)],
            vec![DataValue::Text("West".into()), DataValue::Real(400.0)],
        ],
    )
}

fn table() -> DashboardComponent {
    let style = |id: &str, when: &str, tone: Tone, column: &str| ConditionalStyle {
        id: id.into(),
        when: when.into(),
        tone,
        column: Some(column.into()),
    };
    DashboardComponent {
        id: "t".into(),
        kind: ComponentKind::Table,
        title: "Sales".into(),
        placement: at(1, 1, 3, 1),
        page_size: Some(1),
        filter: Some("record.total >= params.min".into()),
        styles: vec![
            style("s1", "value > 380", Tone::Positive, "total"),
            style("s2", "true", Tone::Muted, "total"),
            style("s3", "record.region = 'West'", Tone::Emphasis, "region"),
        ],
        ..Default::default()
    }
}

#[test]
fn filters_rows_before_paging_and_styles_cells_by_column() {
    let (columns, rows) = sales();
    let scope = scope(json!({"params": {"min": 100}}));
    let first = table_slice(&table(), &columns, &rows, &scope, 0).expect("slice");
    assert_eq!((first.pages, first.page, first.kept), (2, 0, 2));
    assert_eq!(first.rows, [["East", "350"]]);
    assert_eq!(first.tones[0], [None, Some(Tone::Muted)]);
    let second = table_slice(&table(), &columns, &rows, &scope, 1).expect("slice");
    assert_eq!(second.rows, [["West", "400"]]);
    assert_eq!(second.tones[0], [Some(Tone::Emphasis), Some(Tone::Positive)]);
    // North (50) is filtered out; a page past the end clamps to the last.
    assert_eq!(table_slice(&table(), &columns, &rows, &scope, 9).expect("slice").page, 1);
}

#[test]
fn shows_a_filter_syntax_error_instead_of_rows() {
    let (columns, rows) = sales();
    let mut broken = table();
    broken.filter = Some("record.total >".into());
    let error = table_slice(&broken, &columns, &rows, &Map::new(), 0).expect_err("a syntax error");
    assert!(error.starts_with("Filter:"), "{error}");
}
