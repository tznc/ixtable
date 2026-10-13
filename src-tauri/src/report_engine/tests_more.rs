//! Port of `report-pagination.test.ts`.

use super::fixtures::{band, base_json, field, lay, orders, rows, t, texts};
use super::layout::PAGE_BAND_TABLE;
use super::Row;
use serde_json::{json, Value};

const PAGE_NUMBERS: &str = "groupPage & '/' & groupPages & ' ' & page & '/' & pages";

fn wide(id: &str, expression: &str) -> Value {
    field(id, expression, 0.0, 0.0, 300.0, 14.0)
}

fn by_region(patch: Value, height: f64) -> Value {
    let mut group = json!({
        "id": "g1",
        "groupBy": "record.region",
        "header": band(height, vec![wide("gh", "record.region")], false),
        "footer": band(0.0, vec![], false),
    });
    for (k, v) in patch.as_object().unwrap() {
        group[k] = v.clone();
    }
    group
}

fn flagged(mut b: Value, flags: Value) -> Value {
    for (k, v) in flags.as_object().unwrap() {
        b[k] = v.clone();
    }
    b
}

fn pages(doc: &super::ReportDocument) -> Vec<Vec<(String, String, f64)>> {
    texts(doc)
}

fn expect(doc: &super::ReportDocument, expected: &[&[(&str, &str, f64)]]) {
    let want: Vec<Vec<(String, String, f64)>> = expected
        .iter()
        .map(|p| p.iter().map(|(a, b, c)| t(a, b, *c)).collect())
        .collect();
    assert_eq!(pages(doc), want);
}

fn table_of(id: &str, y: f64, h: f64) -> Value {
    json!({ "id": id, "kind": "table", "x": 0, "y": y, "w": 300, "h": h,
        "columns": [{ "id": "c", "header": "Customer", "expression": "record.customer", "width": 300 }] })
}

fn west(n: usize) -> Vec<Row> {
    rows(Value::Array(
        (0..n).map(|i| json!({ "region": "West", "customer": format!("C{i}") })).collect(),
    ))
}

#[test]
fn defaults_keep_the_original_layout_and_group_page_follows_page() {
    let doc = lay(
        &base_json(vec![
            ("detail", band(300.0, vec![wide("d", "record.customer")], false)),
            ("pageFooter", band(20.0, vec![wide("pf", PAGE_NUMBERS)], false)),
        ]),
        &orders(),
    );
    let footers: Vec<String> = pages(&doc)
        .iter()
        .map(|p| p.iter().find(|x| x.0 == "pf").unwrap().1.clone())
        .collect();
    assert_eq!(footers, ["1/3 1/3", "2/3 2/3", "3/3 3/3"]);
}

#[test]
fn breaks_before_a_band_but_never_leaves_the_first_page_blank() {
    let detail = flagged(band(20.0, vec![wide("d", "record.customer")], false), json!({ "pageBreakBefore": true }));
    let doc = lay(&base_json(vec![("detail", detail)]), &orders()[..3]);
    expect(
        &doc,
        &[&[("d", "Acme", 36.0)], &[("d", "Birch", 36.0)], &[("d", "Cobalt", 36.0)]],
    );
}

#[test]
fn breaks_after_a_band_without_a_trailing_blank_page() {
    let doc = lay(
        &base_json(vec![
            (
                "reportHeader",
                flagged(band(40.0, vec![wide("t", "'Title'")], false), json!({ "pageBreakAfter": true })),
            ),
            ("detail", band(20.0, vec![wide("d", "record.customer")], false)),
            (
                "reportFooter",
                flagged(band(20.0, vec![wide("f", "'End'")], false), json!({ "pageBreakAfter": true })),
            ),
        ]),
        &orders()[..2],
    );
    expect(
        &doc,
        &[
            &[("t", "Title", 36.0)],
            &[("d", "Acme", 36.0), ("d", "Birch", 56.0), ("f", "End", 76.0)],
        ],
    );
}

