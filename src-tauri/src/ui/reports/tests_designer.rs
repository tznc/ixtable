//! Designer flows: running sums, conditional formatting and charts (port of
//! `tests/integration/report-phase6.test.tsx`), a subreport (the "prints each order's lines
//! through a subreport" test of `nested-subforms.test.tsx`) and the table editor.

use super::tests::tall_document;
use super::test_support::{
    add_component, add_query, choose, edit_store, id, last_component, new_report, open_reports, page_texts,
    preview, report, validation_issues_for_reports, wait_for_page, wait_until,
};
use crate::data::DataValue;
use crate::report_engine::{fixtures::{base_json, band}, Item, Mark, ReportDocument};
use crate::reports::Report;
use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;
use serde_json::json;

fn document(h: &mut Harness) -> ReportDocument {
    let preview = preview(h);
    h.cx.read(|cx| preview.read(cx).document().cloned()).expect("a laid-out document")
}

#[gpui_kit::test]
fn designs_a_running_sum_with_conditional_formatting_and_a_chart_and_previews_them(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    h.create_table("orders", &[("id", "INTEGER", 1), ("region", "TEXT", 0), ("amount", "REAL", 0)]);
    let regions = ["East", "North", "West"];
    for i in 0..6usize {
        h.insert_row(
            "orders",
            &[
                ("id", DataValue::Integer(i as i64 + 1)),
                ("region", DataValue::Text(regions[i % 3].into())),
                ("amount", DataValue::Real((i as f64 + 1.0) * 10.0)),
            ],
        );
    }
    add_query(&mut h, "q-orders", "Orders", "SELECT * FROM orders ORDER BY id");
    new_report(&mut h);
    choose(&mut h, "report-dataset", "Query: Orders");

    add_component(&mut h, "detail", "field");
    wait_until(&mut h, "the dataset columns", |h| h.exists("prop-bound-field"));
    choose(&mut h, "prop-bound-field", "amount");
    choose(&mut h, "prop-running-sum", "Over all");
    h.click("add-rule");
    h.fill("rule-1-when", "value > 100");
    let field = last_component(&h, |r| &r.bands.detail);
    assert_eq!(field.extra["runningSum"], "all");
    assert_eq!(field.extra["conditions"][0]["when"], "value > 100");
    assert_eq!(field.extra["conditions"][0]["style"]["bold"], true);

    add_component(&mut h, "reportFooter", "chart");
    h.fill("chart-title", "Amount by region");
    h.fill("chart-x", "region");
    h.fill("chart-y", "amount");
    h.fill("chart-format", "0.0");
    let chart = last_component(&h, |r| &r.bands.report_footer);
    assert_eq!(chart.extra["title"], "Amount by region");
    assert_eq!(chart.extra["xField"], "region");
    assert_eq!(chart.extra["yFields"], json!(["amount"]));
    assert_eq!(chart.extra["format"], "0.0");

    h.click("tab-preview");
    wait_for_page(&mut h, "Page 1 of 1");
    let doc = document(&mut h);
    let page = &doc.pages[0];
    let bold = |text: &str| {
        page.items.iter().find_map(|item| match item {
            Item::Text(t) if t.lines.iter().any(|l| l.text == text) => Some(t.bold),
            _ => None,
        })
    };
    for total in ["10", "30", "60", "100"] {
        assert_eq!(bold(total), Some(false), "{total} should be normal");
    }
    for total in ["150", "210"] {
        assert_eq!(bold(total), Some(true), "{total} should be bold");
    }
    let (index, item) = page
        .items
        .iter()
        .enumerate()
        .find_map(|(i, item)| matches!(item, Item::Chart(_)).then_some((i, item)))
        .expect("a chart");
    let Item::Chart(chart) = item else { unreachable!() };
    assert_eq!(h.label(id(&format!("page-chart-{index}"))), "Amount by region");
    let labels: Vec<&str> = chart
        .marks
        .iter()
        .filter_map(|m| match m {
            Mark::Text(t) => Some(t.lines.iter().map(|l| l.text.as_str())),
            _ => None,
        })
        .flatten()
        .collect();
    for label in ["East", "North", "West"] {
        assert!(labels.contains(&label), "{label} missing from {labels:?}");
    }
}

fn component_json(id: &str, expression: &str, y: f64) -> serde_json::Value {
    json!({"id": id, "kind": "field", "expression": expression, "x": 0, "y": y, "w": 200, "h": 14})
}

fn named_report(id: &str, name: &str, query: &str, detail: serde_json::Value) -> Report {
    let mut value = base_json(vec![("detail", detail)]);
    value["id"] = json!(id);
    value["name"] = json!(name);
    value["datasetQueryId"] = json!(query);
    serde_json::from_value(value).expect("report")
}

