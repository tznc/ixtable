//! Port of tests/unit/expression-issues.test.ts.

use super::{ExpressionIssue, expression_issues, referenced_tables};
use crate::archive::DocumentConfig;
use crate::automation::{ActionDef, Step, Trigger, TriggerMode};
use crate::dashboards::{ComponentKind, Dashboard, DashboardComponent, KpiComparison};
use crate::design::{
    ConditionalStyle, ControlKind, DesignSchema, Form, FormRule, FormSource, RelatedList, Relationship, SourceKind,
    Tone,
};
use crate::forms::schema::{new_control, new_form};
use crate::report_engine::model::{new_component, new_report};
use crate::reports::{Band, ReportGroup};
use crate::ui::shell::{AutomationTab, RevealTarget};
use serde_json::{Value, json};

fn base() -> DocumentConfig {
    DocumentConfig {
        design: DesignSchema { version: 3, forms: vec![], navigation: vec![], start_page: None },
        ..serde_json::from_value(json!({"version": 1, "name": "Test", "activeMode": "design"})).unwrap()
    }
}

fn with_forms(forms: Vec<Form>) -> DocumentConfig {
    let mut config = base();
    config.design.forms = forms;
    config
}

fn table_form() -> Form {
    new_form("Orders", Some(FormSource { kind: SourceKind::Table, table: Some("orders".into()), ..Default::default() }))
}

fn nothing(_: Option<&str>) -> Option<Vec<String>> {
    None
}

fn fields(issues: &[ExpressionIssue]) -> Vec<&str> {
    issues.iter().map(|i| i.field.as_str()).collect()
}

fn steps(value: Value) -> Vec<Step> {
    serde_json::from_value(value).unwrap()
}

#[test]
fn finds_nothing_in_a_config_without_broken_expressions() {
    let mut form = table_form();
    let mut control = new_control(ControlKind::Text, &form, None);
    control.visible_when = Some("record.total > 0".into());
    control.label = "Total".into();
    let mut blank = new_control(ControlKind::Computed, &form, None);
    blank.computed = Some("  ".into());
    form.controls = vec![control, blank];
    assert!(expression_issues(&with_forms(vec![form]), &nothing).is_empty());
}

#[test]
fn flags_every_form_expression_field_and_points_at_the_control() {
    let form = table_form();
    let mut control = new_control(ControlKind::Text, &form, None);
    control.label = "Total".into();
    control.visible_when = Some("record.total >".into());
    control.enabled_when = Some("nope(1)".into());
    control.computed = Some("1 +".into());
    control.default_value = Some("missing.x".into());
    control.validation.expression = Some("len()".into());
    control.styles = vec![ConditionalStyle { id: "s".into(), when: "(".into(), tone: Tone::Positive, column: None }];
    control.relationship = Some(Relationship {
        table: "customers".into(),
        value_column: "id".into(),
        display_column: "name".into(),
        filter: Some("record.bad".into()),
        ..Default::default()
    });
    control.related = Some(RelatedList {
        table: "lines".into(),
        foreign_key: "order_id".into(),
        parent_column: "id".into(),
        filter: Some(")".into()),
        ..Default::default()
    });
    let control_id = control.id.clone();
    let broken = Form {
        filter: Some("record.status =".into()),
        rules: vec![FormRule { id: "r".into(), expression: "and".into(), message: "x".into() }],
        controls: vec![control],
        ..form.clone()
    };
    let columns = |table: Option<&str>| match table {
        Some("orders") => Some(vec!["total".to_string(), "status".to_string()]),
        Some("customers") => Some(vec!["id".to_string(), "name".to_string()]),
        _ => None,
    };
    let issues = expression_issues(&with_forms(vec![broken]), &columns);
    assert_eq!(
        fields(&issues),
        vec![
            "Filter",
            "Rule 1",
            "Control \"Total\" › Visible when",
            "Control \"Total\" › Enabled when",
            "Control \"Total\" › Computed value",
            "Control \"Total\" › Default value",
            "Control \"Total\" › Validation",
            "Control \"Total\" › Lookup filter",
            "Control \"Total\" › Related list filter",
            "Control \"Total\" › Style 1",
        ]
    );
    assert!(issues.iter().all(|i| i.object_kind == "form"));
    assert_eq!(issues[0].target, RevealTarget::Design { object_id: form.id.clone(), element_id: None });
    assert_eq!(issues[2].object_id, form.id);
    assert_eq!(issues[2].object_name, "Orders");
    assert_eq!(
        issues[2].target,
        RevealTarget::Design { object_id: form.id.clone(), element_id: Some(control_id) }
    );
    let message = |suffix: &str| issues.iter().find(|i| i.field.ends_with(suffix)).unwrap().message.clone();
    assert_eq!(message("Enabled when"), "Unknown function 'nope'");
    assert_eq!(message("Lookup filter"), "Unknown field 'record.bad'");
}

