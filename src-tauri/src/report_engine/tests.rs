//! Port of `tests/unit/report-engine.test.ts`.

use super::fixtures::{
    band, base_json, calc, doc_json, field, lay, lay_with, norm, orders, params, rows, static_text, t, texts,
};
use super::layout::compare_keys;
use super::text::{measure_text, normalize_text, wrap_text};
use super::{AssetInfo, Item, LayoutOptions};
use serde_json::{json, Value};

#[test]
fn measures_with_the_helvetica_afm_widths() {
    assert_eq!(measure_text("Acme", 10.0, false), 25.56);
    assert_eq!(measure_text("Acme", 10.0, true), 27.23);
    assert_eq!(measure_text("€", 10.0, false), 5.56);
}

#[test]
fn normalizes_to_what_the_standard_and_bundled_fonts_can_print() {
    assert_eq!(
        normalize_text("a\tb\r\nc\u{1} € “q” ✓ 日 א"),
        "a b\nc € “q” ✓ 日 ?"
    );
}

#[test]
fn wraps_greedily_on_spaces_keeps_explicit_breaks_and_splits_long_words() {
    assert_eq!(wrap_text("The quick brown fox", 50.0, 10.0, false), ["The quick", "brown fox"]);
    assert_eq!(wrap_text("one\n\ntwo", 100.0, 10.0, false), ["one", "", "two"]);
    assert_eq!(wrap_text("WWWWWWWW", 30.0, 10.0, false), ["WWW", "WWW", "WW"]);
}

#[test]
fn orders_group_keys_totally_and_deterministically() {
    let mut keys = vec![json!("b"), Value::Null, json!(2), json!("B"), json!(true), json!(10), json!("a")];
    keys.sort_by(compare_keys);
    assert_eq!(
        keys,
        vec![Value::Null, json!(true), json!(2), json!(10), json!("B"), json!("a"), json!("b")]
    );
}

fn text_item(id: &str, x: f64, y: f64, w: f64, h: f64) -> Value {
    json!({ "kind": "text", "componentId": id, "x": x, "y": y, "w": w, "h": h,
        "fontSize": 10, "bold": false, "gray": 0 })
}

fn merged(mut base: Value, extra: Value) -> Value {
    for (k, v) in extra.as_object().unwrap() {
        base[k] = v.clone();
    }
    base
}

#[test]
fn paginates_detail_bands_and_resolves_page_pages_in_a_second_pass() {
    let report = base_json(vec![
        (
            "reportHeader",
            band(
                40.0,
                vec![json!({ "id": "title", "kind": "staticText", "text": "Sales report", "x": 0, "y": 0,
                    "w": 300, "h": 20, "style": { "fontSize": 16, "bold": true } })],
                false,
            ),
        ),
        ("detail", band(300.0, vec![field("name", "record.customer", 0.0, 0.0, 100.0, 14.0)], false)),
        (
            "pageFooter",
            band(
                20.0,
                vec![json!({ "id": "pg", "kind": "calculated",
                    "expression": "'Page ' & page & ' of ' & pages", "x": 0, "y": 0, "w": 523, "h": 14,
                    "style": { "align": "right" } })],
                false,
            ),
        ),
    ]);
    let doc = lay(&report, &orders()[..3]);
    let title = merged(
        text_item("title", 36.0, 36.0, 300.0, 20.0),
        json!({ "fontSize": 16, "bold": true, "text": "Sales report",
            "lines": [{ "text": "Sales report", "x": 38, "y": 49.69, "width": 92.48 }] }),
    );
    let one = |id: &str, y: f64, text: &str, lx: f64, ly: f64, w: f64, bx: f64, bw: f64| {
        merged(
            text_item(id, bx, y, bw, 14.0),
            json!({ "text": text, "lines": [{ "text": text, "x": lx, "y": ly, "width": w }] }),
        )
    };
    let expected = json!({
        "width": 595, "height": 842, "diagnostics": [],
        "pages": [
            { "number": 1, "items": [
                title,
                one("name", 76.0, "Acme", 38.0, 84.56, 25.56, 36.0, 100.0),
                one("name", 376.0, "Birch", 38.0, 384.56, 22.78, 36.0, 100.0),
                one("pg", 786.0, "Page 1 of 2", 505.85, 794.56, 51.15, 36.0, 523.0),
            ]},
            { "number": 2, "items": [
                one("name", 36.0, "Cobalt", 38.0, 44.56, 28.9, 36.0, 100.0),
                one("pg", 786.0, "Page 2 of 2", 505.85, 794.56, 51.15, 36.0, 523.0),
            ]},
        ]
    });
    assert_eq!(doc_json(&doc), norm(expected));
}

