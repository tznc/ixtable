//! Ports of `report-grow.test.ts` and `report-grow-split.test.ts`.

use super::fixtures::{band, base_json, lay, of, rows, static_text, text_of};
use super::grow::GROW_WITH_TABLE;
use super::split::{cut_at, slice_items};
use super::text::LINE_HEIGHT;
use super::{ImageItem, Item, RectItem, Row, TextItem, TextLine};
use serde_json::{json, Value};

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 0.005, "{a} != {b}");
}

fn long() -> String {
    "lorem ipsum dolor sit amet ".repeat(8).trim().to_string()
}

fn note_rows(values: &[String]) -> Vec<Row> {
    rows(Value::Array(values.iter().map(|n| json!({ "note": n })).collect()))
}

fn note(patch: Value) -> Value {
    let mut c = json!({ "id": "note", "kind": "field", "expression": "record.note", "x": 0, "y": 0,
        "w": 100, "h": 14, "canGrow": true, "style": { "borderWidth": 1 } });
    for (k, v) in patch.as_object().unwrap() {
        if v.is_null() {
            c.as_object_mut().unwrap().remove(k);
        } else {
            c[k] = v.clone();
        }
    }
    c
}

fn below() -> Value {
    static_text("below", "after", 150.0, 20.0, 100.0, 14.0)
}

fn beside() -> Value {
    static_text("beside", "beside", 150.0, 0.0, 100.0, 14.0)
}

#[test]
fn grows_the_box_and_band_and_pushes_components_below_it_down() {
    let report = base_json(vec![("detail", band(40.0, vec![note(json!({})), beside(), below()], false))]);
    let doc = lay(&report, &note_rows(&[long(), "short".into()]));
    let texts = text_of(&doc, "note");
    let (text, second) = (&texts[0].1, &texts[1].1);
    let lines = text.lines.len();
    assert!(lines > 5);
    let grow = lines as f64 * 10.0 * LINE_HEIGHT - 14.0;
    close(text.h, 14.0 + grow);
    let frame = of(&doc, "note").into_iter().find_map(|(_, i)| match i {
        Item::Rect(r) => Some(r),
        _ => None,
    });
    close(frame.unwrap().h, 14.0 + grow);
    assert_eq!(of(&doc, "beside")[0].1.y(), 36.0);
    close(of(&doc, "below")[0].1.y(), 36.0 + 20.0 + grow);
    close(second.y, 36.0 + 40.0 + grow);
    assert_eq!(of(&doc, "below")[1].1.y(), 36.0 + 40.0 + grow + 20.0);
}

#[test]
fn chains_growth_through_stacked_boxes_and_keeps_text_that_fits_unchanged() {
    let second = note(json!({ "id": "second", "y": 20 }));
    let mut moved = below();
    moved["y"] = json!(34);
    let report = base_json(vec![("detail", band(48.0, vec![note(json!({})), second, moved], false))]);
    let doc = lay(&report, &note_rows(&[long()]));
    let first = &text_of(&doc, "note")[0].1;
    let next = &text_of(&doc, "second")[0].1;
    let grow = first.h - 14.0;
    close(next.y, 36.0 + 20.0 + grow);
    close(next.h, first.h);
    close(of(&doc, "below")[0].1.y(), 36.0 + 34.0 + 2.0 * grow);
    let fits = lay(
        &base_json(vec![("detail", band(40.0, vec![note(json!({})), below()], false))]),
        &note_rows(&["x".into()]),
    );
    let plain = lay(
        &base_json(vec![("detail", band(40.0, vec![note(json!({ "canGrow": null })), below()], false))]),
        &note_rows(&["x".into()]),
    );
    assert_eq!(fits, plain);
}

#[test]
fn clips_text_without_can_grow_as_before() {
    let report = base_json(vec![("detail", band(40.0, vec![note(json!({ "canGrow": null }))], false))]);
    let doc = lay(&report, &note_rows(&[long()]));
    let text = &text_of(&doc, "note")[0].1;
    assert_eq!(text.lines.len(), 1);
    assert_eq!(text.h, 14.0);
}

#[test]
fn paginates_by_the_grown_height() {
    let values: Vec<String> = (0..20).map(|i| if i == 0 { long() } else { "x".into() }).collect();
    let data = note_rows(&values);
    let fixed = lay(
        &base_json(vec![("detail", band(38.0, vec![note(json!({ "canGrow": null }))], false))]),
        &data,
    );
    assert_eq!(fixed.pages.len(), 1);
    let report = base_json(vec![("detail", band(38.0, vec![note(json!({}))], false))]);
    let doc = lay(&report, &data);
    assert_eq!(doc.pages.len(), 2);
    let texts = text_of(&doc, "note");
    assert_eq!(texts.last().unwrap().0, 1);
    assert_eq!(texts.iter().find(|(p, _)| *p == 1).unwrap().1.y, 36.0);
    assert_eq!(lay(&report, &data), doc);
}

