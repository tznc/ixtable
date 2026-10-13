//! Port of `report-subreport.test.ts`.

use super::component::{Band, Comp, Kind};
use super::fixtures::{band, base_json, lay_with, report, rows, texts};
use super::model::{embeddable_reports, report_problems, subreport_depth};
use super::subreport::{
    linked_rows, split_at_subreport, SUBREPORT_DEPTH, SUBREPORT_EXTRA, SUBREPORT_LOOP, SUBREPORT_MISSING,
    SUBREPORT_PAGE_BAND,
};
use super::{LayoutOptions, ReportDocument, Row, SubreportData};
use crate::reports;
use serde_json::{json, Value};

fn fld(id: &str, expression: &str, y: f64, x: f64) -> Value {
    json!({ "id": id, "kind": "field", "expression": expression, "x": x, "y": y, "w": 200, "h": 14 })
}

fn sub(report_id: &str, y: f64, links: Value) -> Value {
    json!({ "id": format!("s-{report_id}"), "kind": "subreport", "reportId": report_id, "links": links,
        "x": 40, "y": y, "w": 300, "h": 20 })
}

fn default_links() -> Value {
    json!([{ "child": "order_id", "master": "id" }])
}

fn invoices() -> Vec<Row> {
    rows(json!([{ "id": 1, "customer": "Acme" }, { "id": 2, "customer": "Birch" }, { "id": 3, "customer": "Cobalt" }]))
}

fn lines() -> Vec<Row> {
    rows(json!([
        { "id": 10, "order_id": 1, "item": "Bolt", "qty": 4 },
        { "id": 11, "order_id": 1, "item": "Nut", "qty": 8 },
        { "id": 12, "order_id": 2, "item": "Gear", "qty": 1 },
        { "id": 13, "order_id": "1", "item": "Washer", "qty": 2 }
    ]))
}

fn named(id: &str, patch: Vec<(&str, Value)>) -> Value {
    let mut r = base_json(patch);
    r["id"] = json!(id);
    r["name"] = json!(id);
    r
}

fn lines_report() -> Value {
    named(
        "lines",
        vec![
            ("reportHeader", band(12.0, vec![fld("lh", "'Lines of ' & parent.customer", 0.0, 0.0)], false)),
            ("detail", band(12.0, vec![fld("ld", "record.item & ' x' & record.qty", 0.0, 0.0)], false)),
            ("reportFooter", band(12.0, vec![fld("lf", "sum(rows.qty)", 0.0, 0.0)], false)),
        ],
    )
}

fn invoice_report(detail_height: f64, extra: Vec<Value>) -> Value {
    let mut comps = vec![
        fld("name", "record.customer", 0.0, 0.0),
        sub("lines", 14.0, default_links()),
        fld("total", "'End ' & record.id", 34.0, 0.0),
    ];
    comps.extend(extra);
    named("invoices", vec![("detail", band(detail_height, comps, false))])
}

fn options(entries: Vec<(&str, Value, Vec<Row>)>) -> LayoutOptions {
    let mut o = LayoutOptions::default();
    for (id, def, data) in entries {
        o.subreports.insert(id.into(), SubreportData { report: report(&def), rows: data });
    }
    o
}

