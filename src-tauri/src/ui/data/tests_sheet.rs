//! Datasheet flows (ports of tests/integration/datasheet-tools.test.tsx and
//! optimistic-grid.test.tsx). Imports are explicit: `gpui_kit::*` would shadow `#[test]`.

use super::{
    sheet::find::CellAt,
    testkit::{choose, paste_at, sheet},
};
use crate::{
    data::DataValue,
    ui::testing::{shown, Harness},
};
use gpui_kit::TestAppContext;

fn text(value: &str) -> DataValue {
    DataValue::Text(value.into())
}

// Creates `orders` (id, name, region, qty) with three records and selects it.
fn seed_orders(h: &mut Harness) {
    h.create_table(
        "orders",
        &[("id", "INTEGER", 1), ("name", "TEXT", 0), ("region", "TEXT", 0), ("qty", "INTEGER", 0)],
    );
    for (id, name, region, qty) in [(1, "Acme Ltd", "north", 2), (2, "Bolt Ltd", "south", 3), (3, "Core Inc", "north", 10)] {
        h.insert_row(
            "orders",
            &[
                ("id", DataValue::Integer(id)),
                ("name", text(name)),
                ("region", text(region)),
                ("qty", DataValue::Integer(qty)),
            ],
        );
    }
    h.click("table:orders");
    h.click("Refresh");
    assert!(h.exists("cell:2:3"), "three records are shown");
}

// The text of every shown cell of a column.
fn column(h: &mut Harness, index: usize) -> Vec<String> {
    let mut values = vec![];
    while let Some(label) = h.try_label(format!("cell:{}:{index}", values.len())) {
        values.push(label);
    }
    values
}

fn column_of(page: &crate::data::Page, index: usize) -> Vec<String> {
    page.rows.iter().map(|r| shown(&r[index])).collect()
}

#[gpui_kit::test]
fn filters_by_selection_excludes_a_selection_and_clears_filters(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_orders(&mut h);
    h.click("cell:0:2");
    h.click("filter-selection");
    assert_eq!(column(&mut h, 1), ["Acme Ltd", "Core Inc"]);
    assert_eq!(h.label("filter-chip:0"), "region = north");

    h.click("cell:0:1");
    h.click("filter-excluding");
    assert_eq!(column(&mut h, 1), ["Core Inc"]);

    h.click("remove-filter:0");
    assert_eq!(column(&mut h, 1), ["Bolt Ltd", "Core Inc"]);
    h.click("clear-filters");
    assert_eq!(column(&mut h, 1).len(), 3);
    let s = sheet(&mut h);
    assert!(!h.cx.read(|cx| s.read(cx).can_filter()) || h.cx.read(|cx| s.read(cx).filters.is_empty()));
    assert!(!h.exists("filter-chip:0"));
}

#[gpui_kit::test]
fn finds_the_next_match_and_replaces_every_match_in_one_batch(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_orders(&mut h);
    h.click("find");
    h.type_into("find-text", "ltd");
    h.click("find-next");
    let s = sheet(&mut h);
    assert_eq!(h.cx.read(|cx| s.read(cx).active), Some(CellAt { row: 0, column: 1 }));
    h.click("find-next");
    assert_eq!(h.cx.read(|cx| s.read(cx).active), Some(CellAt { row: 1, column: 1 }));

    h.dialogs.queue_confirm(true);
    h.type_into("replace-text", "LLC");
    h.click("find-replace-all");
    assert_eq!(h.label("find-status"), "Replaced 2 matches in 2 records.");
    assert_eq!(h.dialogs.count("confirm"), 1);
    let page = h.read_page("orders");
    assert_eq!(column_of(&page, 1), ["Acme LLC", "Bolt LLC", "Core Inc"]);
}