fn grouped(keep_together: bool) -> Value {
    base_json(vec![
        (
            "groups",
            json!([{
                "id": "g1",
                "groupBy": "record.region",
                "header": band(20.0, vec![json!({ "id": "gh", "kind": "field",
                    "expression": "record.region & ' (' & count(rows) & ')'",
                    "x": 0, "y": 0, "w": 200, "h": 14, "style": { "bold": true } })], keep_together),
                "footer": band(20.0, vec![json!({ "id": "gf", "kind": "calculated",
                    "expression": "sum(rows.amount)", "format": "#,##0.00",
                    "x": 400, "y": 0, "w": 100, "h": 14 })], false),
            }]),
        ),
        (
            "detail",
            band(
                16.0,
                vec![
                    field("c", "record.customer", 10.0, 0.0, 100.0, 14.0),
                    json!({ "id": "a", "kind": "field", "expression": "record.amount",
                        "format": "#,##0.00", "x": 400, "y": 0, "w": 100, "h": 14 }),
                ],
                false,
            ),
        ),
        (
            "reportFooter",
            band(
                40.0,
                vec![
                    json!({ "id": "total", "kind": "calculated", "expression": "sum(rows.amount)",
                        "format": "$#,##0.00", "x": 400, "y": 0, "w": 100, "h": 14 }),
                    calc("sumof", "sumof(rows, qty * price)", 400.0, 20.0, 100.0, 14.0),
                    calc("cnt", "count(rows)", 0.0, 0.0, 100.0, 14.0),
                ],
                false,
            ),
        ),
    ])
}

#[test]
fn groups_rows_stable_sorted_by_key_with_group_scoped_totals() {
    let doc = lay(&grouped(false), &orders());
    let expected = vec![vec![
        t("gh", "East (2)", 36.0),
        t("c", "Birch", 56.0),
        t("a", "80.00", 56.0),
        t("c", "Dune", 72.0),
        t("a", "10.00", 72.0),
        t("gf", "90.00", 88.0),
        t("gh", "North (1)", 108.0),
        t("c", "Elm", 128.0),
        t("a", "300.00", 128.0),
        t("gf", "300.00", 144.0),
        t("gh", "West (2)", 164.0),
        t("c", "Acme", 184.0),
        t("a", "120.50", 184.0),
        t("c", "Cobalt", 200.0),
        t("a", "42.25", 200.0),
        t("gf", "162.75", 216.0),
        t("total", "$552.75", 236.0),
        t("sumof", "425", 256.0),
        t("cnt", "5", 236.0),
    ]];
    assert_eq!(texts(&doc), expected);
}

#[test]
fn sorts_groups_descending_and_keeps_input_order_inside_a_group() {
    let mut report = grouped(false);
    report["bands"]["groups"][0]["descending"] = json!(true);
    let mut reversed = orders();
    reversed.reverse();
    let headers: Vec<String> = texts(&lay(&report, &reversed))[0]
        .iter()
        .filter(|(id, _, _)| id == "gh" || id == "c")
        .map(|(_, text, _)| text.clone())
        .collect();
    assert_eq!(
        headers,
        ["West (2)", "Cobalt", "Acme", "North (1)", "Elm", "East (2)", "Dune", "Birch"]
    );
}

#[test]
fn breaks_pages_between_groups_and_never_orphans_a_keep_together_group_header() {
    let mut loose = grouped(false);
    loose["bands"]["reportHeader"] = band(676.0, vec![], false);
    let mut tight = grouped(true);
    tight["bands"]["reportHeader"] = band(676.0, vec![], false);
    let page = |r: &Value, n: usize| texts(&lay(r, &orders()))[n].clone();
    assert_eq!(page(&loose, 0).last().unwrap(), &t("gh", "North (1)", 784.0));
    assert_eq!(page(&loose, 1)[0], t("c", "Elm", 36.0));
    assert_eq!(page(&tight, 0).last().unwrap(), &t("gf", "90.00", 764.0));
    assert_eq!(page(&tight, 1)[0], t("gh", "North (1)", 36.0));
}

