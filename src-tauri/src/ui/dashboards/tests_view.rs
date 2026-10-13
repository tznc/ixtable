//! The live dashboard (port of `tests/unit/dashboard-view.test.tsx` and the view cases of
//! `dashboard-conditions.test.tsx`, which mocked `runSavedQuery`; here a `QueryRunner` double
//! stands in). Imports are explicit: `gpui_kit::*` would shadow `#[test]`.

use super::test_support::{
    FakeQueries, advance_by, define, install_fake, new_dashboard, open_dashboards, open_in_runtime, runtime_page, seed_ops,
};
use crate::ui::reports::test_support::{choose, id, wait_until};
use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;
use serde_json::{Map, Value, json};

fn params(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

fn open_view(h: &mut Harness) {
    open_dashboards(h);
    h.click("tab-view");
}

#[gpui_kit::test]
fn shows_progress_and_cancel_after_two_seconds_cancels_and_refreshes(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_ops(&mut h);
    let fake = FakeQueries::new(42, true);
    install_fake(&mut h, &fake);
    open_view(&mut h);
    wait_until(&mut h, "the first run", |_| !fake.calls().is_empty());
    let first = fake.calls()[0].clone();
    assert_eq!((first.query.as_str(), first.params.clone()), ("q1", params(json!({"region": null}))));
    assert!(!first.run_id.is_empty());
    assert!(!h.exists("dash-cancel"));

    advance_by(&mut h, 2100);
    assert_eq!(h.label("dash-progress"), "Loading dashboard data… 2s (0 of 1 queries done)");
    assert!(h.exists("dash-progress-bar"));
    h.click("dash-cancel");
    wait_until(&mut h, "the cancelled query", |h| h.try_label(id("gate:Open orders")).as_deref() == Some("Query cancelled."));
    assert_eq!(*fake.cancelled.lock().unwrap(), [first.run_id.clone()]);
    assert!(!h.exists("dash-progress"));

    h.click("dash-refresh");
    wait_until(&mut h, "the refreshed value", |h| h.try_label(id("kpi-value:Open orders")).as_deref() == Some("42"));
    assert_eq!(fake.calls().len(), 2);
    assert_eq!(h.label(id("text:Notes:0")), "First.");
    assert_eq!(h.label(id("text:Notes:1")), "Second.");
}

#[gpui_kit::test]
fn passes_filter_values_as_parameters(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_ops(&mut h);
    let fake = FakeQueries::new(7, false);
    install_fake(&mut h, &fake);
    open_view(&mut h);
    wait_until(&mut h, "the KPI", |h| h.try_label(id("kpi-value:Open orders")).as_deref() == Some("7"));
    choose(&mut h, "filter:Region", "East");
    // The value settles for 250 ms before the queries run again.
    assert_eq!(fake.calls().len(), 1);
    advance_by(&mut h, 300);
    wait_until(&mut h, "the second run", |_| fake.calls().len() == 2);
    assert_eq!(fake.last().expect("a run").params, params(json!({"region": "East"})));
}

#[gpui_kit::test]
fn reports_a_missing_dashboard_on_a_runtime_page(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_ops(&mut h);
    open_in_runtime(&mut h, "nope");
    assert_eq!(h.label("dash-missing"), "This dashboard does not exist.");
}

#[gpui_kit::test]
fn a_new_dashboard_says_it_has_no_components(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    new_dashboard(&mut h);
    h.click("tab-view");
    assert_eq!(h.label("dash-empty"), "This dashboard has no components yet.");
}

fn conditions_dashboards(h: &mut Harness) {
    let at = |column: u32| json!({"column": column, "row": 1, "columnSpan": 3, "rowSpan": 1});
    let filter = json!([{"id": "f1", "name": "Region", "param": "region", "logicalType": "text", "control": "select", "options": ["East", "West"]}]);
    define(
        h,
        vec![],
        vec![
            json!({"id": "d1", "name": "Ops", "filters": filter, "components": [
                {"id": "t", "kind": "text", "title": "East notes", "placement": at(1), "text": "Only for East.", "visibleWhen": "params.region = 'East'"},
                {"id": "b", "kind": "button", "title": "Go", "label": "Run", "placement": at(4), "actionId": "a1", "enabledWhen": "isnull(params.region)"},
                {"id": "n", "kind": "text", "title": "Always", "placement": at(7), "text": "Here."},
            ]}),
            json!({"id": "d2", "name": "Broken", "filters": filter, "components": [
                {"id": "x", "kind": "text", "title": "Big region", "placement": at(1), "text": "Hidden text", "visibleWhen": "params.region > 5"},
                {"id": "y", "kind": "text", "title": "Odd", "placement": at(4), "text": "Odd text", "enabledWhen": "params.region + 1 > 0"},
            ]}),
        ],
    );
}

#[gpui_kit::test]
fn shows_a_condition_that_fails_to_evaluate_instead_of_hiding_the_component(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    conditions_dashboards(&mut h);
    open_in_runtime(&mut h, "d2");
    choose(&mut h, "filter:Region", "East");
    wait_until(&mut h, "the visible-when problem", |h| h.exists(id("widget-problem:Big region:visible")));
    assert!(h.label(id("widget-problem:Big region:visible")).starts_with("Visible when: "));
    assert!(!h.exists(id("text:Big region:0")), "the component's text is not shown");
    assert!(h.label(id("widget-problem:Odd:enabled")).starts_with("Enabled when: "));
    assert!(!h.exists(id("widget-disabled:Odd")), "a failing condition does not say the widget is unavailable");
    let view = runtime_page(&mut h);
    let states = h.cx.read(|cx| view.read(cx).widgets(cx));
    let odd = states.iter().find(|w| w.title == "Odd").expect("Odd");
    assert!(!odd.enabled && odd.shown && odd.enabled_error.is_some());
    let big = states.iter().find(|w| w.title == "Big region").expect("Big region");
    assert!(big.visible_error.is_some());
}

#[gpui_kit::test]
fn hides_and_disables_components_from_params_keeping_other_placements(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    conditions_dashboards(&mut h);
    open_in_runtime(&mut h, "d1");
    let view = runtime_page(&mut h);
    let shown = |h: &mut Harness| h.cx.read(|cx| view.read(cx).widgets(cx).into_iter().map(|w| (w.title, w.shown, w.enabled)).collect::<Vec<_>>());
    assert_eq!(
        shown(&mut h),
        [("East notes".to_string(), false, true), ("Go".to_string(), true, true), ("Always".to_string(), true, true)]
    );
    assert!(!h.exists(id("widget:East notes")));
    assert!(!h.exists(id("widget-disabled:Go")));
    let always = h.try_find(id("widget:Always")).map(|s| s.bounds());

    choose(&mut h, "filter:Region", "East");
    wait_until(&mut h, "the East notes", |h| h.exists(id("widget:East notes")));
    wait_until(&mut h, "Go disabled", |h| h.exists(id("widget-disabled:Go")));
    assert_eq!(h.label(id("widget-disabled:Go")), "Not available right now.");
    assert_eq!(h.try_find(id("widget:Always")).map(|s| s.bounds()), always, "other placements stay put");
    assert_eq!(shown(&mut h)[1], ("Go".to_string(), true, false));
}