#[test]
fn repeats_a_grown_group_header_at_its_grown_height() {
    let header = band(20.0, vec![note(json!({ "id": "gh", "expression": "record.note" }))], false);
    let mut under = below();
    under["y"] = json!(0);
    let report = base_json(vec![
        (
            "groups",
            json!([{ "id": "g", "groupBy": "1", "header": header, "footer": band(0.0, vec![], false),
                "repeatHeader": true }]),
        ),
        ("detail", band(500.0, vec![under], false)),
    ]);
    let doc = lay(&report, &note_rows(&[long(), long(), long()]));
    let heads = text_of(&doc, "gh");
    assert_eq!(heads.iter().map(|h| h.0).collect::<Vec<_>>(), [0, 1, 2]);
    let grown_height = heads[0].1.h + 6.0;
    for page in [1, 2] {
        let y = of(&doc, "below").into_iter().find(|(p, _)| *p == page).unwrap().1.y();
        close(y, 36.0 + grown_height);
    }
}

#[test]
fn can_grow_is_ignored_in_a_band_with_a_table_with_a_diagnostic() {
    let table = json!({ "id": "tbl", "kind": "table", "x": 0, "y": 20, "w": 200, "h": 20,
        "columns": [{ "id": "c", "header": "Note", "expression": "record.note", "width": 1 }] });
    let report = base_json(vec![("detail", band(40.0, vec![note(json!({})), table], false))]);
    let doc = lay(&report, &note_rows(&[long()]));
    assert_eq!(text_of(&doc, "note")[0].1.h, 14.0);
    assert!(doc
        .diagnostics
        .iter()
        .any(|d| d.component_id == "note" && d.message == GROW_WITH_TABLE));
}

// Splitting grown text across pages.

const BODY_TOP: f64 = 36.0;
const BODY_BOTTOM: f64 = 842.0 - 36.0 - 20.0;

fn words() -> Vec<String> {
    (0..600).map(|i| format!("w{i}")).collect()
}

fn memo() -> Value {
    json!({ "id": "memo", "kind": "field", "expression": "record.memo", "x": 0, "y": 0, "w": 100,
        "h": 14, "canGrow": true, "style": { "borderWidth": 1 } })
}

fn after() -> Value {
    static_text("after", "after", 0.0, 20.0, 100.0, 14.0)
}

fn page_footer() -> Value {
    band(
        20.0,
        vec![json!({ "id": "pf", "kind": "calculated", "expression": "'Page ' & page", "x": 0, "y": 0,
            "w": 100, "h": 14 })],
        false,
    )
}

fn memo_rows(values: &[String]) -> Vec<Row> {
    rows(Value::Array(values.iter().map(|n| json!({ "memo": n })).collect()))
}

#[test]
fn continues_a_band_taller_than_a_page_on_the_next_pages_without_losing_lines() {
    let report = base_json(vec![
        ("detail", band(40.0, vec![memo(), beside(), after()], false)),
        ("pageFooter", page_footer()),
    ]);
    let doc = lay(&report, &memo_rows(&[words().join(" ")]));
    assert!(doc.pages.len() >= 3);
    let pieces = text_of(&doc, "memo");
    assert_eq!(pieces.iter().map(|p| p.0).collect::<Vec<_>>(), (0..doc.pages.len()).collect::<Vec<_>>());
    let printed: Vec<&str> = pieces
        .iter()
        .flat_map(|p| p.1.lines.iter().map(|l| l.text.as_str()))
        .collect();
    assert_eq!(printed.join(" "), words().join(" "));
    for (_, piece) in &pieces {
        assert!(piece.y + piece.h <= BODY_BOTTOM + 0.01);
        for line in &piece.lines {
            assert!(line.y < BODY_BOTTOM);
        }
    }
    for (_, piece) in &pieces[1..] {
        assert_eq!(piece.y, BODY_TOP);
    }
    let frames: Vec<usize> = of(&doc, "memo")
        .into_iter()
        .filter(|(_, i)| matches!(i, Item::Rect(_)))
        .map(|(p, _)| p)
        .collect();
    assert_eq!(frames, pieces.iter().map(|p| p.0).collect::<Vec<_>>());
    let footers: Vec<String> = text_of(&doc, "pf").iter().map(|t| t.1.lines[0].text.clone()).collect();
    let expected: Vec<String> = (0..doc.pages.len()).map(|i| format!("Page {}", i + 1)).collect();
    assert_eq!(footers, expected);
    let last = pieces.last().unwrap();
    let below = &text_of(&doc, "after")[0];
    assert_eq!(below.0, last.0);
    close(below.1.y, last.1.y + last.1.h + 6.0);
}