fn table_report(keep_together: bool) -> Value {
    let table = json!({
        "id": "t", "kind": "table", "x": 0, "y": 20, "w": 300, "h": 20,
        "columns": [
            { "id": "c1", "header": "Customer", "expression": "record.customer", "width": 2 },
            { "id": "c2", "header": "Amount", "expression": "record.amount", "width": 1,
              "align": "right", "format": "0.00" }
        ]
    });
    base_json(vec![
        ("pageHeader", band(20.0, vec![static_text("ph", "Header", 0.0, 0.0, 100.0, 14.0)], false)),
        (
            "reportFooter",
            band(
                60.0,
                vec![
                    static_text("before", "Before", 0.0, 0.0, 100.0, 14.0),
                    table,
                    static_text("after", "After", 0.0, 44.0, 100.0, 14.0),
                ],
                keep_together,
            ),
        ),
    ])
}

fn many() -> Vec<super::Row> {
    rows(Value::Array(
        (0..60).map(|i| json!({ "customer": format!("C{}", i + 1), "amount": i })).collect(),
    ))
}

#[test]
fn splits_a_table_across_pages_and_repeats_its_header_row() {
    let doc = lay(&table_report(false), &many());
    let pages = texts(&doc);
    assert_eq!(pages.len(), 2);
    assert_eq!(
        pages[0][..6],
        [
            t("ph", "Header", 36.0),
            t("before", "Before", 56.0),
            t("t", "Customer", 76.0),
            t("t", "Amount", 76.0),
            t("t", "C1", 90.8),
            t("t", "0.00", 90.8),
        ]
    );
    assert_eq!(pages[0][pages[0].len() - 2], t("t", "C48", 786.4));
    assert_eq!(
        pages[1][..5],
        [
            t("ph", "Header", 36.0),
            t("t", "Customer", 56.0),
            t("t", "Amount", 56.0),
            t("t", "C49", 70.8),
            t("t", "48.00", 70.8),
        ]
    );
    assert_eq!(pages[1].last().unwrap(), &t("after", "After", 252.4));
    let json = doc_json(&doc);
    let rects: Vec<&Value> = json["pages"][1]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "rect")
        .collect();
    let rect = |x: f64, w: f64| {
        norm(json!({ "kind": "rect", "componentId": "t", "x": x, "y": 56, "w": w, "h": 14.8,
            "lineWidth": 0.5, "gray": 0, "fill": 0.9 }))
    };
    assert_eq!(rects[0], &rect(36.0, 200.0));
    assert_eq!(rects[1], &rect(236.0, 100.0));
    let amount = doc.pages[1]
        .items
        .iter()
        .find_map(|i| match i {
            Item::Text(t) if t.text == "48.00" => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        serde_json::to_value(&amount.lines[0]).unwrap(),
        json!({ "text": "48.00", "x": 310.48, "y": 80.5, "width": 22.52 })
    );
}

#[test]
fn moves_a_short_keep_together_table_band_to_the_next_page_instead_of_splitting_it() {
    let mut report = table_report(true);
    report["bands"]["reportHeader"] = band(600.0, vec![], false);
    let pages = texts(&lay(&report, &many()[..10]));
    assert_eq!(pages[0], [t("ph", "Header", 36.0)]);
    assert_eq!(pages[1][1], t("before", "Before", 56.0));
    assert_eq!(pages[1].last().unwrap(), &t("after", "After", 242.8));
}

#[test]
fn grows_the_band_and_shifts_components_below_an_unsplit_table() {
    let doc = lay(&table_report(false), &many()[..3]);
    assert_eq!(texts(&doc)[0].last().unwrap(), &t("after", "After", 139.2));
}

