//! Exporting a table from Data mode (`export-01`, `export-02`). The Query-mode and Runtime
//! captures of the old `export` spec belong to those modes' specs.

use crate::ui::data::testkit::{choose_open, menu_labels, seed_sales};
use crate::ui::screenshot::context;
use crate::ui::testing::Harness;

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn export() {
    let mut cx = context();
    let mut h = Harness::new_document(&mut cx);
    seed_sales(&mut h);
    h.click("table:customers");
    assert!(h.exists("cell:0:1"));
    h.click("sort:name");
    h.click("sort:name");
    assert!(h.label("sort:name").contains('\u{2193}'));

    h.click("Export");
    assert_eq!(menu_labels(&mut h), ["CSV", "Excel (.xlsx)", "JSON"]);
    h.capture(
        "export-01-data-menu",
        &[
            "The customers grid is sorted by name descending (a down arrow on the name header).",
            "An Export button sits in the grid toolbar next to Refresh, with its menu open below it.",
            "The menu lists CSV, Excel (.xlsx) and JSON.",
        ],
    );

    let path = h.temp_path("customers.csv");
    h.dialogs.queue_save(Some(path.clone()));
    choose_open(&mut h, "CSV");
    let status = h.notice().expect("export toast");
    let text = std::fs::read_to_string(&path).expect("exported file");
    let lines: Vec<&str> = text.trim_start_matches('\u{feff}').trim().split("\r\n").collect();
    assert_eq!(lines[0], "id,name,city");
    assert_eq!(status, format!("Exported {} rows", lines.len() - 1));
    h.capture(
        "export-02-data-done",
        &[
            "The menu has closed and the toolbar shows a confirmation such as \"Exported 3 rows\".",
            "The grid itself is unchanged.",
        ],
    );
}
