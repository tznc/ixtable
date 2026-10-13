//! Report parameters (port of `tests/integration/report-parameters.test.tsx`). The React test
//! opened the report from Runtime navigation; here a `ReportRun` is hosted in a window of
//! its own, which is what the Runtime shows for a report page. The Runtime navigation
//! itself (the "Sales report" button) is tested with the Runtime port.

use super::test_support::{edit_store, wait_for_page, wait_until};
use super::{ReportPreview, ReportRun};
use crate::data::DataValue;
use crate::reports::Report;
use crate::ui::testing::Harness;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, WindowOptions};
use serde_json::json;

const QUERY: &str = "7a1f8e2a-0c39-4c55-9a4c-2b8f9f6f7a02";

fn seed(h: &mut Harness) -> String {
    h.create_table("sales", &[("id", "INTEGER", 1), ("region", "TEXT", 0), ("amount", "REAL", 0)]);
    let rows = [("West", 120.0), ("East", 80.0), ("West", 40.0), ("West", 300.0)];
    for (i, (region, amount)) in rows.into_iter().enumerate() {
        h.insert_row(
            "sales",
            &[
                ("id", DataValue::Integer(i as i64 + 1)),
                ("region", DataValue::Text(region.into())),
                ("amount", DataValue::Real(amount)),
            ],
        );
    }
    let report: Report = serde_json::from_value(json!({
        "id": "r-sales", "name": "Regional sales", "datasetQueryId": QUERY, "params": {"region": "West"},
        "page": {"size": "A4", "orientation": "portrait", "margins": {"top": 36, "right": 36, "bottom": 36, "left": 36}},
        "bands": {
            "reportHeader": {"height": 0, "keepTogether": false, "components": []},
            "pageHeader": {"height": 0, "keepTogether": false, "components": []},
            "groups": [],
            "detail": {"height": 16, "keepTogether": false, "components": [
                {"id": "f1", "kind": "field", "expression": "record.region & ' ' & record.amount", "x": 0, "y": 0, "w": 200, "h": 16}
            ]},
            "pageFooter": {"height": 0, "keepTogether": false, "components": []},
            "reportFooter": {"height": 0, "keepTogether": false, "components": []},
        },
    }))
    .expect("report");
    let query = serde_json::from_value(json!({
        "id": QUERY, "name": "Sales by region",
        "sql": "SELECT region, amount FROM sales WHERE region = $region AND amount >= $minimum ORDER BY id",
        "parameters": [
            {"name": "region", "logicalType": "text", "required": true},
            {"name": "minimum", "logicalType": "number", "required": true, "defaultValue": 0},
        ],
    }))
    .expect("query");
    let report_id = report.id.clone();
    edit_store(h, |c| {
        c.saved_queries.push(query);
        c.reports.push(report);
    });
    report_id
}

/// Opens a window holding the report as the Runtime shows it, and points the harness at it.
fn host(h: &mut Harness, report_id: &str) -> (AnyWindowHandle, Entity<ReportRun>) {
    let shell = h.shell();
    let report_id = report_id.to_string();
    let (window, run) = h.cx.update(|cx| {
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
            cx.new(|cx| ReportRun::new(shell, &report_id, None, cx))
        })
        .expect("open the report window")
    });
    h.window = window;
    h.settle();
    (window, run)
}

fn page_text(h: &mut Harness, run: &Entity<ReportRun>) -> Vec<String> {
    let preview: Entity<ReportPreview> = h.cx.read(|cx| run.read(cx).preview.clone()).expect("a preview");
    h.cx.read(|cx| preview.read(cx).page_texts())
}

#[gpui_kit::test]
fn prompts_for_report_parameters_when_a_parameterised_report_is_opened(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let report_id = seed(&mut h);
    let studio = h.window;
    let (_, run) = host(&mut h, &report_id);

    assert_eq!(h.label("report-params"), "Regional sales parameters");
    let text = |h: &mut Harness, name: &str| h.cx.read(|cx| run.read(cx).prompt_text(name, cx));
    assert_eq!(text(&mut h, "region").as_deref(), Some("West"));
    assert_eq!(text(&mut h, "minimum").as_deref(), Some("0"));
    assert!(!h.exists("report-page"));

    h.fill("param-minimum", "");
    h.click("params-run");
    assert_eq!(h.label("params-error"), "Enter a value for minimum.");
    h.fill("param-minimum", "100");
    h.click("params-run");
    assert!(!h.exists("report-params"));
    wait_for_page(&mut h, "Page 1 of 1");
    let shown = page_text(&mut h, &run);
    assert!(shown.contains(&"West 120".to_string()), "{shown:?}");
    assert!(shown.contains(&"West 300".to_string()), "{shown:?}");
    assert!(!shown.contains(&"West 40".to_string()));
    assert!(!shown.iter().any(|t| t.contains("East")));

    h.click("change-params");
    assert_eq!(h.label("report-params"), "Regional sales parameters");
    assert_eq!(text(&mut h, "minimum").as_deref(), Some("100"));
    h.click("params-cancel");
    assert!(!h.exists("report-params"));
    assert!(page_text(&mut h, &run).contains(&"West 300".to_string()));

    // A report opened again starts from the prompt; escape leaves it not run.
    let (_, again) = host(&mut h, &report_id);
    assert!(h.exists("report-params"));
    h.press("escape");
    wait_until(&mut h, "the prompt to close", |h| !h.exists("report-params"));
    assert_eq!(h.label("report-not-run"), "The report was not run.");
    assert_eq!(h.label("change-params"), "Enter parameters…");
    h.click("change-params");
    assert!(h.exists("report-params"));
    assert!(h.cx.read(|cx| again.read(cx).values.is_none()));
    h.window = studio;
}
