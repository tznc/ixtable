//! Datasheet tools: multi-sort, filter by selection, find and replace, totals, hidden and
//! frozen columns, and paste from a spreadsheet (`datasheet-*`).

use crate::data::{DataValue, Sort};
use crate::ui::data::{
    sheet::grid::SheetEvent,
    testkit::{choose, make_table, paste_at, put, sheet},
};
use crate::ui::screenshot::context;
use crate::ui::testing::{Harness, UiContext};
use serde_json::json;

fn names<C: UiContext>(h: &mut Harness<'_, C>) -> Vec<String> {
    let mut values = vec![];
    while let Some(label) = h.try_label(format!("cell:{}:1", values.len())) {
        values.push(label);
    }
    values
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn datasheet() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, (1700.0, 900.0));
    h.click("new-document");
    make_table(
        &h,
        "orders",
        json!([
            { "name": "id", "declaredType": "INTEGER", "primaryKeyPosition": 1 },
            { "name": "customer", "declaredType": "TEXT" },
            { "name": "region", "declaredType": "TEXT" },
            { "name": "qty", "declaredType": "INTEGER" },
            { "name": "amount", "declaredType": "REAL" },
            { "name": "status", "declaredType": "TEXT" },
        ]),
        json!([]),
    );
    for (id, customer, region, qty, amount, status) in [
        (1, "Acme Ltd", "North", 4, 120.5, "Open"),
        (2, "Bolt Ltd", "South", 2, 80.0, "Shipped"),
        (3, "Core Inc", "North", 10, 640.0, "Open"),
        (4, "Delta Ltd", "East", 1, 15.25, "Cancelled"),
        (5, "Echo Inc", "South", 7, 210.0, "Open"),
        (6, "Fox Ltd", "North", 3, 99.99, "Shipped"),
    ] {
        put(
            &h,
            "orders",
            &[
                ("id", DataValue::Integer(id)),
                ("customer", DataValue::Text(customer.into())),
                ("region", DataValue::Text(region.into())),
                ("qty", DataValue::Integer(qty)),
                ("amount", DataValue::Real(amount)),
                ("status", DataValue::Text(status.into())),
            ],
        );
    }
    h.database_changed();
    h.click("table:orders");
    assert!(h.exists("cell:5:5"));

    // Sort by region, then qty (the shift-click of the header buttons).
    let view = sheet(&mut h);
    h.update(|_, cx| {
        view.update(cx, |_, cx| {
            cx.emit(SheetEvent::Sorts(vec![
                Sort { column: "region".into(), descending: false },
                Sort { column: "qty".into(), descending: false },
            ]))
        })
    });
    h.settle();
    let sorted = names(&mut h);
    assert_eq!(sorted[0], "Delta Ltd");
    assert_eq!(sorted[1..4], ["Fox Ltd", "Acme Ltd", "Core Inc"]);
    h.capture(
        "datasheet-01-toolbar-and-multi-sort",
        &[
            "A Datasheet tools bar above the grid shows a Filter menu (Filter by selection, Filter excluding selection), Find and Totals.",
            "The region and qty headers show sort arrows with small 1 and 2 markers, and rows are ordered by region then qty.",
            "Each column header has a small options button next to its name.",
        ],
    );

    h.click("cell:2:2");
    choose(&mut h, "filter", "Filter by selection");
    assert_eq!(names(&mut h), ["Fox Ltd", "Acme Ltd", "Core Inc"]);
    assert_eq!(h.label("filter-chip:0"), "region = North");
    h.capture(
        "datasheet-02-filter-by-selection",
        &[
            "A 'region = North' filter chip with a remove button and a 'Clear filters' button sits at the right of the tools bar.",
            "Only the three North rows are listed and the record count reads 3 records.",
        ],
    );
    h.click("clear-filters");
    assert_eq!(names(&mut h).len(), 6);

    h.click("find");
    h.type_into("find-text", "ltd");
    h.type_into("replace-text", "LLC");
    h.click("find-next");
    h.capture(
        "datasheet-03-find",
        &[
            "A Find and replace panel under the tools bar shows Find 'ltd', Replace with 'LLC', Look in, Whole field, Match case, and Find next, Replace and Replace all buttons.",
            "The first matching cell (customer, row 1, 'Delta Ltd') is selected.",
        ],
    );
    h.dialogs.queue_confirm(true);
    h.click("find-replace-all");
    assert_eq!(h.notice().as_deref(), Some("Replaced 4 matches in 4 records."));
    assert!(names(&mut h).contains(&"Acme LLC".to_string()));
    h.capture(
        "datasheet-04-replace-all",
        &[
            "A toast reports 'Replaced 4 matches in 4 records.'",
            "Every customer that ended in 'Ltd' now ends in 'LLC'; 'Core Inc' and 'Echo Inc' are unchanged.",
        ],
    );
    h.click("find-close");

    h.click("totals");
    choose(&mut h, "Total for qty", "Sum");
    choose(&mut h, "Total for amount", "Average");
    assert_eq!(h.label("total:qty"), "27");
    assert_eq!(h.label("total:amount"), "194.29");
    h.capture(
        "datasheet-05-totals-row",
        &[
            "A Total row sits under the last record with a picker in every column.",
            "qty shows Sum 27 and amount shows Average 194.29; other columns read None.",
            "The Totals button in the tools bar is highlighted as pressed.",
        ],
    );

    choose(&mut h, "Column options for status", "Hide column");
    assert!(!h.exists("cell:0:5"));
    choose(&mut h, "Column options for customer", "Freeze through this column");
    assert!(h.label("head:customer").contains("frozen"));
    assert!(h.label("head:id").contains("frozen"));
    assert!(!h.label("head:region").contains("frozen"));
    h.capture(
        "datasheet-06-hidden-and-frozen",
        &[
            "The status column is gone and the tools bar shows a '1 hidden' button and an 'Unfreeze columns' button.",
            "The id and customer columns are frozen, with a darker divider line after customer.",
        ],
    );

    assert!(paste_at(&mut h, 6, 0, "7\tGolf LLC\tWest\t5\t50\r\n8\tHotel Inc\tWest\t2\t20.5\r\n"));
    assert_eq!(h.notice().as_deref(), Some("Added 2 records."));
    assert!(h.exists("cell:7:1"));
    assert_eq!(h.label("total:qty"), "34");
    h.capture(
        "datasheet-07-paste-from-spreadsheet",
        &[
            "A notice under the tools bar reads 'Added 2 records.'",
            "Rows 7 and 8 (Golf LLC and Hotel Inc, region West) appear at the end of the grid.",
            "The totals row updates to qty Sum 34.",
        ],
    );
}