#[test]
fn starts_every_group_on_a_new_page() {
    let doc = lay(
        &base_json(vec![
            ("groups", json!([by_region(json!({ "newPage": true }), 30.0)])),
            ("detail", band(20.0, vec![wide("d", "record.customer")], false)),
        ]),
        &orders(),
    );
    expect(
        &doc,
        &[
            &[("gh", "East", 36.0), ("d", "Birch", 66.0), ("d", "Dune", 86.0)],
            &[("gh", "North", 36.0), ("d", "Elm", 66.0)],
            &[("gh", "West", 36.0), ("d", "Acme", 66.0), ("d", "Cobalt", 86.0)],
        ],
    );
}

#[test]
fn repeats_a_group_header_on_continuation_pages_only_when_asked() {
    let report = |repeat: bool| {
        base_json(vec![
            ("groups", json!([by_region(json!({ "repeatHeader": repeat }), 30.0)])),
            ("detail", band(300.0, vec![wide("d", "record.customer")], false)),
        ])
    };
    expect(
        &lay(&report(true), &orders()),
        &[
            &[("gh", "East", 36.0), ("d", "Birch", 66.0), ("d", "Dune", 366.0), ("gh", "North", 666.0)],
            &[("gh", "North", 36.0), ("d", "Elm", 66.0), ("gh", "West", 366.0), ("d", "Acme", 396.0)],
            &[("gh", "West", 36.0), ("d", "Cobalt", 66.0)],
        ],
    );
    expect(
        &lay(&report(false), &orders()),
        &[
            &[("gh", "East", 36.0), ("d", "Birch", 66.0), ("d", "Dune", 366.0), ("gh", "North", 666.0)],
            &[("d", "Elm", 36.0), ("gh", "West", 336.0), ("d", "Acme", 366.0)],
            &[("d", "Cobalt", 36.0)],
        ],
    );
}

#[test]
fn repeats_the_group_header_above_each_continuation_of_a_split_table() {
    let mut group = by_region(json!({ "repeatHeader": true }), 30.0);
    group["footer"] = band(40.0, vec![table_of("tbl", 0.0, 40.0)], false);
    let doc = lay(&base_json(vec![("groups", json!([group]))]), &west(80));
    assert!(doc.pages.len() > 1);
    for page in pages(&doc).iter().skip(1) {
        assert_eq!(page[0], t("gh", "West", 36.0));
        assert_eq!((page[1].1.as_str(), page[1].2), ("Customer", 66.0));
    }
}

#[test]
fn keeps_a_block_that_fits_only_without_repeated_headers_out_of_the_footer() {
    let data: Vec<Row> = orders()
        .into_iter()
        .filter(|o| o["region"] == "West" || o["region"] == "East")
        .collect();
    let doc = lay(
        &base_json(vec![
            ("groups", json!([by_region(json!({ "repeatHeader": true }), 40.0)])),
            ("detail", band(730.0, vec![wide("d", "record.customer")], false)),
            ("pageFooter", band(20.0, vec![wide("pf", "page")], false)),
        ]),
        &data,
    );
    let footer_top = 842.0 - 36.0 - 20.0;
    for page in &doc.pages {
        for item in &page.items {
            if let super::Item::Text(text) = item {
                if text.component_id == "pf" {
                    continue;
                }
                assert!(text.y < footer_top);
                if text.component_id == "d" {
                    assert!(text.y + 730.0 <= footer_top);
                }
            }
        }
    }
    let details: Vec<Vec<String>> = pages(&doc)
        .iter()
        .map(|p| p.iter().filter(|x| x.0 == "d").map(|x| x.1.clone()).collect())
        .collect();
    let want: Vec<Vec<&str>> = vec![vec![], vec!["Birch"], vec!["Dune"], vec![], vec!["Acme"], vec!["Cobalt"]];
    assert_eq!(details, want);
}