#[test]
fn reads_table_rows_from_saved_query_data_when_the_table_has_a_query_id() {
    let mut report = table_report(false);
    report["bands"]["reportFooter"]["components"][1]["queryId"] = json!("q1");
    let mut options = LayoutOptions::default();
    options.tables.insert("q1".into(), rows(json!([{ "customer": "Only", "amount": 1 }])));
    let doc = lay_with(&report, &many(), &options);
    let cells: Vec<String> = texts(&doc)[0]
        .iter()
        .filter(|(id, _, _)| id == "t")
        .map(|(_, text, _)| text.clone())
        .collect();
    assert_eq!(cells, ["Customer", "Amount", "Only", "1.00"]);
    let missing = lay(&report, &many());
    assert_eq!(
        serde_json::to_value(&missing.diagnostics).unwrap(),
        json!([{ "componentId": "t", "message": "Table query data is not loaded" }])
    );
}

#[test]
fn renders_shapes_images_and_placeholders_and_reports_expression_errors() {
    let report = base_json(vec![(
        "detail",
        band(
            60.0,
            vec![
                json!({ "id": "ln", "kind": "line", "orientation": "horizontal", "x": 0, "y": 0, "w": 100,
                    "h": 4, "style": { "borderWidth": 2 } }),
                json!({ "id": "rc", "kind": "rectangle", "x": 0, "y": 10, "w": 50, "h": 20,
                    "style": { "fill": 0.8, "borderWidth": 0 } }),
                json!({ "id": "im", "kind": "image", "assetId": "logo", "x": 60, "y": 10, "w": 40, "h": 40 }),
                json!({ "id": "gone", "kind": "image", "assetId": "nope", "x": 110, "y": 10, "w": 40, "h": 40 }),
                field("bad", "record.customer * 2", 200.0, 0.0, 80.0, 14.0),
                json!({ "id": "fmt", "kind": "field", "expression": "params.when", "format": "MMM d, yyyy",
                    "x": 300, "y": 0, "w": 80, "h": 14 }),
            ],
            false,
        ),
    )]);
    let mut options = LayoutOptions {
        params: params(json!({ "when": "2026-01-05" })),
        ..LayoutOptions::default()
    };
    options.assets.insert("logo".into(), AssetInfo { media_type: "image/jpeg".into() });
    let doc = lay_with(&report, &orders()[..1], &options);
    let json = doc_json(&doc);
    let items = json["pages"][0]["items"].as_array().unwrap();
    assert_eq!(
        items[..3],
        [
            norm(json!({ "kind": "line", "componentId": "ln", "x": 36, "y": 38, "w": 100, "h": 0,
                "lineWidth": 2, "gray": 0 })),
            norm(json!({ "kind": "rect", "componentId": "rc", "x": 36, "y": 46, "w": 50, "h": 20,
                "lineWidth": 0, "gray": 0, "fill": 0.8 })),
            norm(json!({ "kind": "image", "componentId": "im", "x": 96, "y": 46, "w": 40, "h": 40,
                "assetId": "logo", "mediaType": "image/jpeg" })),
        ]
    );
    assert_eq!(
        texts(&doc)[0],
        [
            t("gone", "Missing image", 46.0),
            t("bad", "#Error", 36.0),
            t("fmt", "Jan 5, 2026", 36.0)
        ]
    );
    let ids: Vec<&str> = doc.diagnostics.iter().map(|d| d.component_id.as_str()).collect();
    assert_eq!(ids, ["gone", "bad"]);
}

#[test]
fn is_deterministic_and_always_returns_at_least_one_page() {
    let report = grouped(true);
    assert_eq!(lay(&report, &orders()), lay(&report, &orders()));
    let empty = lay(&base_json(vec![]), &[]);
    assert_eq!(empty.pages.len(), 1);
    assert_eq!(empty.pages[0].number, 1);
    assert!(empty.pages[0].items.is_empty());
}

#[test]
fn uses_landscape_letter_dimensions_and_margins() {
    let mut report = base_json(vec![(
        "detail",
        band(14.0, vec![field("n", "record.id", 0.0, 0.0, 50.0, 14.0)], false),
    )]);
    report["page"] = json!({ "size": "Letter", "orientation": "landscape",
        "margins": { "top": 10, "right": 20, "bottom": 30, "left": 40 } });
    let doc = lay(&report, &orders()[..1]);
    assert_eq!((doc.width, doc.height), (792.0, 612.0));
    assert_eq!(texts(&doc)[0], [t("n", "1", 10.0)]);
    assert_eq!(doc.pages[0].items[0].x(), 40.0);
}
