//! The CSV/XLSX import wizard (`import-*`): file preview, new-table columns, an XLSX
//! worksheet, mapping onto an existing table and the report of skipped rows.

use crate::ui::screenshot::context;
use crate::ui::testing::Harness;
use gpui_kit::HeadlessAppContext;
use serde_json::json;
use std::path::PathBuf;

const WINDOW: (f32, f32) = (1280.0, 1300.0);

type H<'a> = Harness<'a, HeadlessAppContext>;

fn fixture(h: &H<'_>, name: &str, contents: &str) -> PathBuf {
    let path = h.temp_path(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

fn open_wizard(h: &mut H<'_>, path: PathBuf) {
    h.click("import-records");
    assert!(h.exists("import-dialog"));
    h.dialogs.queue_open(Some(path));
    h.click("choose-import-file");
    assert!(h.exists("import-status"), "the file preview is shown");
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn import_wizard() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, WINDOW);
    h.click("new-document");

    let csv = fixture(
        &h,
        "Team Members.csv",
        "name;age;joined\nAda;36;2024-01-15\nGrace;45;2024-02-01\nLinus;52;2024-03-09\n",
    );
    open_wizard(&mut h, csv);
    assert_eq!(h.label("preview-head:age"), "age integer");
    assert_eq!(h.label("preview-head:joined"), "joined date");
    assert_eq!(h.find("Import table name").value(), Some("team_members"));
    h.capture(
        "import-01-csv-new-table",
        &[
            "The Import dialog names Team Members.csv and previews its three rows with each column's inferred type (name text, age integer, joined date).",
            "The target is a new table named team_members with an editable name and type for each column.",
        ],
    );
    h.click("import-close");

    let xlsx = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/imports/people.xlsx");
    open_wizard(&mut h, xlsx);
    assert_eq!(h.label("Worksheet"), "People");
    assert_eq!(h.label("Type for active"), "Yes / No");
    h.capture(
        "import-02-xlsx-worksheet",
        &[
            "The dialog shows a Worksheet selector set to People for the XLSX file.",
            "The preview lists the sheet's rows and the active column is typed Yes / No.",
        ],
    );
    h.click("import-close");

    let spec = json!({ "name": "members", "columns": [
        { "name": "id", "declaredType": "INTEGER", "nullable": false, "primaryKeyPosition": 1 },
        { "name": "name", "declaredType": "TEXT", "nullable": false },
        { "name": "age", "declaredType": "INTEGER" },
    ]});
    crate::create_database_table(h.label.clone(), serde_json::from_value(spec).unwrap()).unwrap();
    h.database_changed();
    let people = fixture(&h, "people.csv", "Name,Age,Note\nAda,36,x\n,40,y\nLinus,abc,z\n");
    open_wizard(&mut h, people);
    h.click("import-into-existing");
    assert_eq!(h.label("Field for Name"), "name (required)");
    assert_eq!(h.label("Field for Note"), "Skip this column");
    h.capture(
        "import-03-map-existing-table",
        &[
            "Import into an existing table is selected with members chosen as the target.",
            "Each file column has a field selector: Name maps to name (required), Age to age, and Note is set to Skip this column.",
        ],
    );

    h.click("import-run");
    assert_eq!(h.label("import-summary"), "Imported 1 of 3 rows into members. 2 rows were skipped.");
    assert_eq!(h.label("report-row:1:problem"), "age requires an integer");
    h.capture(
        "import-04-skipped-rows-report",
        &[
            "The summary reads Imported 1 of 3 rows into members. 2 rows were skipped.",
            "The report lists row 2 (name is required) and row 3 (age requires an integer).",
        ],
    );
}