#[gpui_kit::test]
fn shows_a_totals_row_computed_over_every_filtered_record(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_orders(&mut h);
    h.click("totals");
    choose(&mut h, "Total for qty", "Sum");
    assert_eq!(h.label("total:qty"), "15");

    h.click("cell:0:2");
    h.click("filter-selection");
    assert_eq!(h.label("total:qty"), "12");
    let s = sheet(&mut h);
    assert!(h.cx.read(|cx| s.read(cx).layout.totals.is_some()));
}

#[gpui_kit::test]
fn hides_shows_freezes_and_unfreezes_columns(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_orders(&mut h);
    choose(&mut h, "Column options for region", "Hide column");
    assert!(!h.exists("cell:0:2"), "region is hidden");
    assert_eq!(h.label("hidden-columns"), "1 hidden");
    choose(&mut h, "hidden-columns", "Show all columns");
    assert!(h.exists("cell:0:2"), "region is back");

    choose(&mut h, "Column options for name", "Freeze through this column");
    assert!(h.label("head:name").contains("frozen"));
    assert!(!h.label("head:qty").contains("frozen"));
    h.click("unfreeze-columns");
    assert!(!h.label("head:name").contains("frozen"));
}

#[gpui_kit::test]
fn pastes_a_spreadsheet_block_over_existing_records_and_as_new_records(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_orders(&mut h);
    assert!(paste_at(&mut h, 0, 3, "20\r\n30\r\n"));
    assert_eq!(h.label("sheet-notice"), "Pasted into 2 records.");
    assert_eq!(column_of(&h.read_page("orders"), 3), ["20", "30", "10"]);

    // Into the new-record row: each pasted line becomes a record.
    let s = sheet(&mut h);
    let rows = h.cx.read(|cx| s.read(cx).page.rows.len());
    assert!(paste_at(&mut h, rows, 0, "4\t\"Delta, \"\"Co\"\"\"\tsouth\t7\r\n5\tEcho\t\t1\r\n"));
    let page = h.read_page("orders");
    assert_eq!(page.total, 5);
    assert_eq!(shown(&page.rows[3][1]), "Delta, \"Co\"");
    assert_eq!(shown(&page.rows[3][2]), "south");
    assert_eq!(page.rows[4][2], DataValue::Null);

    assert!(paste_at(&mut h, rows, 0, "6\tFox\tnorth\tmany\r\n7\tGolf\tnorth\t1\r\n"));
    assert_eq!(
        h.label("data-error"),
        "Pasted row 1: qty requires an integer. Nothing was pasted."
    );
    assert_eq!(h.read_page("orders").total, 5);
}

#[gpui_kit::test]
fn rejects_a_grid_edit_made_from_stale_values_and_shows_the_current_ones(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.create_table("tickets", &[("id", "INTEGER", 1), ("status", "TEXT", 0)]);
    h.insert_row("tickets", &[("id", DataValue::Integer(1)), ("status", text("open"))]);
    h.click("table:tickets");
    assert_eq!(h.label("cell:0:1"), "open");

    // Someone else changes the record behind the open sheet.
    crate::recordstore::commands::update_row(
        h.label.clone(),
        "tickets".into(),
        vec![crate::data::NamedValue { column: "status".into(), value: text("closed") }],
        vec![DataValue::Integer(1)],
        Some(vec![crate::data::NamedValue { column: "status".into(), value: text("open") }]),
        None,
    )
    .expect("outside update");

    h.click("cell:0:1");
    h.fill("cell-input", "in progress");
    h.press("enter");
    let error = h.label("data-error");
    assert!(
        error.contains("changed by someone else since you opened it: status is now 'closed'"),
        "{error}"
    );
    assert_eq!(shown(&h.read_page("tickets").rows[0][1]), "closed");
    assert_eq!(h.label("cell:0:1"), "closed");

    h.click("cell:0:1");
    h.fill("cell-input", "done");
    h.press("enter");
    assert_eq!(shown(&h.read_page("tickets").rows[0][1]), "done");
}
