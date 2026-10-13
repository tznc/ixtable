//! Port of `report-phase6.test.ts`: running sums, conditional formatting and charts.

use super::chart_paths::{arc_ops, offset_path, wedge_path};
use super::fixtures::{band, base_json, lay, lay_with, orders, rows};
use super::pdf::{write_pdf, PdfOptions};
use super::values::RUNNING_NOT_NUMBER;
use super::{ChartItem, Item, LayoutOptions, Mark, PathOp, ReportDocument, Row, TextItem};
use serde_json::{json, Value};

fn items(doc: &ReportDocument) -> Vec<&Item> {
    doc.pages.iter().flat_map(|p| p.items.iter()).collect()
}

fn texts_of<'a>(doc: &'a ReportDocument, id: &str) -> Vec<&'a TextItem> {
    items(doc)
        .into_iter()
        .filter_map(|i| match i {
            Item::Text(t) if t.component_id == id => Some(t),
            _ => None,
        })
        .collect()
}

fn chart_of<'a>(doc: &'a ReportDocument, id: &str) -> Option<&'a ChartItem> {
    items(doc).into_iter().find_map(|i| match i {
        Item::Chart(c) if c.component_id == id => Some(c),
        _ => None,
    })
}

fn fld(id: &str, expression: &str, patch: Value) -> Value {
    let mut c = json!({ "id": id, "kind": "field", "x": 0, "y": 0, "w": 120, "h": 14, "expression": expression });
    for (k, v) in patch.as_object().unwrap() {
        c[k] = v.clone();
    }
    c
}

fn group(patch: Value) -> Value {
    let mut g = json!({ "id": "g", "groupBy": "record.region", "header": band(0.0, vec![], false),
        "footer": band(0.0, vec![], false) });
    for (k, v) in patch.as_object().unwrap() {
        g[k] = v.clone();
    }
    g
}

fn strings(list: Vec<&TextItem>) -> Vec<String> {
    list.into_iter().map(|t| t.text.clone()).collect()
}

fn diag(doc: &ReportDocument) -> Value {
    serde_json::to_value(&doc.diagnostics).unwrap()
}

#[test]
fn running_sums_accumulate_over_all_rows_or_restart_at_each_group() {
    let report = base_json(vec![
        ("groups", json!([group(json!({}))])),
        (
            "detail",
            band(
                14.0,
                vec![
                    fld("all", "record.amount", json!({ "runningSum": "all", "x": 0 })),
                    fld("grp", "record.amount", json!({ "runningSum": "group", "x": 130 })),
                    fld("plain", "record.amount", json!({ "x": 260 })),
                ],
                false,
            ),
        ),
    ]);
    let doc = lay(&report, &orders());
    assert_eq!(strings(texts_of(&doc, "all")), ["80", "90", "390", "510.5", "552.75"]);
    assert_eq!(strings(texts_of(&doc, "grp")), ["80", "90", "300", "120.5", "162.75"]);
    assert_eq!(strings(texts_of(&doc, "plain")), ["80", "10", "300", "120.5", "42.25"]);
    assert_eq!(diag(&doc), json!([]));
}

#[test]
fn running_sums_sum_group_footers_across_groups_restarting_at_the_enclosing_group() {
    let total = json!({ "id": "tot", "kind": "calculated", "x": 0, "y": 0, "w": 120, "h": 14,
        "expression": "sum(rows.amount)", "runningSum": "group" });
    let outer = group(json!({ "id": "o", "groupBy": "record.qty > 1" }));
    let inner = group(json!({ "id": "i", "footer": band(14.0, vec![total], false) }));
    let doc = lay(&base_json(vec![("groups", json!([outer, inner]))]), &orders());
    assert_eq!(strings(texts_of(&doc, "tot")), ["80", "380", "10", "172.75"]);
}

#[test]
fn running_sums_apply_formats_skip_nulls_and_report_non_numbers() {
    let data = rows(json!([{ "v": 1 }, { "v": null }, { "v": 2.5 }, { "v": "x" }, { "v": 3 }]));
    let report = base_json(vec![(
        "detail",
        band(14.0, vec![fld("s", "record.v", json!({ "runningSum": "all", "format": "0.00" }))], false),
    )]);
    let doc = lay(&report, &data);
    assert_eq!(strings(texts_of(&doc, "s")), ["1.00", "1.00", "3.50", "#Error", "#Error"]);
    assert_eq!(diag(&doc), json!([{ "componentId": "s", "message": RUNNING_NOT_NUMBER }]));
}

#[test]
fn running_sums_print_the_plain_value_outside_detail_and_group_bands() {
    let report = base_json(vec![(
        "reportFooter",
        band(14.0, vec![fld("f", "sum(rows.amount)", json!({ "runningSum": "all" }))], false),
    )]);
    assert_eq!(texts_of(&lay(&report, &orders()), "f")[0].text, "552.75");
}

