//! Port of tests/integration/access-import.test.tsx (with access-template.ts). The unit
//! tests live beside `summary` and `migration`. Imports are explicit: `gpui_kit::*`
//! would shadow `#[test]`.

use super::migration::{MigrationFilter, StatusFilter, filter_items, item_key, read_migration};
use crate::{
    data::{DataValue, NamedValue},
    ui::{shell::ModeId, testing::Harness},
};
use gpui_kit::{SharedString, TestAppContext};
use std::{io::{Read, Write}, path::{Path, PathBuf}};

fn id(text: &str) -> SharedString {
    SharedString::from(text.to_string())
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/access")
}

/// A new directory under the test state directory.
fn scratch(name: &str) -> PathBuf {
    let dir = crate::paths::state_dir().join(format!("access-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// A gzipped binary fixture, unpacked to a temporary file.
pub(crate) fn binary(name: &str) -> PathBuf {
    let gz = std::fs::read(fixtures().join(format!("{name}.gz"))).unwrap();
    let mut bytes = vec![];
    flate2::read::GzDecoder::new(&gz[..]).read_to_end(&mut bytes).unwrap();
    let path = scratch(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// The template fixture folder packed into an `.accdt` (zip).
pub(crate) fn template() -> PathBuf {
    let out = scratch("Order Desk.accdt");
    let root = fixtures().join("template");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&out).unwrap());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored);
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<_> =
            std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            zip.start_file(name, options).unwrap();
            zip.write_all(&std::fs::read(&path).unwrap()).unwrap();
        }
    }
    zip.finish().unwrap();
    out
}

fn open_wizard(h: &mut Harness) {
    h.click("import-access");
    assert_eq!(h.label("access-wizard"), "Import from Access");
}

fn choose(h: &mut Harness, path: PathBuf) {
    h.dialogs.queue_open(Some(path));
    h.click("access-choose");
}

/// Imports a template and opens the new document.
fn import_template(h: &mut Harness) {
    open_wizard(h);
    choose(h, template());
    assert!(h.exists("access-table:Orders"));
    h.click("access-import");
    assert!(h.exists("access-report"));
    h.click("access-open");
    assert!(h.has_shell());
}

#[gpui_kit::test]
fn shows_what_an_access_file_holds_and_converts_it_into_an_open_document(cx: &mut TestAppContext) {
    let mut h = Harness::start(cx);
    open_wizard(&mut h);
    choose(&mut h, binary("orders.accdb"));
    assert!(h.exists("access-table:Customers"));
    assert!(h.exists("access-table:Orders"));
    assert!(h.label("access-summary").starts_with("Access 2007 or later database"));
    assert_eq!(h.find("access-include-data").checked(), Some(true));

    h.click("access-import");
    assert!(h.label("access-created").starts_with("Created a document with 2 tables"));
    assert!(h.exists("report-row:table"));
    assert!(h.exists("report-row:query"));
    assert!(h.label("attention-toggle").starts_with("What did not convert fully"));

    h.click("access-open");
    assert!(h.has_shell());
    assert!(!h.exists("access-wizard"));
    let orders = h.read_page("Orders");
    assert_eq!(orders.total, 8);
    assert!(orders.columns.iter().any(|c| c.name == "Customer"));
    let customers = h.read_page("Customers");
    assert_eq!(customers.rows[0][0], DataValue::Integer(1));
}

#[gpui_kit::test]
fn imports_only_the_structure_when_the_data_option_is_off(cx: &mut TestAppContext) {
    let mut h = Harness::start(cx);
    open_wizard(&mut h);
    choose(&mut h, binary("orders.mdb"));
    assert!(h.label("access-summary").starts_with("Access 2000–2003 database"));
    h.click("access-include-data");
    h.click("access-import");
    assert!(h.label("access-created").contains("Created a document with 2 tables and 0 rows"));
    h.click("access-open");
    assert_eq!(h.read_page("Orders").total, 0);
}

#[gpui_kit::test]
fn explains_why_a_file_cannot_be_read(cx: &mut TestAppContext) {
    let mut h = Harness::start(cx);
    open_wizard(&mut h);
    let path = scratch("notes.accdb");
    std::fs::write(&path, "not a database").unwrap();
    choose(&mut h, path);
    assert!(h.label("access-error").contains("not an Access database"));
    // Import stays disabled without a readable file.
    h.click("access-import");
    assert!(!h.exists("access-created"));
    h.click("access-cancel");
    assert!(!h.exists("access-wizard"));
    assert!(!h.has_shell());
}