#[test]
fn keeps_a_side_by_side_sibling_in_place_on_the_first_page() {
    let report = base_json(vec![("detail", band(40.0, vec![memo(), beside()], false))]);
    let doc = lay(&report, &memo_rows(&[words().join(" ")]));
    let siblings = text_of(&doc, "beside");
    assert_eq!(siblings.len(), 1);
    assert_eq!((siblings[0].0, siblings[0].1.y, siblings[0].1.h), (0, BODY_TOP, 14.0));
    let first = &text_of(&doc, "memo")[0];
    assert_eq!((first.0, first.1.y), (0, BODY_TOP));
}

#[test]
fn starts_a_grown_band_on_the_current_page_and_moves_it_whole_when_keep_together_fits() {
    let mut filler = after();
    filler["id"] = json!("filler");
    filler["y"] = json!(0);
    let short = words()[..40].join(" ");
    let data = memo_rows(&["x".into(), short.clone()]);
    let report = |keep: bool| {
        base_json(vec![
            ("reportHeader", band(650.0, vec![filler.clone()], false)),
            ("detail", band(40.0, vec![memo()], keep)),
        ])
    };
    let split = lay(&report(false), &data);
    assert_eq!(text_of(&split, "memo").iter().map(|t| t.0).collect::<Vec<_>>(), [0, 0, 1]);
    let kept = lay(&report(true), &data);
    let pieces = text_of(&kept, "memo");
    assert_eq!(pieces.iter().map(|t| t.0).collect::<Vec<_>>(), [0, 1]);
    assert_eq!(pieces[1].1.y, BODY_TOP);
    let lines: Vec<&str> = pieces[1].1.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(lines.join(" "), short);
}

#[test]
fn grown_split_layout_is_deterministic() {
    let report = base_json(vec![
        ("detail", band(40.0, vec![memo(), after()], false)),
        ("pageFooter", page_footer()),
    ]);
    let data = memo_rows(&[words().join(" ")]);
    assert_eq!(lay(&report, &data), lay(&report, &data));
}

fn text_block(y: f64, line_count: usize) -> Item {
    Item::Text(TextItem {
        component_id: "t".into(),
        x: 0.0,
        y,
        w: 100.0,
        h: line_count as f64 * 12.0,
        text: String::new(),
        lines: (0..line_count)
            .map(|i| TextLine {
                text: format!("l{i}"),
                x: 2.0,
                y: y + i as f64 * 12.0 + 8.555,
                width: 10.0,
            })
            .collect(),
        font_size: 10.0,
        bold: false,
        gray: 0.0,
    })
}

fn image() -> Item {
    Item::Image(ImageItem {
        component_id: "img".into(),
        x: 0.0,
        y: 50.0,
        w: 20.0,
        h: 30.0,
        asset_id: "a".into(),
        media_type: "image/png".into(),
    })
}

#[test]
fn cuts_between_lines_and_above_an_image_that_would_cross_the_cut() {
    close(cut_at(&[text_block(0.0, 10)], 0.0, 65.0), 60.0);
    close(cut_at(&[text_block(0.0, 10), image()], 0.0, 65.0), 48.0);
    assert_eq!(cut_at(&[image()], 50.0, 65.0), 50.0);
}

#[test]
fn slices_text_by_line_start_and_clips_rectangles() {
    let frame = Item::Rect(RectItem {
        component_id: "t".into(),
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 120.0,
        line_width: 1.0,
        gray: 0.0,
        fill: None,
    });
    let second = slice_items(&[text_block(0.0, 10), frame], 60.0, f64::INFINITY, 36.0 - 60.0);
    let Some(Item::Text(t)) = second.iter().find(|i| matches!(i, Item::Text(_))) else {
        panic!("text")
    };
    let lines: Vec<&str> = t.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(lines, ["l5", "l6", "l7", "l8", "l9"]);
    assert_eq!((t.y, t.h), (36.0, 60.0));
    let Some(Item::Rect(r)) = second.iter().find(|i| matches!(i, Item::Rect(_))) else {
        panic!("rect")
    };
    assert_eq!((r.y, r.h), (36.0, 60.0));
}
