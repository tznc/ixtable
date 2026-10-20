//! Dashboards mode flows (port of `tests/integration/dashboard.test.tsx`, plus the list and
//! designer cases the other dashboard tests rely on). Imports are explicit: `gpui_kit::*`
//! would shadow `#[test]`.

use super::test_support::{
    SALES_BY_REGION, component, dashboard, edit_store, new_dashboard, pick_query, seed_sales, wait_for_columns,
};
use crate::pages::{ComponentKind, FilterControl};
use crate::ui::reports::test_support::{choose, id, wait_until};
use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;

/// A new document in a window tall enough to show the whole properties panel (the harness
/// only clicks what is visible).
fn tall_document(cx: &mut TestAppContext) -> Harness<'_> {
    let mut h = Harness::start_sized(cx, (1500.0, 2400.0));
    h.click("new-document");
    assert!(h.has_shell(), "New document did not open the shell");
    h
}

fn chart_rows(h: &mut Harness, name: &str) -> Vec<String> {
    (0..16).map_while(|i| h.try_label(id(&format!("chart-row:{name}:{i}")))).collect()
}

#[gpui_kit::test]
fn builds_a_kpi_bar_chart_and_filter_on_the_shared_grid_views_live_data_and_persists_a_keyboard_resize(
    cx: &mut TestAppContext,
) {
    let mut h = tall_document(cx);
    seed_sales(&mut h);
    new_dashboard(&mut h);

    h.click("add-kpi");
    pick_query(&mut h, SALES_BY_REGION);
    h.fill("prop-expression", "sum(rows.total)");
    h.fill("prop-format", "#,##0");

    h.click("add-chart");
    pick_query(&mut h, SALES_BY_REGION);
    wait_for_columns(&mut h, "the query columns", "prop-y:total");
    choose(&mut h, "prop-x", "region");
    h.click(id("prop-y:total"));

    h.click("dashboard-settings");
    h.click("add-filter");
    let filter = dashboard(&h).filters[0].clone();
    assert_eq!(filter.param, "region");
    assert_eq!(filter.name, "Region");
    h.fill(id(&format!("filter-choices:{}", filter.id)), "East, North, West");

    let saved = dashboard(&h);
    assert_eq!(saved.filters.len(), 1);
    assert_eq!(saved.filters[0].control, FilterControl::Select);
    assert_eq!(saved.filters[0].options, ["East", "North", "West"]);
    let kinds: Vec<(ComponentKind, String)> = saved.components.iter().map(|c| (c.kind, c.title.clone())).collect();
    assert_eq!(kinds, [(ComponentKind::Kpi, "KPI 1".to_string()), (ComponentKind::Chart, "Chart 1".to_string())]);

    h.click("tab-view");
    wait_until(&mut h, "the KPI", |h| h.try_label(id("kpi-value:KPI 1")).as_deref() == Some("800"));
    wait_until(&mut h, "the chart data", |h| chart_rows(h, "Chart 1").len() == 4);
    assert_eq!(chart_rows(&mut h, "Chart 1"), ["regiontotal", "East350", "North50", "West400"]);
    assert_eq!(h.label(id("chart-data:Chart 1")), "Chart 1 data");
    assert!(h.label(id("chart:Chart 1")).starts_with("Chart 1. Bar chart: 1 series over 3 region. Values from 50 to 400."));

    choose(&mut h, "filter:Region", "East");
    wait_until(&mut h, "the East total", |h| h.try_label(id("kpi-value:KPI 1")).as_deref() == Some("350"));
    wait_until(&mut h, "the East chart", |h| chart_rows(h, "Chart 1") == ["regiontotal", "East350"]);

    h.click("tab-design");
    let kpi = component(&h, "KPI 1");
    h.click(id(&kpi.id));
    h.press("alt-right");
    let placement = component(&h, "KPI 1").placement;
    assert_eq!((placement.column, placement.row, placement.column_span, placement.row_span), (1, 1, 4, 1));
}

#[gpui_kit::test]
fn creates_renames_duplicates_and_deletes_dashboards(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    new_dashboard(&mut h);
    h.fill("page-name", "Sales board");
    assert_eq!(h.label("mode-title"), "Sales board");
    h.click("add-text");
    h.click("duplicate-page");
    let names: Vec<String> = h.config().pages.iter().map(|d| d.name.clone()).collect();
    assert_eq!(names, ["Main form", "Sales board", "Sales board copy"]);
    let (original, copy) = (h.config().pages[1].clone(), h.config().pages[2].clone());
    assert_ne!(original.id, copy.id);
    assert_eq!(copy.components.len(), 1);
    assert_ne!(original.components[0].id, copy.components[0].id, "a copy gets fresh component ids");
    h.click("delete-page");
    assert_eq!(h.dialogs.count("confirm"), 1, "deleting a page asks first");
    let names: Vec<String> = h.config().pages.iter().map(|d| d.name.clone()).collect();
    assert_eq!(names, ["Main form", "Sales board"]);
}

#[gpui_kit::test]
fn adds_every_kind_of_component_with_its_properties_and_lists_the_issues(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    seed_sales(&mut h);
    new_dashboard(&mut h);
    for add in ["add-kpi", "add-chart", "add-table", "add-text", "add-button", "add-form", "add-report"] {
        h.click(add);
    }
    let kinds: Vec<ComponentKind> = dashboard(&h).components.iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        [
            ComponentKind::Kpi,
            ComponentKind::Chart,
            ComponentKind::Table,
            ComponentKind::Text,
            ComponentKind::Button,
            ComponentKind::Form,
            ComponentKind::Report
        ]
    );
    // Unset references are fine; references to deleted objects are reported.
    assert!(!h.exists("dash-issues"));
    edit_store(&mut h, |c| c.pages[1].components[0].query_id = Some("gone".into()));
    assert!(h.exists("dash-issues"));
    assert_eq!(h.label(id("issue:0")), "\"KPI 1\" uses a saved query that does not exist");
    edit_store(&mut h, |c| c.pages[1].components[0].query_id = None);
    assert!(!h.exists("dash-issues"));

    // The last added component is selected; its title edits as one step.
    h.fill("prop-title", "Region report");
    assert_eq!(component(&h, "Region report").kind, ComponentKind::Report);

    // Choosing a query clears its issue and lists the columns.
    let kpi = component(&h, "KPI 1");
    h.click(id(&kpi.id));
    pick_query(&mut h, SALES_BY_REGION);
    wait_for_columns(&mut h, "the KPI fields", "prop-value-field");
    assert_eq!(component(&h, "KPI 1").query_id.as_deref(), Some(SALES_BY_REGION));
    h.click("prop-delete");
    assert!(dashboard(&h).components.iter().all(|c| c.title != "KPI 1"));
}

#[test]
fn palette_icons_are_bundled() {
    use gpui_kit::AssetSource;
    for kind in crate::page_engine::model::COMPONENT_KINDS {
        let path = super::editor::component_icon(kind).path();
        assert!(crate::ui::assets::AppAssets.load(&path).expect("load").is_some(), "{path} is not bundled");
    }
}

#[test]
fn record_navigation_icons_are_bundled() {
    use gpui_kit::{AssetSource, assets::IconName};
    for icon in [IconName::ChevronFirst, IconName::ChevronLast, IconName::ChevronLeft, IconName::ChevronRight] {
        let path = icon.path();
        assert!(crate::ui::assets::AppAssets.load(&path).expect("load").is_some(), "{path} is not bundled");
    }
}