#[gpui_kit::test]
fn imports_a_template_whose_vba_is_viewable_and_whose_action_queries_run(cx: &mut TestAppContext) {
    let mut h = Harness::start(cx);
    import_template(&mut h);

    h.switch_mode(ModeId::App);
    h.click("tab-assets");
    h.click("view-asset:Access VBA.txt");
    assert_eq!(h.label("asset-preview"), "Preview of Access VBA.txt");
    let titles: Vec<String> = (0..12).filter_map(|i| h.try_label(id(&format!("preview-nav:{i}")))).collect();
    assert!(titles.contains(&"Module Helpers".to_string()), "{titles:?}");
    assert!(titles.contains(&"Form Customer Details".to_string()), "{titles:?}");

    let config = crate::read_document_config(h.label.clone()).unwrap();
    let raise = config.saved_queries.iter().find(|q| q.name == "RaiseBigOrders").unwrap();
    let action = raise.action.as_ref().unwrap();
    assert_eq!(action.table, "Orders");
    assert_eq!(format!("{:?}", action.kind), "Update");
    let run = crate::records::run_action_query(
        &h.label,
        &raise.id,
        vec![NamedValue { column: "minimum".into(), value: DataValue::Integer(0) }],
        true,
        0,
    )
    .unwrap();
    assert_eq!(run.changed, h.read_page("Orders").total);
}

#[gpui_kit::test]
fn keeps_a_migration_report_in_settings_with_links_review_marks_and_export(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::start(cx);
    open_wizard(&mut h);
    choose(&mut h, template());
    h.click("access-import");
    assert!(h.label("access-note-about-settings").contains("Settings › Access migration keeps this report"));
    h.click("access-open");
    assert!(h.has_shell());

    h.switch_mode(ModeId::App);
    h.click("tab-access");
    assert!(
        h.label("access-intro").starts_with("Imported from Order Desk.accdt, an Access template"),
        "{}",
        h.label("access-intro")
    );
    let settings = crate::read_document_config(h.label.clone()).unwrap().settings;
    let migration = read_migration(&settings).unwrap();
    let attention = filter_items(&migration, &MigrationFilter::new(StatusFilter::Attention));
    let first = item_key(&attention[0]);
    let before = h.label("access-intro");
    h.click(id(&format!("reviewed:{first}")));
    assert_ne!(h.label("access-intro"), before);
    assert_eq!(h.find(id(&format!("reviewed:{first}"))).checked(), Some(true));

    h.click("access-status:all");
    h.click("access-kind:form");
    let forms = filter_items(
        &migration,
        &MigrationFilter { kind: "form".into(), ..MigrationFilter::new(StatusFilter::All) },
    );
    assert!(!forms.is_empty());
    assert!(h.exists(id(&format!("item:{}", item_key(&forms[0])))));

    let target = scratch("report.md");
    h.dialogs.queue_save(Some(target.clone()));
    h.click("export-md");
    assert_eq!(h.label("access-notice"), "Saved report.md");
    let md = std::fs::read_to_string(&target).unwrap();
    assert!(md.starts_with("# Access migration report: "));
    assert!(md.contains("| Forms |"));
    assert!(md.contains("(reviewed)"));

    let linked = forms.iter().find(|i| i.target.is_some()).expect("a form with a link");
    h.click(id(&format!("open-item:{}", item_key(linked))));
    assert_eq!(h.mode(), ModeId::Design);
}

#[gpui_kit::test]
fn labels_table_counts_and_explains_extra_tables(cx: &mut TestAppContext) {
    let mut h = Harness::start(cx);
    open_wizard(&mut h);
    assert!(h.exists("access-intro"));
    choose(&mut h, binary("orders.accdb"));
    assert_eq!(h.label("access-table:Customers"), "Customers: 6 fields, 5 rows");
    assert!(!h.exists("access-intro"));

    open_complex(&mut h);
    h.click("access-import");
    let created = h.label("access-created");
    assert!(created.starts_with("Created a document with 1 table (plus "), "{created}");
    assert!(created.contains("for attachment fields, multi-value fields or make-table queries"));
    assert!(h.exists("access-unsaved"));
    // Short attention lists open without a click.
    if h.exists("attention-toggle") {
        let key = h.label("attention-toggle");
        assert!(key.starts_with("What did not convert fully"));
    }
    h.click("access-open");
    assert!(h.has_shell());
}

fn open_complex(h: &mut Harness) {
    choose(h, binary("complex-data.accdb"));
    assert!(h.label("access-summary").contains("1 table"));
}

#[gpui_kit::test]
fn shows_the_error_code_apart_from_the_message(cx: &mut TestAppContext) {
    let mut h = Harness::start(cx);
    open_wizard(&mut h);
    let path = scratch("notes.accdb");
    std::fs::write(&path, "not a database").unwrap();
    choose(&mut h, path);
    assert!(!h.label("access-error").contains("IMPORT_FAILED"));
    assert_eq!(h.label("access-error-code"), "Error code IMPORT_FAILED");
}

#[gpui_kit::test]
fn closes_with_the_header_close_button(cx: &mut TestAppContext) {
    let mut h = Harness::start(cx);
    open_wizard(&mut h);
    assert!(h.exists("access-wizard-backdrop"));
    h.click("access-wizard-dismiss");
    assert!(!h.exists("access-wizard"));
}
