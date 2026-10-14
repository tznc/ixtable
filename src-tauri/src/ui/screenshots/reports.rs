//! Reports mode (`reports-NN-*`): the designer, preview, PDF export, paging, the parameter
//! prompt, Unicode text that grows, running sums, rules and charts. Port of
//! `scripts/screenshot/specs/reports.spec.tsx`.
//!
//! The window is tall so the whole properties panel is visible: the harness only clicks
//! what is on screen. The Runtime page of the original spec is a `ReportRun` in a window
//! of its own, which is what the Runtime shows for a report page.

use crate::data::DataValue;
use crate::reports::Report;
use crate::ui::reports::test_support::{
    add_component, add_query, choose, edit_store, id, new_report, page_texts, preview, wait_for_page, wait_until,
};
use crate::ui::reports::ReportRun;
use crate::ui::screenshot::context;
use crate::ui::testing::{Harness, UiContext as _};
use gpui_kit::{AppContext as _, HeadlessAppContext, WindowOptions};
use serde_json::json;

const WINDOW: (f32, f32) = (1400.0, 1700.0);
const QUERY_SQL: &str = "SELECT o.id, c.name AS customer, o.status, o.amount FROM orders o JOIN customers c ON c.id = o.customer_id ORDER BY o.status, o.id";

fn start(cx: &mut HeadlessAppContext) -> Harness<'_, HeadlessAppContext> {
    let mut h = Harness::start_sized(cx, WINDOW);
    h.click("new-document");
    assert!(h.has_shell());
    h
}

fn seed_sales(h: &mut Harness<'_, HeadlessAppContext>) {
    h.create_table("customers", &[("id", "INTEGER", 1), ("name", "TEXT", 0), ("city", "TEXT", 0)]);
    h.create_table(
        "orders",
        &[("id", "INTEGER", 1), ("customer_id", "INTEGER", 0), ("status", "TEXT", 0), ("amount", "REAL", 0)],
    );
    for (n, name, city) in [(1, "Northstar Goods", "Leeds"), (2, "Juniper Supply", "Bristol"), (3, "Harbor & Pine", "York")] {
        h.insert_row(
            "customers",
            &[("id", DataValue::Integer(n)), ("name", DataValue::Text(name.into())), ("city", DataValue::Text(city.into()))],
        );
    }
    let orders = [(1, 1, "open", 120.0), (2, 1, "shipped", 340.0), (3, 2, "open", 75.5), (4, 3, "shipped", 410.0), (5, 2, "cancelled", 20.0)];
    for (n, customer, status, amount) in orders {
        h.insert_row(
            "orders",
            &[
                ("id", DataValue::Integer(n)),
                ("customer_id", DataValue::Integer(customer)),
                ("status", DataValue::Text(status.into())),
                ("amount", DataValue::Real(amount)),
            ],
        );
    }
}

/// A new report on the orders-with-customers query, named `name`, with the dataset's columns loaded.
fn start_report(h: &mut Harness<'_, HeadlessAppContext>, name: &str, with_group: bool) {
    new_report(h);
    h.fill("report-name", name);
    choose(h, "report-dataset", "Query: Orders with customers");
    if with_group {
        h.click("add-group");
        wait_until(h, "the dataset columns", |h| h.exists("group-1-field"));
        choose(h, "group-1-field", "status");
    }
}

fn add_bound_field(h: &mut Harness<'_, HeadlessAppContext>, band: &str, column: &str) {
    add_component(h, band, "field");
    wait_until(h, "the field picker", |h| h.exists("prop-bound-field"));
    choose(h, "prop-bound-field", column);
}