#[test]
fn uses_the_forms_columns_for_record_fields_like_the_designer() {
    let form = table_form();
    let mut control = new_control(ControlKind::Text, &form, None);
    control.computed = Some("record.renamed * 2".into());
    let config = with_forms(vec![Form { controls: vec![control], ..form }]);
    assert!(expression_issues(&config, &nothing).is_empty());
    let issues = expression_issues(&config, &|_| Some(vec!["total".to_string()]));
    assert_eq!(issues[0].message, "Unknown field 'record.renamed'");
}

#[test]
fn checks_query_source_parameters_with_app_and_params_only() {
    let mut source = FormSource { kind: SourceKind::Query, query_id: Some("q".into()), ..Default::default() };
    source.params.insert("since".into(), "record.x".into());
    source.params.insert("n".into(), "params.n".into());
    let form = new_form("Recent", Some(source));
    let issues = expression_issues(&with_forms(vec![form]), &nothing);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].field, "Query parameter since");
    assert_eq!(issues[0].message, "Unknown name 'record'");
}

#[test]
fn walks_report_bands_groups_and_table_columns() {
    let mut report = new_report("Sales");
    let mut field = new_component("field", 500.0, (0.0, 0.0));
    field.extra.insert("expression".into(), json!("record.amount +"));
    let mut table = new_component("table", 500.0, (0.0, 0.0));
    table.extra.insert("columns".into(), json!([{"id": "c", "header": "Qty", "expression": "sum(", "width": 40}]));
    let table_id = table.id.clone();
    let band = |components| Band { height: 10.0, components, ..Default::default() };
    report.bands.detail.components = vec![field];
    report.bands.groups = vec![ReportGroup {
        id: "g".into(),
        group_by: "unknown.region".into(),
        header: band(vec![]),
        footer: band(vec![table]),
        ..Default::default()
    }];
    let mut config = base();
    config.reports = vec![report.clone()];
    let issues = expression_issues(&config, &nothing);
    assert_eq!(
        fields(&issues),
        vec!["Group 1 › Group by", "Group 1 footer › Table column \"Qty\"", "Detail › Field"]
    );
    assert_eq!(
        issues[1].target,
        RevealTarget::Reports { object_id: report.id.clone(), element_id: Some(table_id) }
    );
    assert_eq!(issues[0].target, RevealTarget::Reports { object_id: report.id, element_id: None });
}

#[test]
fn walks_dashboard_components_with_their_scopes() {
    let kpi = DashboardComponent {
        id: "k".into(),
        kind: ComponentKind::Kpi,
        title: "Revenue".into(),
        expression: Some("sum(rows.amount)".into()),
        comparison: Some(KpiComparison { label: "vs".into(), expression: Some("record.x".into()), ..Default::default() }),
        visible_when: Some("params.region = 'EU'".into()),
        enabled_when: Some("rows".into()),
        ..Default::default()
    };
    let table = DashboardComponent {
        id: "t".into(),
        kind: ComponentKind::Table,
        title: "Orders".into(),
        filter: Some("record.total > params.min".into()),
        styles: vec![ConditionalStyle {
            id: "s".into(),
            when: "value >".into(),
            tone: Tone::Negative,
            column: Some("total".into()),
        }],
        ..Default::default()
    };
    let mut config = base();
    config.dashboards = vec![Dashboard { id: "d".into(), name: "Ops".into(), components: vec![kpi, table], ..Default::default() }];
    let issues = expression_issues(&config, &nothing);
    let pairs: Vec<(&str, &str)> = issues.iter().map(|i| (i.field.as_str(), i.message.as_str())).collect();
    assert_eq!(pairs[0], ("Component \"Revenue\" › Comparison", "Unknown name 'record'"));
    assert_eq!(pairs[1], ("Component \"Revenue\" › Enabled when", "Unknown name 'rows'"));
    assert_eq!(pairs[2].0, "Component \"Orders\" › Style 1");
    assert_eq!(pairs.len(), 3);
    assert_eq!(
        issues[2].target,
        RevealTarget::Dashboards { object_id: "d".into(), element_id: Some("t".into()) }
    );
}

