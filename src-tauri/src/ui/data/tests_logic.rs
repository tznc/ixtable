//! Ports of tests/unit/datasheet-tools.test.ts, datasheet-fields.test.ts,
//! relationship-layout.test.ts and relationships.test.ts.

use super::{
    layout::{default_layout, LayoutNode, Position},
    relationships::{
        drawn_from, handle_id, is_unique_key, relationship_edges, Connection, Direction,
        DrawnRelationship,
    },
    sheet::{
        filters::{add_filter, describe_filter, selection_filter, Op, SheetFilter},
        find::{cell_matches, cell_text, next_match, replace_text, CellAt, FindOptions},
        layout::{
            freeze_through, hide_column, read_layout, visible_columns, write_layout,
            DatasheetLayout,
        },
        paste::{
            is_multi_cell, parse_clipboard_grid, paste_writes, pasted_message, plan_paste,
            PastePlan,
        },
        rows::{is_editable, typed_values, SheetFields},
        sorts::next_sorts,
        totals::totals_for,
    },
};
use crate::archive::DocumentConfig;
use crate::data::{
    totals::TotalFunction, Column, DataValue, ForeignKey, IndexDef, NamedValue, Page, Sort,
    TableSchema, UniqueDef,
};
use crate::records::RecordOperation;
use crate::recordstore::FieldSettings;
use crate::ui::schema::testkit::{column, schema};
use serde_json::json;
use std::collections::BTreeMap;

fn col(name: &str, logical: &str) -> Column {
    column(name, logical)
}
fn text(value: &str) -> DataValue {
    DataValue::Text(value.into())
}
fn int(value: i64) -> DataValue {
    DataValue::Integer(value)
}
fn blob() -> DataValue {
    DataValue::Blob("AAAA".into())
}
fn opts(find: &str) -> FindOptions {
    FindOptions {
        find: find.into(),
        match_case: false,
        whole_field: false,
    }
}
fn cells(items: &[(usize, &str)]) -> BTreeMap<usize, String> {
    items.iter().map(|(k, v)| (*k, v.to_string())).collect()
}
fn config(navigation: serde_json::Value) -> DocumentConfig {
    let mut config: DocumentConfig =
        serde_json::from_value(json!({"version": 7, "name": "T", "activeMode": "data"})).unwrap();
    config.navigation_state = navigation;
    config
}
fn sort(column: &str, descending: bool) -> Sort {
    Sort {
        column: column.into(),
        descending,
    }
}
fn pairs(sorts: &[Sort]) -> Vec<(String, bool)> {
    sorts.iter().map(|s| (s.column.clone(), s.descending)).collect()
}

// ---- parseClipboardGrid / isMultiCell ---------------------------------------

#[test]
fn parses_tab_separated_rows_with_lf_and_excel_crlf() {
    let expected = vec![vec!["a", "b"], vec!["c", "d"]];
    assert_eq!(parse_clipboard_grid("a\tb\nc\td"), expected);
    assert_eq!(parse_clipboard_grid("a\tb\r\nc\td\r\n"), expected);
    assert_eq!(parse_clipboard_grid("a\n"), vec![vec!["a"]]);
    assert!(parse_clipboard_grid("").is_empty());
    assert_eq!(
        parse_clipboard_grid("a\t\tc\n\tb\t"),
        vec![vec!["a", "", "c"], vec!["", "b", ""]]
    );
}

#[test]
fn reads_quoted_fields_with_tabs_newlines_and_doubled_quotes() {
    assert_eq!(
        parse_clipboard_grid("\"a\tb\"\t\"line1\nline2\"\t\"say \"\"hi\"\"\"\r\nx\ty\tz\r\n"),
        vec![vec!["a\tb", "line1\nline2", "say \"hi\""], vec!["x", "y", "z"]]
    );
    assert_eq!(parse_clipboard_grid("ab\"c\td"), vec![vec!["ab\"c", "d"]]);
}

#[test]
fn detects_multi_cell_text() {
    assert!(!is_multi_cell("hello"));
    assert!(!is_multi_cell("hello\n"));
    assert!(!is_multi_cell("hello\r\n"));
    assert!(is_multi_cell("a\tb"));
    assert!(is_multi_cell("a\nb"));
    assert!(is_multi_cell("a\r\nb\r\n"));
}

