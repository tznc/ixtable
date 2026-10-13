//! Unit tests for dashboard definitions and validation.
use super::*;
use crate::archive::{QueryParameter, SavedQuery, Severity};
use serde_json::json;

fn config_with(dashboard: Page) -> DocumentConfig {
    let mut config = DocumentConfig::default();
    config.saved_queries.push(SavedQuery {
        id: "q1".into(),
        name: "Sales".into(),
        sql: "SELECT * FROM sales WHERE ($region IS NULL OR region = $region)".into(),
        parameters: vec![
            QueryParameter {
                name: "region".into(),
                logical_type: "text".into(),
                ..Default::default()
            },
            QueryParameter {
                name: "since".into(),
                logical_type: "date".into(),
                required: true,
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    config.pages.push(dashboard);
    config
}

fn messages(issues: &[Issue], severity: Severity) -> Vec<String> {
    issues
        .iter()
        .filter(|i| i.severity == severity)
        .map(|i| format!("{}:{}: {}", i.object_kind, i.object_id, i.message))
        .collect()
}

fn at(column: u16, row: u16, column_span: u16, row_span: u16) -> Placement {
    Placement {
        column,
        row,
        column_span,
        row_span,
        region: None,
    }
}

fn sample() -> Page {
    serde_json::from_value(json!({
        "id": "d1",
        "name": "Sales",
        "filters": [{"id": "f1", "name": "Region", "param": "region", "control": "select", "options": ["East", "West"]}],
        "components": [
            {"id": "k1", "kind": "kpi", "title": "Total", "queryId": "q1", "expression": "sum(rows.amount)",
             "placement": {"column": 1, "row": 1, "columnSpan": 4, "rowSpan": 1}},
            {"id": "c1", "kind": "chart", "title": "By region", "queryId": "q1", "chartType": "bar",
             "x": "region", "y": ["amount"], "placement": {"column": 5, "row": 1, "columnSpan": 8, "rowSpan": 2}},
            {"id": "fl", "kind": "filter", "filterId": "f1", "placement": {"column": 1, "row": 2, "columnSpan": 4}}
        ]
    }))
    .unwrap()
}

#[test]
fn defaults_and_round_trip_use_the_forms_grid_types() {
    let dashboard = sample();
    assert_eq!(dashboard.layout, GridLayout::default());
    assert_eq!(dashboard.components[2].placement, at(1, 2, 4, 1));
    assert_eq!(dashboard.components[1].chart_type, Some(ChartType::Bar));
    let value = serde_json::to_value(&dashboard).unwrap();
    // Same serialized grid as a form layout; unused component fields are omitted.
    assert_eq!(
        value["layout"],
        serde_json::to_value(GridLayout::default()).unwrap()
    );
    assert_eq!(
        value["components"][0]["placement"],
        json!({"column": 1, "row": 1, "columnSpan": 4, "rowSpan": 1, "region": null})
    );
    assert!(value["components"][0].get("formId").is_none());
    assert!(value["components"][0].get("stacked").is_none());
    let back: Page = serde_json::from_value(value).unwrap();
    assert_eq!(back, dashboard);
    let yaml = serde_yaml::to_string(&dashboard).unwrap();
    assert_eq!(serde_yaml::from_str::<Page>(&yaml).unwrap(), dashboard);
    // A v2-era stub (id + name only) still loads.
    let stub: Page = serde_json::from_value(json!({"id": "d", "name": "Old"})).unwrap();
    assert!(stub.components.is_empty() && stub.filters.is_empty());
}

#[test]
fn filter_default_serializes_as_default() {
    let filter: PageFilter = serde_json::from_value(
        json!({"id": "f", "name": "Period", "param": "period", "control": "dateRange", "default": "2026-01-01"}),
    )
    .unwrap();
    assert_eq!(filter.default_value, Some(json!("2026-01-01")));
    assert_eq!(filter.logical_type, "text");
    assert_eq!(filter_params(&filter), vec!["periodFrom", "periodTo"]);
    assert_eq!(
        serde_json::to_value(&filter).unwrap()["default"],
        json!("2026-01-01")
    );
}

#[test]
fn valid_dashboard_reports_only_the_unsupplied_required_parameter() {
    let issues = validate(&config_with(sample()));
    assert!(messages(&issues, Severity::Error).is_empty(), "{issues:?}");
    assert_eq!(
        messages(&issues, Severity::Warning),
        vec![
            "dashboardComponent:k1: query \"Sales\" requires $since but no dashboard filter sets it",
            "dashboardComponent:c1: query \"Sales\" requires $since but no dashboard filter sets it",
        ]
    );
}

#[test]
fn missing_dependencies_are_errors() {
    let mut dashboard = sample();
    dashboard.components.extend(
        serde_json::from_value::<Vec<PageComponent>>(json!([
            {"id": "t1", "kind": "table", "queryId": "nope", "pageSize": 0, "placement": {"row": 3}},
            {"id": "fm", "kind": "form", "formId": "missing", "placement": {"row": 4}},
            {"id": "rp", "kind": "report", "reportId": "missing", "placement": {"row": 5}},
            {"id": "bt", "kind": "button", "title": "Go", "actionId": "missing", "placement": {"row": 6}},
            {"id": "f2", "kind": "filter", "filterId": "nope", "placement": {"row": 7}}
        ]))
        .unwrap(),
    );
    dashboard.filters.push(PageFilter {
        id: "f9".into(),
        name: "Other".into(),
        param: "region".into(),
        options_query_id: Some("gone".into()),
        ..Default::default()
    });
    let errors = messages(&validate(&config_with(dashboard)), Severity::Error);
    assert_eq!(
        errors,
        vec![
            "dashboardFilter:f9: more than one filter sets parameter region",
            "dashboardFilter:f9: filter \"Other\" lists choices from a saved query that does not exist",
            "dashboardComponent:t1: \"t1\" uses a saved query that does not exist",
            "dashboardComponent:t1: table \"t1\" needs a page size of at least 1",
            "dashboardComponent:fm: \"fm\" embeds a form that does not exist",
            "dashboardComponent:rp: \"rp\" embeds a report that does not exist",
            "dashboardComponent:bt: button \"Go\" runs an action that does not exist",
            "dashboardComponent:f2: \"f2\" shows a filter that does not exist",
        ]
    );
}

#[test]
fn placements_follow_the_shared_grid_rules() {
    let mut dashboard = sample();
    dashboard.components[0].placement = at(10, 1, 4, 1);
    dashboard.components[1].placement = Placement {
        region: Some("hero".into()),
        ..at(1, 1, 1, 1)
    };
    dashboard.components[2].placement = at(0, 1, 1, 1);
    let errors = messages(&validate(&config_with(dashboard.clone())), Severity::Error);
    assert_eq!(
        errors,
        vec![
            "dashboardComponent:k1: grid placement exceeds declared columns",
            "dashboardComponent:c1: placement references an unknown region",
            "dashboardComponent:fl: grid placement uses 1-based positions and positive spans",
        ]
    );
    dashboard.layout.columns.clear();
    let errors = messages(&validate(&config_with(dashboard)), Severity::Error);
    assert_eq!(
        errors,
        vec!["dashboard:d1: Sales: grid layout requires at least one column track"]
    );
}

#[test]
fn filters_must_map_to_declared_query_parameters() {
    let mut dashboard = sample();
    dashboard.filters.push(PageFilter {
        id: "f2".into(),
        name: "Since".into(),
        param: "since".into(),
        control: FilterControl::DateRange,
        ..Default::default()
    });
    dashboard.filters.push(PageFilter {
        id: "f3".into(),
        name: "Blank".into(),
        param: " ".into(),
        ..Default::default()
    });
    let issues = validate(&config_with(dashboard));
    assert_eq!(
        messages(&issues, Severity::Warning),
        vec![
            "dashboardFilter:f2: filter \"Since\" sets $sinceFrom, $sinceTo, which no query on this dashboard declares",
            "dashboardComponent:k1: query \"Sales\" requires $since but no dashboard filter sets it",
            "dashboardComponent:c1: query \"Sales\" requires $since but no dashboard filter sets it",
        ]
    );
    assert_eq!(
        messages(&issues, Severity::Error),
        vec!["dashboardFilter:f3: filter \"Blank\" has no parameter name"]
    );
}

#[test]
fn incomplete_components_are_warnings() {
    let dashboard: Page = serde_json::from_value(json!({
        "id": "d1", "name": "Draft",
        "components": [
            {"id": "k", "kind": "kpi", "title": "K"},
            {"id": "c", "kind": "chart", "title": "C", "queryId": "q1", "chartType": "summary"},
            {"id": "b", "kind": "button", "title": "B"},
            {"id": "k", "kind": "text", "text": "dup"}
        ]
    }))
    .unwrap();
    let issues = validate(&config_with(dashboard));
    assert_eq!(
        messages(&issues, Severity::Warning),
        vec![
            "dashboardComponent:c: query \"Sales\" requires $since but no dashboard filter sets it",
            "dashboardComponent:k: \"K\" has no saved query",
            "dashboardComponent:k: KPI \"K\" needs a value field or expression",
            "dashboardComponent:c: chart \"C\" has no value fields",
            "dashboardComponent:b: button \"B\" has no action",
        ]
    );
    assert_eq!(
        messages(&issues, Severity::Error),
        vec!["dashboardComponent:k: component ids must be non-empty and unique within a dashboard"]
    );
}

#[test]
fn embedded_form_modes_and_button_form_state_are_checked() {
    let mut dashboard = sample();
    dashboard.components = serde_json::from_value(json!([
        {"id": "e1", "kind": "form", "title": "Edit", "formId": "qf", "mode": "edit", "recordId": "params.id",
         "placement": {"column": 1, "row": 1, "columnSpan": 4}},
        {"id": "e2", "kind": "form", "title": "Show", "formId": "qf", "mode": "detail",
         "placement": {"column": 5, "row": 1, "columnSpan": 4}},
        {"id": "b1", "kind": "button", "title": "Set", "actionId": "a1",
         "placement": {"column": 9, "row": 1, "columnSpan": 4}}
    ]))
    .unwrap();
    assert_eq!(
        dashboard.components[0].record_id.as_deref(),
        Some("params.id")
    );
    let mut config = config_with(dashboard);
    config.design.forms.push(crate::design::Form {
        id: "qf".into(),
        name: "Sales".into(),
        source: Some(crate::design::FormSource {
            kind: crate::design::SourceKind::Query,
            query_id: Some("q1".into()),
            ..Default::default()
        }),
        modes: vec![FormMode::List, FormMode::Detail],
        ..Default::default()
    });
    config.actions = serde_json::from_value(json!([{
        "id": "a1", "name": "A", "steps": [{"id": "s", "kind": "setState", "scope": "form", "key": "k", "value": "1"}]
    }]))
    .unwrap();
    let issues = validate(&config);
    let warnings = messages(&issues, Severity::Warning).join("\n");
    assert!(
        warnings.contains("\"Edit\" opens its form in a mode"),
        "{warnings}"
    );
    assert!(!warnings.contains("\"Show\" opens"), "{warnings}");
    assert!(warnings.contains("sets form state"), "{warnings}");
    let errors = messages(&issues, Severity::Error).join("\n");
    assert!(!errors.contains("sets form state"), "{errors}");
}

#[test]
fn component_conditions_filters_and_styles_round_trip() {
    let raw = json!({
        "id": "t1", "kind": "table", "title": "Orders", "queryId": "q1",
        "filter": "record.amount > 10", "visibleWhen": "params.region <> null",
        "enabledWhen": "app.role = 'admin'",
        "styles": [{"id": "s1", "when": "value < 0", "tone": "warning", "column": "amount"}],
        "placement": {"column": 1, "row": 3, "columnSpan": 6, "rowSpan": 1}
    });
    let component: PageComponent = serde_json::from_value(raw.clone()).unwrap();
    let back = serde_json::to_value(&component).unwrap();
    for key in ["filter", "visibleWhen", "enabledWhen", "styles"] {
        assert_eq!(back[key], raw[key], "{key}");
    }
    let mut dashboard = sample();
    dashboard.components.push(PageComponent {
        filter: Some(" ".into()),
        visible_when: Some(String::new()),
        styles: vec![ConditionalStyle {
            id: "s".into(),
            ..Default::default()
        }],
        ..component
    });
    let found = messages(&validate(&config_with(dashboard)), Severity::Error);
    for needle in [
        "empty filter expression",
        "empty visibility expression",
        "conditional style with an empty condition",
        "conditional style with no column",
    ] {
        assert!(
            found.iter().any(|e| e.contains(needle)),
            "{needle}: {found:?}"
        );
    }
}

#[test]
fn table_style_with_a_column_is_valid() {
    let mut dashboard = sample();
    let raw = json!({
        "id": "t9", "kind": "table", "title": "Styled", "queryId": "q1",
        "styles": [{"id": "s1", "when": "value < 0", "tone": "warning", "column": "amount"}],
        "placement": {"column": 1, "row": 9, "columnSpan": 6, "rowSpan": 1}
    });
    dashboard
        .components
        .push(serde_json::from_value(raw).unwrap());
    let found = messages(&validate(&config_with(dashboard)), Severity::Error);
    assert!(!found.iter().any(|e| e.contains("no column")), "{found:?}");
}