#[gpui_kit::test]
fn prints_each_orders_lines_through_a_subreport(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.create_table("orders", &[("id", "INTEGER", 1), ("customer_id", "INTEGER", 0), ("ref", "TEXT", 0)]);
    h.create_table("lines", &[("id", "INTEGER", 1), ("order_id", "INTEGER", 0), ("item", "TEXT", 0)]);
    for (order, reference) in [(1, "SO-1"), (2, "SO-2")] {
        h.insert_row(
            "orders",
            &[
                ("id", DataValue::Integer(order)),
                ("customer_id", DataValue::Integer(1)),
                ("ref", DataValue::Text(reference.into())),
            ],
        );
    }
    for (line, order, item) in [(1, 1, "Bolt"), (2, 1, "Nut"), (3, 2, "Gear")] {
        h.insert_row(
            "lines",
            &[
                ("id", DataValue::Integer(line)),
                ("order_id", DataValue::Integer(order)),
                ("item", DataValue::Text(item.into())),
            ],
        );
    }
    add_query(&mut h, "q-orders", "Orders", "SELECT * FROM orders ORDER BY id");
    add_query(&mut h, "q-lines", "Lines", "SELECT * FROM lines ORDER BY id");
    let lines = named_report("r-lines", "Order lines", "q-lines", band(14.0, vec![component_json("l1", "'Item ' & record.item", 0.0)], false));
    let subreport = json!({
        "id": "sub", "kind": "subreport", "reportId": "r-lines",
        "links": [{"child": "order_id", "master": "id"}], "x": 20, "y": 14, "w": 300, "h": 20,
    });
    let invoices = named_report(
        "r-invoices",
        "Invoices",
        "q-orders",
        band(
            48.0,
            vec![
                component_json("o1", "'Order ' & record.ref", 0.0),
                subreport,
                component_json("o2", "'End of ' & record.ref", 34.0),
            ],
            false,
        ),
    );
    edit_store(&mut h, |c| c.reports.extend([lines, invoices]));
    assert_eq!(validation_issues_for_reports(&h), Vec::<String>::new());

    open_reports(&mut h);
    // Reports added straight to the config are on no page yet.
    h.click(id("loose-report:r-invoices"));
    assert_eq!(h.label("mode-title"), "Invoices");
    h.click("component-sub");
    // The chosen report is named on the choice; the link fields are listed.
    assert_eq!(h.label("subreport-report"), "Order lines");
    assert!(h.exists("link-1-child"));
    assert!(h.exists("link-1-parent"));
    let invoices = h.config().reports.into_iter().find(|r| r.id == "r-invoices").expect("invoices");
    let sub = invoices.bands.detail.components.into_iter().find(|c| c.id == "sub").expect("subreport");
    let link = &sub.extra["links"][0];
    assert_eq!((link["child"].as_str(), link["master"].as_str()), (Some("order_id"), Some("id")));

    h.click("tab-preview");
    wait_for_page(&mut h, "Page 1 of 1");
    let printed: Vec<String> = page_texts(&mut h)
        .into_iter()
        .filter(|t| t.starts_with("Order ") || t.starts_with("Item ") || t.starts_with("End of "))
        .collect();
    assert_eq!(
        printed,
        ["Order SO-1", "Item Bolt", "Item Nut", "End of SO-1", "Order SO-2", "Item Gear", "End of SO-2"]
    );
}

#[gpui_kit::test]
fn edits_table_columns_conditions_and_links_in_the_properties_panel(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.create_table("orders", &[("id", "INTEGER", 1), ("region", "TEXT", 0)]);
    add_query(&mut h, "q-orders", "Orders", "SELECT * FROM orders");
    new_report(&mut h);
    choose(&mut h, "report-dataset", "Query: Orders");
    add_component(&mut h, "detail", "table");
    wait_until(&mut h, "the dataset columns", |h| h.exists("table-add-field"));
    choose(&mut h, "table-add-field", "region");
    h.click("add-table-column");
    h.fill("table-col-1-header", "Area");
    h.fill("table-col-2-expression", "record.id &");
    assert!(h.exists("table-col-2-expression-problem"));
    h.fill("table-col-2-expression", "record.id");
    assert!(!h.exists("table-col-2-expression-problem"));
    h.fill("table-col-2-width", "0");
    h.fill("table-col-2-width", "60");
    let table = last_component(&h, |r| &r.bands.detail);
    let columns = table.extra["columns"].as_array().expect("columns");
    assert_eq!(columns.len(), 2);
    assert_eq!(columns[0]["header"], "Area");
    assert_eq!(columns[0]["expression"], "record.region");
    assert_eq!(columns[1]["expression"], "record.id");
    // A width below one is not applied.
    assert_eq!(columns[1]["width"], 60.0);
    h.click("remove-table-col-1");
    let columns = last_component(&h, |r| &r.bands.detail).extra["columns"].as_array().unwrap().len();
    assert_eq!(columns, 1);
    assert_eq!(report(&h).bands.detail.components.len(), 1);
}