fn group_band(h: &Harness<'_, HeadlessAppContext>, which: &str) -> String {
    format!("{which}:{}", h.config().reports[0].bands.groups[0].id)
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn reports_design_preview_and_export() {
    let mut cx = context();
    let mut h = start(&mut cx);
    seed_sales(&mut h);
    add_query(&mut h, "q-orders", "Orders with customers", QUERY_SQL);
    start_report(&mut h, "Orders by status", true);

    add_component(&mut h, "reportHeader", "staticText");
    h.fill("prop-text", "Orders by status");
    let header = group_band(&h, "groupHeader");
    add_bound_field(&mut h, &header, "status");
    add_bound_field(&mut h, "detail", "customer");
    add_bound_field(&mut h, "detail", "amount");
    let amount = h.config().reports[0].bands.detail.components[1].id.clone();
    h.click(id(&format!("component-{amount}")));
    let before = h.config().reports[0].bands.detail.components[1].x;
    for _ in 0..4 {
        h.press("shift-right");
    }
    assert_eq!(h.config().reports[0].bands.detail.components[1].x, before + 40.0);
    let footer = group_band(&h, "groupFooter");
    add_component(&mut h, &footer, "calculated");
    h.fill("prop-expression", "'Subtotal ' & format(sum(rows.amount), '#,##0.00')");
    add_component(&mut h, "reportFooter", "calculated");
    h.fill("prop-expression", "'Grand total ' & format(sum(rows.amount), '#,##0.00')");
    add_component(&mut h, "pageFooter", "calculated");
    h.fill("prop-expression", "'Page ' & page & ' of ' & pages");
    h.capture(
        "reports-01-designer",
        &[
            "The band canvas shows report header, group header/footer, detail, and footers with their components.",
            "The selected page-footer calculated field and its expression appear in the properties panel.",
        ],
    );

    h.click("tab-preview");
    wait_for_page(&mut h, "Page 1 of 1");
    let shown = page_texts(&mut h);
    assert!(shown.contains(&"Orders by status".to_string()), "{shown:?}");
    assert!(shown.contains(&"Grand total 965.50".to_string()), "{shown:?}");
    h.capture(
        "reports-02-preview",
        &[
            "The preview page shows the title, one group per status with customer rows and subtotals.",
            "The grand total 965.50 and 'Page 1 of 1' footer are visible on the page.",
        ],
    );

    // Print writes the PDF to a file chosen in the save dialog, as Export PDF does.
    let (printed, pdf) = (h.temp_path("orders-printed.pdf"), h.temp_path("orders-by-status.pdf"));
    h.dialogs.queue_save(Some(printed));
    h.click("print");
    wait_until(&mut h, "the print export", |h| h.notice().is_some());
    h.dialogs.queue_save(Some(pdf.clone()));
    h.click("export-pdf");
    let want = "Exported orders-by-status.pdf".to_string();
    wait_until(&mut h, "the PDF export", |h| h.notice().as_deref() == Some(want.as_str()));
    assert_eq!(&std::fs::read(&pdf).expect("pdf")[..5], b"%PDF-");
    h.capture(
        "reports-03-exported",
        &["The preview toolbar toasts the exported PDF name after Print and Export."],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn reports_paging() {
    let mut cx = context();
    let mut h = start(&mut cx);
    seed_sales(&mut h);
    add_query(&mut h, "q-orders", "Orders with customers", QUERY_SQL);
    start_report(&mut h, "Orders paged by status", true);
    h.click("group-1-repeatHeader");
    h.click("group-1-resetPageNumber");
    let group = h.config().reports[0].bands.groups[0].clone();
    assert!(group.repeat_header && group.reset_page_number);
    assert_eq!(h.label("group-1-newpage-hint"), "Always on while group page numbers restart.");

    add_component(&mut h, "reportHeader", "staticText");
    h.fill("prop-text", "Orders paged by status");
    h.click("band-reportHeader");
    h.click("band-break-after");
    assert!(h.config().reports[0].bands.report_header.page_break_after);
    let header = group_band(&h, "groupHeader");
    add_component(&mut h, &header, "calculated");
    h.fill("prop-expression", "'Status: ' & record.status");
    add_bound_field(&mut h, "detail", "customer");
    h.click("band-detail");
    h.fill("band-height", "450");
    assert_eq!(h.config().reports[0].bands.detail.height, 450.0);
    add_component(&mut h, "pageFooter", "calculated");
    h.fill("prop-expression", "'Part ' & groupPage & ' of ' & groupPages & ' / page ' & page");
    h.click(id(&format!("band-{header}")));
    h.capture(
        "reports-04-paging-settings",
        &[
            "Group 1 has 'repeats header on each page' and 'restarts group page numbers' checked.",
            "'Group 1 starts a new page' is checked and disabled with the hint 'Always on while group page numbers restart.'",
        ],
    );

    h.click("tab-preview");
    wait_until(&mut h, "the first page", |h| h.try_label("report-page").is_some_and(|l| l.starts_with("Page 1 of ")));
    let label = h.label("report-page");
    let total: usize = label.rsplit(' ').next().and_then(|n| n.parse().ok()).expect("page count");
    assert!(total > 2, "{label}");
    let mut found = false;
    for n in 1..=total {
        if n > 1 {
            h.click("next-page");
        }
        wait_for_page(&mut h, &format!("Page {n} of {total}"));
        let texts = page_texts(&mut h);
        if texts.iter().any(|t| t == "Status: open") && texts.iter().any(|t| t.starts_with("Part 2 of 2")) {
            found = true;
            break;
        }
    }
    assert!(found, "no page repeats the open group header with 'Part 2 of 2'");
    h.capture(
        "reports-05-repeated-group-header",
        &[
            "The preview page repeats the 'Status: open' group header above the continued detail row.",
            "The page footer reads 'Part 2 of 2' for the open group.",
        ],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn reports_parameters_and_unicode() {
    let mut cx = context();
    let mut h = start(&mut cx);
    seed_sales(&mut h);
    let query = serde_json::from_value(json!({
        "id": "q-status", "name": "Orders by status",
        "sql": "SELECT o.id, c.name AS customer, o.status, o.amount FROM orders o JOIN customers c ON c.id = o.customer_id WHERE o.status = $status AND o.amount >= $minimum ORDER BY o.id",
        "parameters": [
            {"name": "status", "logicalType": "text", "required": true},
            {"name": "minimum", "logicalType": "number", "defaultValue": 0},
        ],
    }))
    .expect("query");
    let report: Report = serde_json::from_value(json!({
        "id": "r-review", "name": "Orders for review", "datasetQueryId": "q-status", "params": {"status": "open"},
        "page": {"size": "A4", "orientation": "portrait", "margins": {"top": 36, "right": 36, "bottom": 36, "left": 36}},
        "bands": {
            "reportHeader": {"height": 40, "keepTogether": false, "components": [
                {"id": "t1", "kind": "staticText", "x": 0, "y": 0, "w": 260, "h": 16, "canGrow": true,
                 "text": "Revue des commandes · Обзор заказов · Ανασκόπηση παραγγελιών · 订单审核 → ✓ Notes for the review meeting grow with their text.",
                 "style": {"borderWidth": 0.5}},
                {"id": "t2", "kind": "staticText", "text": "Customer · Amount", "x": 0, "y": 22, "w": 260, "h": 16, "style": {"bold": true}},
            ]},
            "pageHeader": {"height": 0, "keepTogether": false, "components": []},
            "groups": [],
            "detail": {"height": 16, "keepTogether": false, "components": [
                {"id": "d1", "kind": "field", "expression": "record.customer & ' · ' & record.amount", "x": 0, "y": 0, "w": 260, "h": 16}
            ]},
            "pageFooter": {"height": 0, "keepTogether": false, "components": []},
            "reportFooter": {"height": 0, "keepTogether": false, "components": []},
        },
    }))
    .expect("report");
    edit_store(&mut h, |c| {
        c.saved_queries.push(query);
        c.reports.push(report);
    });

    // The Runtime shows a report page as a `ReportRun`; host one in a window of its own.
    let shell = h.shell();
    let (window, run) = h.cx.update(|cx| {
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
            cx.new(|cx| ReportRun::new(shell, "r-review", None, cx))
        })
        .expect("open the report window")
    });
    h.window = window;
    h.settle();
    assert_eq!(h.label("report-params"), "Orders for review parameters");
    h.capture(
        "reports-06-parameter-prompt",
        &[
            "A dialog titled 'Orders for review' asks for status (prefilled 'open', required) and minimum (0).",
            "The dialog has Cancel and Run report buttons and no report page is shown behind it.",
        ],
    );

    h.click("params-run");
    wait_for_page(&mut h, "Page 1 of 1");
    let preview = h.cx.read(|cx| run.read(cx).preview.clone()).expect("a preview");
    let texts = h.cx.read(|cx| preview.read(cx).page_texts());
    assert!(texts.iter().any(|t| t.contains("Northstar Goods")), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("订单审核")), "{texts:?}");
    assert!(!texts.iter().any(|t| t.contains("Harbor")), "{texts:?}");
    h.capture(
        "reports-07-unicode-can-grow",
        &[
            "The header text with French, Russian, Greek and Chinese wraps over several lines inside a grown bordered box.",
            "The bold 'Customer · Amount' heading and the open orders sit below the grown box without overlapping it.",
        ],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn reports_running_sum_and_charts() {
    let mut cx = context();
    let mut h = start(&mut cx);
    seed_sales(&mut h);
    add_query(&mut h, "q-orders", "Orders with customers", QUERY_SQL);
    start_report(&mut h, "Running totals", false);
    for column in ["customer", "status"] {
        add_component(&mut h, "detail", "field");
        wait_until(&mut h, "the field picker", |h| h.exists("prop-bound-field"));
        choose(&mut h, "prop-bound-field", column);
    }
    add_component(&mut h, "detail", "field");
    choose(&mut h, "prop-bound-field", "amount");
    choose(&mut h, "prop-running-sum", "Over all");
    h.fill("prop-format", "#,##0.00");
    h.click("add-rule");
    h.fill("rule-1-when", "value > 500");
    choose(&mut h, "rule-1-fill", "Light gray");
    let rule = &h.config().reports[0].bands.detail.components[2].extra["conditions"][0];
    assert_eq!(rule["style"], json!({"bold": true, "fill": 0.9}));
    h.capture(
        "reports-08-running-sum-rule",
        &[
            "The detail band holds customer, status and amount fields; the amount field is selected.",
            "The properties panel shows Running sum 'Over all' and conditional formatting rule 1 'value > 500' with Bold checked and a light gray fill.",
        ],
    );

    add_component(&mut h, "reportFooter", "chart");
    h.fill("chart-title", "Amount by status");
    h.fill("chart-x", "status");
    h.fill("chart-y", "amount");
    h.fill("prop-w", "255");
    add_component(&mut h, "reportFooter", "chart");
    choose(&mut h, "chart-type", "Donut");
    h.fill("chart-title", "Amount by customer");
    h.fill("chart-x", "customer");
    h.fill("chart-y", "amount");
    h.fill("prop-w", "260");
    h.fill("prop-x", "263");
    let charts = &h.config().reports[0].bands.report_footer.components;
    assert_eq!((charts[1].x, charts[1].w), (263.0, 260.0));
    h.capture(
        "reports-09-chart-designer",
        &[
            "The report footer band holds two chart components side by side, labelled 'bar chart: amount' and 'donut chart: amount'.",
            "The properties panel shows the chart settings: type Donut, title 'Amount by customer', category column customer, value column amount.",
        ],
    );

    h.click("tab-preview");
    wait_for_page(&mut h, "Page 1 of 1");
    let texts = page_texts(&mut h);
    for total in ["20.00", "140.00", "215.50", "555.50", "965.50"] {
        assert!(texts.iter().any(|t| t == total), "{total} missing from {texts:?}");
    }
    let preview = preview(&mut h);
    let doc = h.cx.read(|cx| preview.read(cx).document().cloned()).expect("document");
    let chart_text: Vec<String> = doc.pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            crate::report_engine::Item::Chart(c) => Some(c),
            _ => None,
        })
        .flat_map(|c| c.marks.iter())
        .filter_map(|m| match m {
            crate::report_engine::Mark::Text(t) => Some(t.lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect();
    for label in ["cancelled", "open", "shipped", "Northstar Goods (48%)"] {
        assert!(chart_text.iter().any(|t| t == label), "{label} missing from {chart_text:?}");
    }
    h.capture(
        "reports-10-running-sum-charts-preview",
        &[
            "The amount column is a running total (20.00, 140.00, 215.50, 555.50, 965.50); the last two are bold on a light gray fill.",
            "Below the rows, a blue bar chart 'Amount by status' with bars for cancelled, open and shipped sits beside a donut 'Amount by customer' with a legend of three customers and their shares.",
        ],
    );
}