// ---- planPaste ---------------------------------------------------------------

fn grid() -> Vec<Vec<String>> {
    vec![
        vec!["a".into(), "b".into(), "c".into()],
        vec!["d".into(), "e".into(), "f".into()],
    ]
}

#[test]
fn maps_cells_onto_the_given_column_order_skipping_columns_not_listed() {
    let plan = plan_paste(&grid(), &[0, 2, 3, 5], (1, 2), 10);
    assert!(plan.inserts.is_empty());
    assert_eq!(plan.updates.iter().map(|u| u.0).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(plan.updates[0].1, cells(&[(2, "a"), (3, "b"), (5, "c")]));
}

#[test]
fn drops_cells_past_the_last_column() {
    let plan = plan_paste(&grid(), &[0, 1], (0, 1), 5);
    assert_eq!(plan.updates[0].1, cells(&[(1, "a")]));
    assert_eq!(plan.updates[1].1, cells(&[(1, "d")]));
}

#[test]
fn turns_rows_past_row_count_into_inserts() {
    let plan = plan_paste(&grid(), &[0, 1, 2], (2, 0), 3);
    assert_eq!(plan.updates.iter().map(|u| u.0).collect::<Vec<_>>(), [2]);
    assert_eq!(plan.inserts, vec![cells(&[(0, "d"), (1, "e"), (2, "f")])]);
    let all_new = plan_paste(&grid(), &[0, 1, 2], (4, 0), 4);
    assert!(all_new.updates.is_empty());
    assert_eq!(all_new.inserts.len(), 2);
    assert_eq!(plan_paste(&grid(), &[0, 1], (0, 9), 5), PastePlan::default());
}

// ---- pasteWrites -------------------------------------------------------------

fn items_page() -> Page {
    let mut id = col("id", "integer");
    id.primary_key_position = 1;
    Page {
        columns: vec![id, col("qty", "integer"), col("note", "text")],
        rows: vec![
            vec![int(1), int(5), text("old")],
            vec![int(2), int(6), DataValue::Null],
        ],
        identities: vec![vec![int(1)], vec![int(2)]],
        total: 2,
        offset: 0,
        limit: 100,
    }
}

fn named(column: &str, value: DataValue) -> NamedValue {
    NamedValue {
        column: column.into(),
        value,
    }
}

#[test]
fn builds_updates_with_expected_values_and_null_for_empty_text() {
    let page = items_page();
    let plan = plan_paste(&[vec!["9".into(), "".into()]], &[1, 2], (0, 1), 2);
    let writes = paste_writes("items", &page, &plan, &SheetFields::new()).unwrap();
    let write = &writes[0];
    assert_eq!(write.operation, RecordOperation::Update);
    assert_eq!(write.table, "items");
    assert_eq!(write.identity, Some(vec![int(1)]));
    assert_eq!(
        write.values,
        vec![named("qty", int(9)), named("note", DataValue::Null)]
    );
    assert_eq!(
        write.meta.expected,
        Some(vec![
            named("id", int(1)),
            named("qty", int(5)),
            named("note", text("old"))
        ])
    );
}

#[test]
fn builds_inserts_that_omit_empty_text() {
    let page = items_page();
    let plan = plan_paste(&[vec!["".into(), "hi".into()]], &[1, 2], (2, 1), 2);
    let writes = paste_writes("items", &page, &plan, &SheetFields::new()).unwrap();
    assert_eq!(writes[0].operation, RecordOperation::Insert);
    assert_eq!(writes[0].identity, None);
    assert_eq!(writes[0].values, vec![named("note", text("hi"))]);
    assert!(writes[0].meta.expected.is_none());
}

#[test]
fn lists_updates_before_inserts_and_names_the_bad_pasted_row() {
    let page = items_page();
    let three = vec![vec!["1".to_string()], vec!["2".into()], vec!["3".into()]];
    let plan = plan_paste(&three, &[1], (1, 1), 2);
    let ops: Vec<_> = paste_writes("items", &page, &plan, &SheetFields::new())
        .unwrap()
        .iter()
        .map(|w| w.operation)
        .collect();
    assert_eq!(
        ops,
        [RecordOperation::Update, RecordOperation::Insert, RecordOperation::Insert]
    );
    let bad = plan_paste(&[vec!["1".into()], vec!["abc".into()]], &[1], (0, 1), 2);
    assert!(paste_writes("items", &page, &bad, &SheetFields::new())
        .unwrap_err()
        .contains("Pasted row 2: qty requires an integer"));
    let after = plan_paste(&[vec!["1".into()], vec!["x".into()]], &[1], (1, 1), 2);
    assert!(paste_writes("items", &page, &after, &SheetFields::new())
        .unwrap_err()
        .contains("Pasted row 2:"));
}

#[test]
fn describes_pastes() {
    assert_eq!(pasted_message(1, 0), "Pasted into 1 record.");
    assert_eq!(pasted_message(3, 0), "Pasted into 3 records.");
    assert_eq!(pasted_message(0, 1), "Added 1 record.");
    assert_eq!(pasted_message(0, 4), "Added 4 records.");
    assert_eq!(pasted_message(2, 1), "Pasted into 2 records and added 1 record.");
}

// ---- find --------------------------------------------------------------------

#[test]
fn find_cell_text_skips_nulls_and_blobs() {
    assert_eq!(cell_text(&DataValue::Null, None), None);
    assert_eq!(cell_text(&blob(), None), None);
    assert_eq!(cell_text(&int(42), None).as_deref(), Some("42"));
    assert_eq!(cell_text(&text("abc"), None).as_deref(), Some("abc"));
}

#[test]
fn find_matches_substrings_case_and_whole_field() {
    assert!(cell_matches(&text("Hello World"), &opts("lo wo"), None));
    assert!(!cell_matches(&text("Hello World"), &opts("xyz"), None));
    let case = |find: &str| FindOptions {
        match_case: true,
        ..opts(find)
    };
    assert!(!cell_matches(&text("Hello"), &case("hello"), None));
    assert!(cell_matches(&text("Hello"), &case("Hello"), None));
    let whole = |find: &str| FindOptions {
        whole_field: true,
        ..opts(find)
    };
    assert!(!cell_matches(&text("Hello"), &whole("ell"), None));
    assert!(cell_matches(&text("Hello"), &whole("hello"), None));
    assert!(cell_matches(&int(1234), &opts("23"), None));
    assert!(!cell_matches(&text("abc"), &opts(""), None));
}

#[test]
fn find_treats_special_characters_literally_and_skips_nulls_and_blobs() {
    assert!(cell_matches(&text("a.c"), &opts("a.c"), None));
    assert!(!cell_matches(&text("abc"), &opts("a.c"), None));
    assert!(cell_matches(&text("price (usd)"), &opts("(usd)"), None));
    assert!(cell_matches(&text("a+b"), &opts("a+b"), None));
    assert!(!cell_matches(&text("aab"), &opts("a+b"), None));
    let whole = FindOptions {
        whole_field: true,
        ..opts("[x]")
    };
    assert!(cell_matches(&text("[x]"), &whole, None));
    assert!(cell_matches(&text("back\\slash"), &opts("\\"), None));
    assert!(!cell_matches(&DataValue::Null, &opts("null"), None));
    assert!(!cell_matches(&blob(), &opts("blob"), None));
}

#[test]
fn replaces_every_match_literally() {
    assert_eq!(replace_text("a.b.c", "-", &opts(".")), "a-b-c");
    assert_eq!(replace_text("Foo foo FOO", "x", &opts("foo")), "x x x");
    let case = FindOptions {
        match_case: true,
        ..opts("foo")
    };
    assert_eq!(replace_text("Foo foo", "x", &case), "Foo x");
    let whole = |find: &str| FindOptions {
        whole_field: true,
        ..opts(find)
    };
    assert_eq!(replace_text("abc", "z", &whole("ABC")), "z");
    assert_eq!(replace_text("abcd", "z", &whole("abc")), "abcd");
    assert_eq!(replace_text("cost", "$&$1", &opts("cost")), "$&$1");
}

#[test]
fn next_match_walks_cells_in_reading_order() {
    let rows = vec![
        vec![text("apple"), text("pear"), DataValue::Null],
        vec![text("grape"), blob(), text("apple pie")],
        vec![text("plum"), text("fig"), text("Apple")],
    ];
    let all = [0, 1, 2];
    let at = |row, column| Some(CellAt { row, column });
    let none: &[Option<FieldSettings>] = &[];
    assert_eq!(next_match(&rows, &all, &opts("apple"), None, none), at(0, 0));
    assert_eq!(
        next_match(&rows, &all, &opts("apple"), at(0, 0), none),
        at(1, 2)
    );
    assert_eq!(
        next_match(&rows, &all, &opts("apple"), at(1, 2), none),
        at(2, 2)
    );
    assert_eq!(next_match(&rows, &all, &opts("apple"), at(2, 2), none), None);
    assert_eq!(next_match(&rows, &all, &opts("zzz"), None, none), None);
    assert_eq!(
        next_match(&rows, &[0, 1], &opts("apple"), at(0, 0), none),
        None
    );
    assert_eq!(next_match(&rows, &[2], &opts("apple"), None, none), at(1, 2));
    assert_eq!(
        next_match(&rows, &[2, 0], &opts("apple"), at(1, 2), none),
        at(2, 2)
    );
    assert_eq!(next_match(&rows, &all, &opts("blob"), None, none), None);
    assert_eq!(next_match(&rows, &all, &opts("null"), None, none), None);
}

// ---- filters -----------------------------------------------------------------

fn eq(column: &str, value: &str) -> SheetFilter {
    SheetFilter {
        column: column.into(),
        operator: Op::Eq,
        value: Some(text(value)),
    }
}

#[test]
fn builds_filters_for_selected_values() {
    assert_eq!(selection_filter("Region", &text("North"), false), Some(eq("Region", "North")));
    let ne = selection_filter("Region", &text("North"), true).unwrap();
    assert_eq!(ne.operator, Op::Ne);
    assert_eq!(ne.value, Some(text("North")));
    let null = selection_filter("Region", &DataValue::Null, false).unwrap();
    assert_eq!((null.operator, null.value), (Op::IsNull, None));
    let not_null = selection_filter("Region", &DataValue::Null, true).unwrap();
    assert_eq!(not_null.operator, Op::IsNotNull);
    assert_eq!(selection_filter("Photo", &blob(), false), None);
    assert_eq!(selection_filter("Photo", &blob(), true), None);
}

#[test]
fn adds_filters_replacing_the_same_column_and_operator() {
    let base = vec![eq("a", "1")];
    assert_eq!(add_filter(&base, eq("b", "2")), vec![eq("a", "1"), eq("b", "2")]);
    let ne = SheetFilter {
        operator: Op::Ne,
        ..eq("a", "1")
    };
    assert_eq!(add_filter(&base, ne.clone()), vec![eq("a", "1"), ne]);
    assert_eq!(
        add_filter(&[eq("a", "1"), eq("b", "2")], eq("a", "3")),
        vec![eq("b", "2"), eq("a", "3")]
    );
    assert_eq!(base, vec![eq("a", "1")]);
}

#[test]
fn describes_filters() {
    assert_eq!(describe_filter(&eq("Region", "North")), "Region = North");
    let with = |operator, value: Option<DataValue>, column: &str| SheetFilter {
        column: column.into(),
        operator,
        value,
    };
    assert_eq!(
        describe_filter(&with(Op::Ne, Some(text("North")), "Region")),
        "Region ≠ North"
    );
    assert_eq!(
        describe_filter(&with(Op::Contains, Some(text("x")), "Name")),
        "Name contains x"
    );
    assert_eq!(describe_filter(&with(Op::Gt, Some(int(3)), "Qty")), "Qty gt 3");
    assert_eq!(describe_filter(&with(Op::IsNull, None, "Region")), "Region is empty");
    assert_eq!(
        describe_filter(&with(Op::IsNotNull, None, "Region")),
        "Region is not empty"
    );
}

// ---- layout ------------------------------------------------------------------

fn four() -> Vec<Column> {
    ["a", "b", "c", "d"].iter().map(|n| col(n, "text")).collect()
}
fn layout(hidden: &[&str], frozen: usize) -> DatasheetLayout {
    DatasheetLayout {
        hidden: hidden.iter().map(|s| s.to_string()).collect(),
        frozen,
        totals: None,
    }
}

#[test]
fn reads_defaults_and_sanitises_malformed_saved_values() {
    assert_eq!(read_layout(&config(json!(null)), "t"), DatasheetLayout::default());
    assert_eq!(read_layout(&config(json!({})), "t"), DatasheetLayout::default());
    let malformed = config(json!({
        "datasheetLayouts": { "t": { "hidden": "x", "frozen": -2, "totals": "sum" } }
    }));
    assert_eq!(read_layout(&malformed, "t"), DatasheetLayout::default());
}

#[test]
fn round_trips_layouts_through_the_navigation_state() {
    let mut saved = layout(&["b"], 2);
    saved.totals = Some(BTreeMap::from([("a".to_string(), TotalFunction::Count)]));
    let mut cfg = config(json!(null));
    write_layout(&mut cfg, "t", &saved);
    assert_eq!(read_layout(&cfg, "t"), saved);
    assert_eq!(read_layout(&cfg, "other"), DatasheetLayout::default());
}

#[test]
fn keeps_other_tables_and_other_navigation_keys_and_removes_empty_layouts() {
    let mut cfg = config(json!({ "tab": "data", "datasheetLayouts": { "u": { "frozen": 1 } } }));
    write_layout(&mut cfg, "t", &layout(&[], 3));
    assert_eq!(cfg.navigation_state["tab"], "data");
    assert_eq!(read_layout(&cfg, "u").frozen, 1);
    assert_eq!(read_layout(&cfg, "t").frozen, 3);
    write_layout(&mut cfg, "t", &DatasheetLayout::default());
    assert!(cfg.navigation_state["datasheetLayouts"].get("t").is_none());
    assert_eq!(cfg.navigation_state["tab"], "data");
}

#[test]
fn lists_visible_columns_in_table_order() {
    let columns = four();
    assert_eq!(visible_columns(&columns, &layout(&[], 0)), [0, 1, 2, 3]);
    assert_eq!(visible_columns(&columns, &layout(&["b", "d"], 0)), [0, 2]);
}

#[test]
fn hides_columns_keeping_the_frozen_count_in_range() {
    let columns = four();
    assert_eq!(hide_column(&layout(&[], 0), &columns, "c").hidden, ["c"]);
    assert_eq!(hide_column(&layout(&[], 2), &columns, "a").frozen, 1);
    assert_eq!(hide_column(&layout(&[], 2), &columns, "b").frozen, 1);
    assert_eq!(hide_column(&layout(&[], 2), &columns, "c").frozen, 2);
    let result = hide_column(&layout(&["a"], 1), &columns, "c");
    assert_eq!(result.frozen, 1);
    assert_eq!(result.hidden, ["a", "c"]);
    let only = layout(&["a", "b", "c"], 0);
    assert_eq!(hide_column(&only, &columns, "d"), only);
    let hidden = layout(&["a"], 0);
    assert_eq!(hide_column(&hidden, &columns, "zzz"), hidden);
    assert_eq!(hide_column(&hidden, &columns, "a"), hidden);
}

#[test]
fn freezes_through_a_visible_column() {
    let columns = four();
    assert_eq!(freeze_through(&layout(&[], 0), &columns, "c").frozen, 3);
    assert_eq!(freeze_through(&layout(&["a"], 0), &columns, "c").frozen, 2);
    let l = layout(&["a"], 1);
    assert_eq!(freeze_through(&l, &columns, "a"), l);
    assert_eq!(freeze_through(&l, &columns, "zzz"), l);
}

// ---- sorts -------------------------------------------------------------------

#[test]
fn sorts_by_a_new_column_replacing_existing_sorts_and_flips_on_repeat() {
    assert_eq!(pairs(&next_sorts(&[], "a", false)), [("a".to_string(), false)]);
    assert_eq!(
        pairs(&next_sorts(&[sort("b", true)], "a", false)),
        [("a".to_string(), false)]
    );
    assert_eq!(
        pairs(&next_sorts(&[sort("a", false)], "a", false)),
        [("a".to_string(), true)]
    );
    assert_eq!(
        pairs(&next_sorts(&[sort("a", true)], "a", false)),
        [("a".to_string(), false)]
    );
    assert_eq!(
        pairs(&next_sorts(&[sort("a", false), sort("b", true)], "b", false)),
        [("b".to_string(), false)]
    );
}

#[test]
fn shift_click_appends_or_flips_a_key_in_place() {
    assert_eq!(
        pairs(&next_sorts(&[sort("a", true)], "b", true)),
        [("a".to_string(), true), ("b".to_string(), false)]
    );
    let both = [sort("a", false), sort("b", false)];
    assert_eq!(
        pairs(&next_sorts(&both, "a", true)),
        [("a".to_string(), true), ("b".to_string(), false)]
    );
    assert_eq!(
        pairs(&next_sorts(&both, "b", true)),
        [("a".to_string(), false), ("b".to_string(), true)]
    );
}

// ---- totals ------------------------------------------------------------------

#[test]
fn offers_totals_by_column_type() {
    assert_eq!(totals_for(&col("q", "integer")).len(), 7);
    assert_eq!(totals_for(&col("q", "decimal(10,2)")).len(), 7);
    assert_eq!(totals_for(&col("q", "boolean")), [TotalFunction::Count]);
    assert_eq!(
        totals_for(&col("q", "text")),
        [TotalFunction::Count, TotalFunction::Min, TotalFunction::Max]
    );
}

// ---- datasheet-fields.test.ts ---------------------------------------------------

fn field(id: &str, column: &str) -> FieldSettings {
    FieldSettings {
        id: id.into(),
        column: column.into(),
        ..Default::default()
    }
}

#[test]
fn leaves_formatted_fields_to_forms_and_applies_masks_before_the_logical_type() {
    let columns = vec![col("id", "integer"), col("phone", "text"), col("tags", "text")];
    let phone = FieldSettings {
        input_mask: Some("000-0000".into()),
        ..field("f1", "phone")
    };
    let tags = FieldSettings {
        format: Some("multiSelect".into()),
        ..field("f2", "tags")
    };
    assert!(is_editable(&columns[1], Some(&phone)));
    assert!(!is_editable(&columns[2], Some(&tags)));
    assert!(is_editable(&columns[2], None));

    let fields: SheetFields = vec![None, Some(phone.clone()), Some(tags.clone())];
    assert_eq!(
        typed_values(&columns, &cells(&[(1, "555-1234")]), false, &fields).unwrap(),
        vec![named("phone", text("5551234"))]
    );
    assert!(typed_values(&columns, &cells(&[(1, "55")]), false, &fields)
        .unwrap_err()
        .contains("phone must match"));

    let page = Page {
        columns: columns.clone(),
        rows: vec![vec![int(1), text("5550000"), text("[\"a\"]")]],
        identities: vec![vec![int(1)]],
        total: 1,
        offset: 0,
        limit: 100,
    };
    let plan = plan_paste(&[vec!["555-1234".into()], vec!["12".into()]], &[1], (0, 1), 1);
    assert!(paste_writes("people", &page, &plan, &fields)
        .unwrap_err()
        .contains("Pasted row 2:"));
    let one = plan_paste(&[vec!["555-1234".into()]], &[1], (0, 1), 1);
    let writes = paste_writes("people", &page, &one, &fields).unwrap();
    assert_eq!(writes[0].values, vec![named("phone", text("5551234"))]);
    assert_eq!(writes[0].meta.expected.as_ref().map(Vec::len), Some(3));
}

#[test]
fn finds_the_masked_and_formatted_text_a_cell_shows() {
    let phone = FieldSettings {
        input_mask: Some("000-0000".into()),
        ..field("f1", "phone")
    };
    let tags = FieldSettings {
        format: Some("multiSelect".into()),
        ..field("f2", "tags")
    };
    assert_eq!(cell_text(&text("5551234"), Some(&phone)).as_deref(), Some("555-1234"));
    assert_eq!(
        cell_text(&text("[\"red\",\"blue\"]"), Some(&tags)).as_deref(),
        Some("red, blue")
    );
    assert!(cell_matches(&text("5551234"), &opts("5-12"), Some(&phone)));
    let rows = vec![vec![int(1), text("5551234"), text("[\"red\",\"blue\"]")]];
    let fields = vec![None, Some(phone), Some(tags)];
    assert_eq!(
        next_match(&rows, &[0, 1, 2], &opts("red, b"), None, &fields),
        Some(CellAt { row: 0, column: 2 })
    );
    assert_eq!(next_match(&rows, &[0, 1, 2], &opts("red, b"), None, &[]), None);
}

// ---- relationship-layout.test.ts ------------------------------------------------

fn table(fields: usize) -> LayoutNode {
    LayoutNode {
        columns: fields,
        ..Default::default()
    }
}
fn related(name: &str, fields: usize, parents: &[&str]) -> LayoutNode {
    LayoutNode {
        name: Some(name.into()),
        columns: fields,
        targets: parents.iter().map(|p| p.to_string()).collect(),
    }
}
fn at(x: f32, y: f32) -> Position {
    Position { x, y }
}

#[test]
fn places_three_tables_per_row_with_the_original_spacing() {
    assert_eq!(
        default_layout(&[table(2), table(3), table(4), table(1)]),
        vec![at(30.0, 25.0), at(330.0, 25.0), at(630.0, 25.0), at(30.0, 215.0)]
    );
    let positions = default_layout(&[table(3), table(10), table(2), table(1)]);
    assert_eq!(positions[3], at(30.0, 25.0 + 280.0 + 50.0));
    assert!(default_layout(&[]).is_empty());
}

fn dependent_tables() -> Vec<LayoutNode> {
    vec![
        related("customers", 3, &[]),
        related("order_items", 4, &["orders"]),
        related("orders", 5, &["customers"]),
        related("shipments", 4, &["orders"]),
    ]
}

#[test]
fn puts_referenced_tables_left_of_the_tables_that_reference_them() {
    let positions = default_layout(&dependent_tables());
    assert_eq!(positions[0], at(30.0, 25.0));
    assert_eq!(positions[2], at(330.0, 25.0));
    assert_eq!(positions[1].x, 630.0);
    assert_eq!(positions[3].x, 630.0);
    assert!(positions[3].y > positions[1].y);
}

#[test]
fn draws_every_edge_between_neighbouring_columns() {
    let tables = dependent_tables();
    let positions = default_layout(&tables);
    for (child, node) in tables.iter().enumerate() {
        for target in &node.targets {
            let parent = tables
                .iter()
                .position(|t| t.name.as_deref() == Some(target))
                .unwrap();
            assert_eq!(positions[child].x - positions[parent].x, 300.0);
        }
    }
}

#[test]
fn places_unrelated_tables_below_the_related_ones_and_survives_cycles() {
    let positions = default_layout(&[
        related("a", 2, &["b"]),
        related("b", 2, &["a"]),
        related("self", 2, &["self"]),
        related("lonely", 2, &[]),
    ]);
    assert_eq!(positions.len(), 4);
    assert_ne!(positions[0].x, positions[1].x);
    assert_eq!(positions[2], at(30.0, positions[3].y));
    assert_eq!(positions[3], at(330.0, 25.0 + 140.0 + 50.0));
}

// ---- relationships.test.ts ------------------------------------------------------

fn fk(from: &[&str], target: &str, to: &[&str]) -> ForeignKey {
    ForeignKey {
        id: 0,
        name: None,
        from_columns: from.iter().map(|s| s.to_string()).collect(),
        target_table: target.into(),
        target_columns: to.iter().map(|s| s.to_string()).collect(),
        on_update: "NO ACTION".into(),
        on_delete: "NO ACTION".into(),
    }
}
fn rel_table(name: &str, columns: &[&str], patch: impl FnOnce(&mut TableSchema)) -> TableSchema {
    let mut s = schema(name, columns.iter().map(|c| col(c, "integer")).collect(), &[]);
    patch(&mut s);
    s
}
fn names(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn regions() -> TableSchema {
    rel_table("regions", &["country", "code", "title"], |s| {
        s.primary_key = names(&["country", "code"])
    })
}
fn sites() -> TableSchema {
    rel_table("sites", &["id", "country", "code"], |s| {
        s.primary_key = names(&["id"]);
        s.foreign_keys = vec![fk(&["country", "code"], "regions", &["country", "code"])];
    })
}
fn users() -> TableSchema {
    rel_table("users", &["id"], |s| s.primary_key = names(&["id"]))
}
fn profiles() -> TableSchema {
    rel_table("profiles", &["id", "user_id"], |s| {
        s.primary_key = names(&["id"]);
        s.uniques = vec![UniqueDef {
            name: None,
            columns: names(&["user_id"]),
        }];
        s.foreign_keys = vec![fk(&["user_id"], "users", &["id"])];
    })
}

#[test]
fn anchors_every_column_of_a_composite_key_and_labels_it_once() {
    let edges = relationship_edges(&[regions(), sites()]);
    assert_eq!(edges.len(), 2);
    assert_eq!(edges[0].id, "sites-0-0");
    assert_eq!(edges[0].source, "regions");
    assert_eq!(edges[0].source_handle, "c0-source");
    assert_eq!(edges[0].target, "sites");
    assert_eq!(edges[0].target_handle, "c1-target");
    assert_eq!(edges[0].label.as_deref(), Some("1 — ∞ (country, code)"));
    assert_eq!(edges[1].id, "sites-0-1");
    assert_eq!(edges[1].source_handle, "c1-source");
    assert_eq!(edges[1].target_handle, "c2-target");
    assert_eq!(edges[1].label, None);
}

#[test]
fn labels_one_to_one_when_the_referencing_columns_are_unique() {
    assert_eq!(
        relationship_edges(&[users(), profiles()])[0].label.as_deref(),
        Some("1 — 1")
    );
    let keyed = rel_table("details", &["user_id"], |s| {
        s.primary_key = names(&["user_id"]);
        s.foreign_keys = vec![fk(&["user_id"], "users", &["id"])];
    });
    assert_eq!(
        relationship_edges(&[users(), keyed])[0].label.as_deref(),
        Some("1 — 1")
    );
}

#[test]
fn detects_uniqueness_from_keys_constraints_indexes_and_column_flags() {
    assert!(is_unique_key(&sites(), &names(&["id"])));
    assert!(!is_unique_key(&sites(), &names(&["country", "code"])));
    assert!(is_unique_key(&regions(), &names(&["code", "country"])));
    assert!(!is_unique_key(&regions(), &names(&["code"])));
    let indexed = rel_table("t", &["a"], |s| {
        s.indexes = vec![IndexDef {
            name: "i".into(),
            table: "t".into(),
            columns: names(&["a"]),
            unique: true,
            sql: None,
        }]
    });
    assert!(is_unique_key(&indexed, &names(&["a"])));
    let flagged = rel_table("t", &["a"], |s| s.columns[0].unique = true);
    assert!(is_unique_key(&flagged, &names(&["a"])));
}

#[test]
fn maps_a_connection_from_a_parent_column_to_a_child_column() {
    let schemas = [regions(), sites()];
    let drawn = drawn_from(
        &Connection {
            source: Some("regions"),
            source_handle: Some(&handle_id(1, Direction::Source)),
            target: Some("sites"),
            target_handle: Some(&handle_id(2, Direction::Target)),
        },
        &schemas,
    );
    assert_eq!(
        drawn,
        Some(DrawnRelationship {
            child_table: "sites".into(),
            child_column: "code".into(),
            parent_table: "regions".into(),
            parent_column: "code".into(),
        })
    );
    let unknown_column = Connection {
        source: Some("regions"),
        source_handle: Some("c9-source"),
        target: Some("sites"),
        target_handle: Some("c0-target"),
    };
    assert_eq!(drawn_from(&unknown_column, &schemas), None);
    let missing = Connection {
        source: None,
        source_handle: None,
        target: Some("sites"),
        target_handle: None,
    };
    assert_eq!(drawn_from(&missing, &schemas), None);
}
