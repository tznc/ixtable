//! Import and export flows (ports of tests/integration/file-import.test.tsx and the
//! Data mode part of export.test.tsx). Imports are explicit: `gpui_kit::*` would shadow
//! `#[test]`.

use crate::{
    data::DataValue,
    ui::{
        shell::ModeId,
        testing::Harness,
    },
};
use gpui_kit::TestAppContext;
use std::path::PathBuf;

fn fixture(h: &Harness, name: &str, contents: &str) -> PathBuf {
    let path = h.temp_path(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

fn repo_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/imports").join(name)
}

// Opens the import dialog and chooses `path`, as a user does.
fn open_wizard(h: &mut Harness, path: PathBuf) {
    h.click("import-records");
    assert!(h.exists("import-dialog"));
    h.dialogs.queue_open(Some(path));
    h.click("choose-import-file");
    assert!(h.exists("import-status"), "the file preview is shown");
}

fn query(h: &Harness, sql: &str) -> Result<Vec<Vec<DataValue>>, crate::manager::AppError> {
    tauri::async_runtime::block_on(crate::queries::execute_parameterized_query(
        h.label.clone(),
        sql.into(),
        vec![],
        None,
        None,
        None,
    ))
    .map(|run| run.rows)
}

fn people(h: &mut Harness) {
    h.create_table("people", &[("id", "INTEGER", 1), ("name", "TEXT", 0)]);
    for (id, name) in [(1, "ACME"), (2, "Bolt"), (3, "Core")] {
        h.insert_row("people", &[("id", DataValue::Integer(id)), ("name", DataValue::Text(name.into()))]);
    }
    h.click("table:people");
    assert!(h.exists("cell:2:1"));
}

#[gpui_kit::test]
fn imports_a_csv_into_a_new_table_with_inferred_types_and_a_generated_key(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let path = fixture(&h, "Team Members.csv", "name;age;joined\nAda;36;2024-01-15\nGrace;45;2024-02-01\n");
    open_wizard(&mut h, path);
    assert_eq!(h.label("preview-head:age"), "age integer");
    assert_eq!(h.label("preview-head:joined"), "joined date");
    assert_eq!(h.label("import-status"), "2 rows \u{b7} showing the first 2");
    assert_eq!(h.find("Import table name").value(), Some("team_members"));
    assert_eq!(h.label("Type for age"), "Integer");

    h.click("import-run");
    assert_eq!(h.label("import-summary"), "Imported 2 of 2 rows into team_members.");
    h.click("import-close");
    assert!(!h.exists("import-dialog"));
    let page = h.read_page("team_members");
    let names: Vec<&str> = page.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "name", "age", "joined"]);
    assert_eq!(
        page.rows,
        [
            vec![DataValue::Integer(1), DataValue::Text("Ada".into()), DataValue::Integer(36), DataValue::Date("2024-01-15".into())],
            vec![DataValue::Integer(2), DataValue::Text("Grace".into()), DataValue::Integer(45), DataValue::Date("2024-02-01".into())],
        ]
    );
    // The new table is listed in the object browser.
    assert!(h.exists("table:team_members"));
}

#[gpui_kit::test]
fn maps_file_columns_onto_an_existing_table_and_reports_rows_that_fail_checks(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let spec = serde_json::json!({ "name": "members", "columns": [
        { "name": "id", "declaredType": "INTEGER", "nullable": false, "primaryKeyPosition": 1 },
        { "name": "name", "declaredType": "TEXT", "nullable": false },
        { "name": "age", "declaredType": "INTEGER" },
    ]});
    crate::create_database_table(h.label.clone(), serde_json::from_value(spec).unwrap()).unwrap();
    h.database_changed();
    let path = fixture(&h, "people.csv", "Name,Age,Note\nAda,36,x\n,40,y\nLinus,abc,z\n");
    open_wizard(&mut h, path);
    h.click("import-into-existing");
    assert_eq!(h.label("Field for Name"), "name (required)");
    assert_eq!(h.label("Field for Age"), "age");
    assert_eq!(h.label("Field for Note"), "Skip this column");

    h.click("import-run");
    assert_eq!(h.label("import-summary"), "Imported 1 of 3 rows into members. 2 rows were skipped.");
    let problems: Vec<[String; 3]> = (0..2)
        .map(|i| {
            [
                h.label(format!("report-row:{i}:row")),
                h.label(format!("report-row:{i}:field")),
                h.label(format!("report-row:{i}:problem")),
            ]
        })
        .collect();
    assert_eq!(
        problems,
        [
            ["2".to_string(), "name".to_string(), "name is required".to_string()],
            ["3".to_string(), "age".to_string(), "age requires an integer".to_string()],
        ]
    );
    let page = h.read_page("members");
    assert_eq!(
        page.rows,
        [vec![DataValue::Integer(1), DataValue::Text("Ada".into()), DataValue::Integer(36)]]
    );
}