#[test]
fn walks_action_steps_nested_branches_and_triggers() {
    let action = ActionDef {
        id: "a1".into(),
        name: "Close order".into(),
        steps: steps(json!([
            {"id": "s1", "kind": "updateRecord", "table": "orders", "match": "current", "values": {"status": "'closed"}},
            {"id": "s2", "kind": "condition", "when": "record.total > 100",
             "then": [{"id": "s3", "kind": "message", "text": "bogus(1)"}],
             "else": [{"id": "s4", "kind": "setState", "scope": "app", "key": "k", "value": "1", "when": "x.y"}]},
            {"id": "s5", "kind": "runQuery", "queryId": "q", "params": {"id": "record.id"}, "storeAs": "r"},
        ])),
        ..Default::default()
    };
    let trigger: Trigger = serde_json::from_value(json!({
        "id": "t1", "name": "On create", "table": "orders", "event": "created",
        "condition": "record.total >", "actionId": "a1", "mode": "async", "enabled": true,
        "idempotencyKey": "trigger.id & record.id", "maxAttempts": 3, "backoffMs": 100,
    }))
    .unwrap();
    let mut config = base();
    config.actions = vec![action];
    config.triggers = vec![trigger];
    let issues = expression_issues(&config, &nothing);
    assert_eq!(
        fields(&issues),
        vec![
            "Step 1 › Value status",
            "Step 2 › Then › Step 1 › Text",
            "Step 2 › Else › Step 1 › Only when",
            "Condition",
        ]
    );
    assert_eq!(
        issues[0].target,
        RevealTarget::Automation { tab: AutomationTab::Actions, object_id: "a1".into() }
    );
    assert_eq!(
        issues[3].target,
        RevealTarget::Automation { tab: AutomationTab::Triggers, object_id: "t1".into() }
    );
}

#[test]
fn ignores_the_idempotency_key_of_a_sync_trigger_as_the_editor_hides_it() {
    let mut trigger: Trigger = serde_json::from_value(json!({
        "id": "t", "name": "Sync", "table": "t", "event": "created", "actionId": "a",
        "mode": "sync", "enabled": true, "idempotencyKey": "(", "maxAttempts": 1, "backoffMs": 0,
    }))
    .unwrap();
    assert_eq!(trigger.mode, TriggerMode::Sync);
    let mut config = base();
    config.triggers = vec![trigger.clone()];
    assert!(expression_issues(&config, &nothing).is_empty());
    trigger.mode = TriggerMode::Async;
    config.triggers = vec![trigger];
    assert_eq!(fields(&expression_issues(&config, &nothing)), vec!["Idempotency key"]);
}

#[test]
fn lists_the_tables_whose_columns_form_expressions_read() {
    let form = table_form();
    let mut lookup = new_control(ControlKind::Relationship, &form, None);
    lookup.relationship = Some(Relationship {
        table: "customers".into(),
        value_column: "id".into(),
        display_column: "name".into(),
        ..Default::default()
    });
    let mut related = new_control(ControlKind::RelatedList, &form, None);
    related.related = Some(RelatedList {
        table: "lines".into(),
        foreign_key: "o".into(),
        parent_column: "id".into(),
        ..Default::default()
    });
    let config = with_forms(vec![Form { controls: vec![lookup, related], ..form }]);
    assert_eq!(referenced_tables(&config), vec!["orders", "customers", "lines"]);
}

#[test]
fn leaves_unused_step_maps_alone() {
    // A match of "current" has no expressions to check.
    let action = ActionDef {
        id: "a".into(),
        name: "A".into(),
        steps: steps(json!([{"id": "s", "kind": "deleteRecord", "table": "orders", "match": "current"}])),
        ..Default::default()
    };
    let mut config = base();
    config.actions = vec![action];
    assert!(expression_issues(&config, &nothing).is_empty());
}