fn xy(doc: &ReportDocument) -> Vec<Vec<(String, String, f64, f64)>> {
    doc.pages
        .iter()
        .map(|p| {
            p.items
                .iter()
                .filter_map(|i| match i {
                    super::Item::Text(t) => Some((t.component_id.clone(), t.text.clone(), t.x, t.y)),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

fn q(a: &str, b: &str, x: f64, y: f64) -> (String, String, f64, f64) {
    (a.into(), b.into(), x, y)
}

fn diag(doc: &ReportDocument) -> Value {
    serde_json::to_value(&doc.diagnostics).unwrap()
}

#[test]
fn prints_linked_rows_at_the_subreports_x_and_pushes_the_band_below_it_down() {
    let o = options(vec![("lines", lines_report(), lines())]);
    let doc = lay_with(&invoice_report(50.0, vec![]), &invoices()[..2], &o);
    assert_eq!(diag(&doc), json!([]));
    assert_eq!(
        xy(&doc),
        vec![vec![
            q("name", "Acme", 36.0, 36.0),
            q("lh", "Lines of Acme", 76.0, 50.0),
            q("ld", "Bolt x4", 76.0, 62.0),
            q("ld", "Nut x8", 76.0, 74.0),
            q("ld", "Washer x2", 76.0, 86.0),
            q("lf", "14", 76.0, 98.0),
            q("total", "End 1", 36.0, 110.0),
            q("name", "Birch", 36.0, 126.0),
            q("lh", "Lines of Birch", 76.0, 140.0),
            q("ld", "Gear x1", 76.0, 152.0),
            q("lf", "1", 76.0, 164.0),
            q("total", "End 2", 36.0, 176.0),
        ]]
    );
}

#[test]
fn prints_nothing_for_a_parent_row_without_linked_rows() {
    let o = options(vec![("lines", lines_report(), lines())]);
    let doc = lay_with(&invoice_report(50.0, vec![]), &invoices()[2..], &o);
    assert_eq!(
        xy(&doc),
        vec![vec![q("name", "Cobalt", 36.0, 36.0), q("total", "End 3", 36.0, 50.0)]]
    );
}

#[test]
fn splits_across_pages_between_subreport_bands() {
    let many = rows(Value::Array(
        (0..80).map(|i| json!({ "order_id": 1, "item": format!("I{i}"), "qty": 1 })).collect(),
    ));
    let o = options(vec![("lines", lines_report(), many)]);
    let doc = lay_with(&invoice_report(50.0, vec![]), &invoices()[..1], &o);
    assert_eq!(doc.pages.len(), 2);
    let pages = xy(&doc);
    let (first, second) = (&pages[0], &pages[1]);
    assert_eq!(first[0], q("name", "Acme", 36.0, 36.0));
    let max = first.iter().map(|t| t.3).fold(f64::MIN, f64::max);
    assert!(max <= 806.0 - 12.0);
    assert_eq!(second[0].3, 36.0);
    let last = &second[second.len() - 1];
    assert_eq!(last, &q("total", "End 1", 36.0, second[second.len() - 2].3 + 12.0));
    let items: Vec<String> = first
        .iter()
        .chain(second.iter())
        .filter(|t| t.0 == "ld")
        .map(|t| t.1.clone())
        .collect();
    let want: Vec<String> = (0..80).map(|i| format!("I{i} x1")).collect();
    assert_eq!(items, want);
}

#[test]
fn counts_running_sums_per_subreport_instance_and_in_the_band_around_it() {
    let mut rq = fld("rq", "record.qty", 0.0, 0.0);
    rq["runningSum"] = json!("all");
    let running = named("running", vec![("detail", band(12.0, vec![rq], false))]);
    let mut rid = fld("rid", "record.id", 0.0, 0.0);
    rid["runningSum"] = json!("all");
    let parent = named(
        "invoices",
        vec![(
            "detail",
            band(
                50.0,
                vec![rid, sub("running", 14.0, default_links()), fld("total", "'End ' & record.id", 34.0, 0.0)],
                false,
            ),
        )],
    );
    let o = options(vec![("running", running, lines())]);
    let doc = lay_with(&parent, &invoices()[..2], &o);
    assert_eq!(diag(&doc), json!([]));
    let text = |id: &str| -> Vec<String> {
        texts(&doc).into_iter().flatten().filter(|t| t.0 == id).map(|t| t.1).collect()
    };
    assert_eq!(text("rq"), ["4", "12", "14", "1"]);
    assert_eq!(text("rid"), ["1", "3"]);
}

#[test]
fn nests_three_levels_and_stops_at_the_fourth_or_at_a_loop() {
    let level = |id: &str, child: Option<&str>| {
        let mut comps = vec![fld(&format!("f-{id}"), &format!("'{id} ' & record.n"), 0.0, 0.0)];
        if let Some(c) = child {
            comps.push(sub(c, 14.0, json!([])));
        }
        named(id, vec![("detail", band(30.0, comps, false))])
    };
    let data = rows(json!([{ "n": 1 }]));
    let chain = |b_child: Option<&str>| {
        let mut o = LayoutOptions::default();
        for (id, child) in [("a", Some("b")), ("b", b_child), ("c", Some("d")), ("d", Some("e"))] {
            o.subreports.insert(id.into(), SubreportData { report: report(&level(id, child)), rows: data.clone() });
        }
        o
    };
    let doc = lay_with(&level("a", Some("b")), &data, &chain(Some("c")));
    let got: Vec<(String, f64)> = xy(&doc)[0].iter().map(|t| (t.1.clone(), t.2)).collect();
    let want: Vec<(String, f64)> = [("a 1", 36.0), ("b 1", 76.0), ("c 1", 116.0), ("d 1", 156.0)]
        .iter()
        .map(|(a, b)| (a.to_string(), *b))
        .collect();
    assert_eq!(got, want);
    assert_eq!(diag(&doc), json!([{ "componentId": "s-e", "message": SUBREPORT_DEPTH }]));
    let looped = lay_with(&level("a", Some("b")), &data, &chain(Some("a")));
    assert_eq!(diag(&looped), json!([{ "componentId": "s-a", "message": SUBREPORT_LOOP }]));
}

#[test]
fn reports_missing_data_extra_subreports_and_subreports_in_page_bands() {
    let mut other = sub("other", 0.0, default_links());
    other["id"] = json!("extra");
    let mut detailed = invoice_report(50.0, vec![other]);
    detailed["bands"]["pageFooter"] = band(20.0, vec![sub("lines", 0.0, default_links())], false);
    let doc = lay_with(&detailed, &invoices()[..1], &LayoutOptions::default());
    assert_eq!(
        diag(&doc),
        json!([
            { "componentId": "extra", "message": SUBREPORT_EXTRA },
            { "componentId": "s-lines", "message": SUBREPORT_MISSING },
            { "componentId": "s-lines", "message": SUBREPORT_PAGE_BAND },
        ])
    );
}

fn comp(v: &Value) -> Comp {
    let c: reports::Component = serde_json::from_value(v.clone()).unwrap();
    Comp::from_stored(&c).unwrap()
}

#[test]
fn splits_a_band_around_the_subreport_and_filters_linked_rows() {
    let b = band(
        60.0,
        vec![
            fld("a", "1", 0.0, 0.0),
            fld("beside", "2", 20.0, 300.0),
            sub("x", 14.0, default_links()),
            fld("z", "3", 40.0, 0.0),
        ],
        false,
    );
    let stored: reports::Band = serde_json::from_value(b).unwrap();
    let parsed = Band::from_stored(&stored);
    let s = comp(&sub("x", 14.0, default_links()));
    let (head, tail) = split_at_subreport(&parsed, &s);
    assert_eq!(head.height, 14.0);
    assert_eq!(head.components.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["a", "beside"]);
    assert_eq!(tail.height, 26.0);
    assert_eq!(tail.components.iter().map(|c| (c.id.as_str(), c.y)).collect::<Vec<_>>(), [("z", 6.0)]);
    let Kind::Subreport { links, .. } = comp(&sub("x", 14.0, default_links())).kind else {
        panic!("subreport")
    };
    let links = &links;
    let data = lines();
    assert_eq!(linked_rows(links, &data, &json!({ "id": 2 })), vec![data[2].clone()]);
    assert!(linked_rows(links, &data, &json!({ "id": null })).is_empty());
    assert_eq!(linked_rows(&[], &data, &Value::Null), data);
}

fn level(id: &str, child: Option<&str>) -> reports::Report {
    let comps = child.map(|c| vec![sub(c, 0.0, default_links())]).unwrap_or_default();
    report(&named(id, vec![("detail", band(30.0, comps, false))]))
}

#[test]
fn measures_depth_and_offers_only_reports_that_keep_three_levels_without_loops() {
    let reports = vec![
        level("a", Some("b")),
        level("b", Some("c")),
        level("c", None),
        level("d", None),
        level("e", Some("d")),
    ];
    assert_eq!(subreport_depth(&reports, "a", &mut vec![]), Some(2));
    assert_eq!(subreport_depth(&[level("x", Some("y")), level("y", Some("x"))], "x", &mut vec![]), None);
    let ids = |list: Vec<&reports::Report>| list.iter().map(|r| r.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(embeddable_reports(&reports, &reports[2])), ["d"]);
    assert_eq!(ids(embeddable_reports(&reports, &reports[0])), ["b", "c", "d", "e"]);
    assert_eq!(ids(embeddable_reports(&reports, &reports[3])), ["b", "c"]);
}

#[test]
fn lists_nesting_and_placement_problems() {
    let mut s2 = sub("zzz", 0.0, default_links());
    s2["id"] = json!("s2");
    let mut s3 = sub("b", 0.0, default_links());
    s3["id"] = json!("s3");
    let bad_json = named(
        "a",
        vec![
            ("detail", band(30.0, vec![sub("b", 0.0, default_links()), s2], false)),
            ("pageHeader", band(20.0, vec![s3], false)),
        ],
    );
    let bad = report(&bad_json);
    let problems = report_problems(&bad, &[bad.clone(), level("b", Some("a"))]);
    assert_eq!(
        problems,
        [
            "its subreports print each other in a loop",
            "Page header component s3: subreports are not supported in page headers or footers",
            "Detail has more than one subreport",
            "Detail component s2: the subreport's report does not exist",
        ]
    );
}