#[test]
fn splits_a_keep_together_table_that_fits_only_without_repeated_headers() {
    let mut group = by_region(json!({ "repeatHeader": true }), 40.0);
    group["footer"] = band(20.0, vec![table_of("tbl", 0.0, 20.0)], true);
    let doc = lay(&base_json(vec![("groups", json!([group]))]), &west(40));
    for page in &doc.pages {
        for item in &page.items {
            assert!(item.y() + item.h() <= 842.0 - 36.0 + 1e-6);
        }
    }
}

#[test]
fn repeats_only_the_non_table_items_of_a_group_header() {
    let mut group = by_region(json!({ "repeatHeader": true }), 40.0);
    group["header"] = band(40.0, vec![wide("gh", "record.region"), table_of("htbl", 20.0, 20.0)], false);
    let doc = lay(
        &base_json(vec![
            ("groups", json!([group])),
            ("detail", band(20.0, vec![wide("d", "record.customer")], false)),
        ]),
        &west(120),
    );
    assert!(doc.pages.len() > 2);
    let all = pages(&doc);
    let detail_pages: Vec<&Vec<(String, String, f64)>> = all.iter().filter(|p| p.iter().any(|x| x.0 == "d")).collect();
    assert!(detail_pages.len() > 1);
    for page in &detail_pages[1..] {
        assert_eq!(page[0], t("gh", "West", 36.0));
        assert_eq!((page[1].0.as_str(), page[1].2), ("d", 76.0));
        assert!(!page.iter().any(|x| x.0 == "htbl"));
    }
}

#[test]
fn restarts_group_page_and_group_pages_per_group_keeping_page_and_pages() {
    let doc = lay(
        &base_json(vec![
            ("groups", json!([by_region(json!({ "resetPageNumber": true }), 20.0)])),
            ("detail", band(400.0, vec![wide("d", "record.customer")], false)),
            ("pageFooter", band(20.0, vec![wide("pf", PAGE_NUMBERS)], false)),
        ]),
        &orders(),
    );
    let got: Vec<(String, String)> = pages(&doc)
        .iter()
        .map(|p| (p[0].1.clone(), p.iter().find(|x| x.0 == "pf").unwrap().1.clone()))
        .collect();
    let want: Vec<(String, String)> = [
        ("East", "1/2 1/5"),
        ("Dune", "2/2 2/5"),
        ("North", "1/1 3/5"),
        ("West", "1/2 4/5"),
        ("Cobalt", "2/2 5/5"),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    assert_eq!(got, want);
}

#[test]
fn honours_page_breaks_and_section_starts_on_empty_group_headers() {
    let mut group = by_region(json!({ "resetPageNumber": true }), 0.0);
    group["header"] = band(0.0, vec![], false);
    let doc = lay(
        &base_json(vec![
            ("groups", json!([group])),
            ("detail", band(20.0, vec![wide("d", "record.customer")], false)),
            ("pageFooter", band(20.0, vec![wide("pf", PAGE_NUMBERS)], false)),
        ]),
        &orders(),
    );
    let got: Vec<Vec<String>> = pages(&doc).iter().map(|p| p.iter().map(|x| x.1.clone()).collect()).collect();
    let want: Vec<Vec<&str>> = vec![
        vec!["Birch", "Dune", "1/1 1/3"],
        vec!["Elm", "1/1 2/3"],
        vec!["Acme", "Cobalt", "1/1 3/3"],
    ];
    assert_eq!(got, want);
}

#[test]
fn reports_a_table_in_a_page_band_instead_of_dropping_it_silently() {
    let table = json!({ "id": "ptbl", "kind": "table", "x": 0, "y": 0, "w": 100, "h": 20, "columns": [] });
    let doc = lay(&base_json(vec![("pageHeader", band(20.0, vec![table], false))]), &[]);
    assert!(doc
        .diagnostics
        .iter()
        .any(|d| d.component_id == "ptbl" && d.message == PAGE_BAND_TABLE));
}