#[gpui_kit::test]
fn imports_an_xlsx_worksheet_with_dates_and_booleans(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    open_wizard(&mut h, repo_fixture("people.xlsx"));
    assert_eq!(h.label("Worksheet"), "People");
    assert_eq!(h.label("Type for active"), "Yes / No");
    h.click("import-run");
    assert_eq!(h.label("import-summary"), "Imported 3 of 3 rows into people.");
    assert_eq!(
        h.read_page("people").rows[0],
        [
            DataValue::Integer(1),
            DataValue::Text("Ada".into()),
            DataValue::Integer(36),
            DataValue::Date("2024-01-15".into()),
            DataValue::Boolean(true),
            DataValue::Real(9.5),
        ]
    );
}

#[gpui_kit::test]
fn bundles_a_csv_as_a_read_only_file_source_that_queries_read_after_save_and_reopen(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    h.switch_mode(ModeId::App);
    h.click("tab-files");
    let path = fixture(&h, "sales.csv", "region,amount\nnorth,10\nsouth,5\nnorth,2\n");
    h.dialogs.queue_open(Some(path));
    h.click("add-file-source");
    assert_eq!(h.label("source-preview"), "3 rows \u{b7} region (text), amount (integer)");
    assert_eq!(h.find("Source name").value(), Some("sales"));
    h.click("confirm-add-source");
    assert_eq!(h.label("sources-status"), "Added files.sales.");
    assert_eq!(h.label("source:sales"), "files.sales");
    assert_eq!(h.label("source-columns:sales"), "region, amount");
    assert_eq!(h.label("source-rows:sales"), "3");

    let sql = "SELECT region, sum(amount) AS total FROM files.sales GROUP BY region ORDER BY region";
    let expected = vec![
        vec![DataValue::Text("north".into()), DataValue::Integer(12)],
        vec![DataValue::Text("south".into()), DataValue::Integer(5)],
    ];
    assert_eq!(query(&h, sql).unwrap(), expected);
    assert!(query(&h, "DELETE FROM files.sales").is_err());

    let saved = h.temp_path("sales.ixt");
    crate::save_document_as(h.label.clone(), saved.to_string_lossy().into_owned()).unwrap();
    crate::close_document(h.label.clone(), true).unwrap();
    crate::open_document(h.label.clone(), saved.to_string_lossy().into_owned()).unwrap();
    assert_eq!(query(&h, sql).unwrap(), expected);
}

#[gpui_kit::test]
fn imports_a_late_bad_value_as_a_skipped_row_instead_of_failing_the_import(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let mut lines = vec!["name,age".to_string()];
    for i in 1..=22_005 {
        let age = if i == 22_002 { "n/a".to_string() } else { (i % 90).to_string() };
        lines.push(format!("P{i},{age}"));
    }
    let path = fixture(&h, "late.csv", &format!("{}\n", lines.join("\n")));
    open_wizard(&mut h, path);
    // The whole file is sniffed, so the column is suggested as text; ask for integers.
    assert_eq!(h.label("Type for age"), "Text");
    h.pick("Type for age", "Integer");
    h.click("import-run");
    assert_eq!(
        h.label("import-summary"),
        "Imported 22,004 of 22,005 rows into late. 1 rows were skipped."
    );
    assert_eq!(h.label("report-row:0:row"), "22002");
    assert_eq!(h.label("report-row:0:problem"), "age requires an integer");
}

#[gpui_kit::test]
fn reports_an_import_that_stops_part_way_and_still_lists_the_new_table(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let mut good = vec!["name,age".to_string()];
    for i in 1..=24_000 {
        good.push(format!("P{i},{}", i % 90));
    }
    let path = fixture(&h, "broken.csv", &format!("{}\n", good.join("\n")));
    open_wizard(&mut h, path.clone());
    // The file changes after the preview: a row near the end has too many fields.
    good.insert(22_000, "X,1,extra,fields".into());
    std::fs::write(&path, format!("{}\n", good.join("\n"))).unwrap();
    h.click("import-run");
    assert!(h.label("import-aborted").starts_with("The import stopped after"));
    assert!(h.label("import-aborted").contains("Could not read the file"));
    assert!(h.label("import-summary").starts_with("Imported "));
    assert!(h.label("import-summary").ends_with(" rows into broken."));
    h.go("import-close");
    // The created table is listed even though the import did not finish.
    assert!(h.exists("table:broken"));
}

#[gpui_kit::test]
fn exports_a_sorted_table_to_csv_from_data_mode_and_reports_the_row_count(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    people(&mut h);
    h.click("sort:name");
    h.click("sort:name");
    assert!(h.label("sort:name").contains('\u{2193}'));

    let path = h.temp_path("people.csv");
    h.dialogs.queue_save(Some(path.clone()));
    h.pick("Export", "CSV");
    assert_eq!(h.label("export-status"), "Exported 3 rows");
    // A BOM so Excel reads UTF-8, then CRLF rows.
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text, "\u{feff}id,name\r\n3,Core\r\n2,Bolt\r\n1,ACME\r\n");
}

#[gpui_kit::test]
fn does_nothing_when_the_save_dialog_is_cancelled(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    people(&mut h);
    h.dialogs.queue_save(None);
    h.pick("Export", "JSON");
    assert_eq!(h.dialogs.count("save"), 1);
    assert!(!h.exists("export-status"));
    assert!(!h.exists("export-error"));
    assert!(h.exists("Export"));
}