fn conditions_report() -> Value {
    base_json(vec![(
        "detail",
        band(
            14.0,
            vec![fld(
                "a",
                "record.amount",
                json!({ "conditions": [
                    { "id": "neg", "when": "value < 0", "style": { "gray": 0.5, "fill": 0.9 } },
                    { "id": "big", "when": "record.amount >= 100", "style": { "bold": true } },
                    { "id": "never", "when": "value > 1000", "style": { "bold": false } },
                ] }),
            )],
            false,
        ),
    )])
}

#[test]
fn conditional_formatting_applies_the_first_matching_rules_style() {
    let doc = lay(&conditions_report(), &rows(json!([{ "amount": -5 }, { "amount": 10 }, { "amount": 500 }])));
    let texts = texts_of(&doc, "a");
    let got: Vec<(String, f64, bool)> = texts.iter().map(|t| (t.text.clone(), t.gray, t.bold)).collect();
    let want: Vec<(String, f64, bool)> =
        vec![("-5".into(), 0.5, false), ("10".into(), 0.0, false), ("500".into(), 0.0, true)];
    assert_eq!(got, want);
    let fills: Vec<_> = items(&doc)
        .into_iter()
        .filter_map(|i| match i {
            Item::Rect(r) if r.component_id == "a" => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(fills.len(), 1);
    assert_eq!((fills[0].fill, fills[0].y), (Some(0.9), texts[0].y));
}

#[test]
fn conditional_formatting_measures_grown_text_with_the_conditional_style() {
    let words = |amount: i64| rows(json!([{ "amount": amount, "label": "Mmmmm Mmmmm" }]));
    let report = base_json(vec![(
        "detail",
        band(
            14.0,
            vec![fld(
                "a",
                "record.label",
                json!({ "canGrow": true, "w": 92, "conditions": [
                    { "id": "b", "when": "record.amount >= 100", "style": { "bold": true } }] }),
            )],
            false,
        ),
    )]);
    assert_eq!(texts_of(&lay(&report, &words(1)), "a")[0].lines.len(), 1);
    let bold_doc = lay(&report, &words(100));
    let bold = texts_of(&bold_doc, "a")[0];
    assert!(bold.bold);
    assert_eq!(bold.lines.len(), 2);
}

#[test]
fn conditional_formatting_reports_a_failing_condition_and_keeps_the_base_style() {
    let report = base_json(vec![(
        "detail",
        band(
            14.0,
            vec![fld(
                "a",
                "record.amount",
                json!({ "conditions": [{ "id": "x", "when": "value + 'a'", "style": { "bold": true } }] }),
            )],
            false,
        ),
    )]);
    let doc = lay(&report, &rows(json!([{ "amount": 1 }])));
    assert!(!texts_of(&doc, "a")[0].bold);
    assert_eq!(doc.diagnostics[0].component_id, "a");
    assert!(doc.diagnostics[0].message.starts_with("Condition: "));
}

fn chart(patch: Value) -> Value {
    let mut c = json!({ "id": "ch", "kind": "chart", "x": 10, "y": 0, "w": 300, "h": 160,
        "chartType": "bar", "xField": "region", "yFields": ["amount"] });
    for (k, v) in patch.as_object().unwrap() {
        c[k] = v.clone();
    }
    c
}

fn chart_layout(c: Value, data: &[Row], tables: Vec<(&str, Vec<Row>)>) -> ReportDocument {
    let mut options = LayoutOptions::default();
    for (id, r) in tables {
        options.tables.insert(id.into(), r);
    }
    lay_with(&base_json(vec![("reportFooter", band(170.0, vec![c], false))]), data, &options)
}

fn labels(item: &ChartItem) -> Vec<String> {
    item.marks
        .iter()
        .filter_map(|m| match m {
            Mark::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn charts_draw_bars_from_the_bands_rows_with_the_dashboard_palette() {
    let doc = chart_layout(chart(json!({ "title": "Sales by region" })), &orders(), vec![]);
    let item = chart_of(&doc, "ch").unwrap();
    assert_eq!((item.x, item.y, item.w, item.h), (46.0, 36.0, 300.0, 160.0));
    assert_eq!(item.title, "Sales by region");
    let bars = item
        .marks
        .iter()
        .filter(|m| matches!(m, Mark::Path(p) if p.fill.as_deref() == Some("#2a78d6")))
        .count();
    assert_eq!(bars, 3);
    let l = labels(item);
    for want in ["Sales by region", "West", "East", "North"] {
        assert!(l.iter().any(|x| x == want), "missing label {want}");
    }
    for mark in &item.marks {
        if let Mark::Path(p) = mark {
            for op in &p.ops {
                let (x, y) = match op {
                    PathOp::M(x, y) | PathOp::L(x, y) => (*x, *y),
                    PathOp::C(_, _, _, _, x, y) => (*x, *y),
                    PathOp::Z => continue,
                };
                assert!((46.0 - 0.01..=346.0 + 0.01).contains(&x));
                assert!((36.0 - 0.01..=196.0 + 0.01).contains(&y));
            }
        }
    }
}

#[test]
fn charts_are_deterministic() {
    let c = || chart(json!({ "chartType": "line", "groupBy": "customer" }));
    assert_eq!(chart_layout(c(), &orders(), vec![]), chart_layout(c(), &orders(), vec![]));
}

#[test]
fn charts_add_a_legend_for_two_or_more_series() {
    let doc = chart_layout(chart(json!({ "yFields": ["amount", "price"] })), &orders(), vec![]);
    let l = labels(chart_of(&doc, "ch").unwrap());
    assert!(l.iter().any(|x| x == "amount") && l.iter().any(|x| x == "price"));
}

#[test]
fn charts_read_a_saved_querys_rows_when_query_id_is_set() {
    let q = rows(json!([{ "region": "Q", "amount": 1 }]));
    let doc = chart_layout(chart(json!({ "queryId": "q" })), &orders(), vec![("q", q)]);
    assert!(labels(chart_of(&doc, "ch").unwrap()).contains(&"Q".to_string()));
    let missing = chart_layout(chart(json!({ "queryId": "other" })), &orders(), vec![]);
    assert_eq!(diag(&missing), json!([{ "componentId": "ch", "message": "Chart query data is not loaded" }]));
}

#[test]
fn charts_draw_pie_donut_area_and_scatter() {
    for kind in ["pie", "donut", "area", "scatter"] {
        let x = if kind == "scatter" { "qty" } else { "region" };
        let doc = chart_layout(chart(json!({ "chartType": kind, "xField": x })), &orders(), vec![]);
        let item = chart_of(&doc, "ch").unwrap();
        assert!(
            item.marks.iter().any(|m| matches!(m, Mark::Path(p) if p.fill.is_some())),
            "{kind}"
        );
    }
    let pie = chart_layout(chart(json!({ "chartType": "pie" })), &orders(), vec![]);
    assert_eq!(labels(chart_of(&pie, "ch").unwrap()), ["West (29%)", "East (16%)", "North (54%)"]);
}

#[test]
fn charts_say_no_data_when_nothing_can_be_drawn() {
    let doc = chart_layout(chart(json!({})), &[], vec![]);
    let item = chart_of(&doc, "ch").unwrap();
    assert_eq!(item.marks.len(), 1);
    assert!(matches!(&item.marks[0], Mark::Text(t) if t.text == "No data"));
}

#[test]
fn charts_move_to_the_next_page_whole_instead_of_splitting() {
    let tall = base_json(vec![
        ("detail", band(700.0, vec![], false)),
        ("reportFooter", band(170.0, vec![chart(json!({}))], false)),
    ]);
    let doc = lay(&tall, &rows(json!([{ "region": "A", "amount": 1 }])));
    assert_eq!(doc.pages.len(), 2);
    let moved = doc.pages[1].items.iter().find(|i| matches!(i, Item::Chart(_))).unwrap();
    assert_eq!(moved.y(), 36.0);
}

#[test]
fn charts_write_rgb_paths_to_the_pdf() {
    let doc = chart_layout(chart(json!({})), &orders(), vec![]);
    let bytes = write_pdf(
        &doc,
        &PdfOptions { creation_date: "2026-01-01T00:00:00Z".into(), ..PdfOptions::default() },
    );
    let pdf: String = bytes.iter().map(|b| *b as char).collect();
    assert!(pdf.contains("0.16 0.47 0.84 rg"));
    assert!(pdf.contains(" c\n") || pdf.contains("re f") || pdf.contains("h\n"));
    assert!(pdf.contains(" l"));
}

#[test]
fn chart_paths_offset_a_path_in_page_points() {
    let ops = offset_path(&[PathOp::M(1.0, 2.0), PathOp::L(3.0, 4.0), PathOp::Z], 10.0, 20.0);
    assert_eq!(ops, [PathOp::M(11.0, 22.0), PathOp::L(13.0, 24.0), PathOp::Z]);
}

#[test]
fn chart_paths_approximate_arcs_with_at_most_quarter_turn_bezier_segments() {
    assert_eq!(arc_ops(0.0, 0.0, 10.0, 0.0, std::f64::consts::PI).len(), 2);
    let ops = arc_ops(0.0, 0.0, 10.0, 0.0, std::f64::consts::FRAC_PI_2);
    let PathOp::C(_, _, _, _, x, y) = ops[0] else { panic!("curve") };
    assert!(x.abs() < 1e-9 && (y - 10.0).abs() < 1e-9);
}

#[test]
fn chart_paths_draw_full_donuts_with_an_opposite_direction_hole() {
    let ops = wedge_path(0.0, 0.0, 10.0, 5.0, 0.0, 2.0 * std::f64::consts::PI);
    assert_eq!(ops.iter().filter(|op| matches!(op, PathOp::M(..))).count(), 2);
    assert_eq!(ops.iter().filter(|op| matches!(op, PathOp::Z)).count(), 2);
}
