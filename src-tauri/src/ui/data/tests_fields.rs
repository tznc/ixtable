//! Field formats and masks in Data mode, and saved-query metadata (ports of the Data
//! parts of tests/integration/field-formats.test.tsx and sql-and-metadata.test.tsx).
//! The generated-form and Runtime halves of field-formats belong to the Design and
//! Runtime ports. Imports are explicit: `gpui_kit::*` would shadow `#[test]`.

use super::testkit::{data_mode, sheet};
use crate::{data::DataValue, ui::testing::Harness};
use gpui_kit::TestAppContext;
use serde_json::json;

fn docs(h: &mut Harness) {
    h.create_table(
        "docs",
        &[("id", "INTEGER", 1), ("phone", "TEXT", 0), ("notes", "TEXT", 0), ("tags", "TEXT", 0), ("files", "TEXT", 0)],
    );
}

fn docs_fields(h: &Harness) -> Vec<crate::recordstore::FieldSettings> {
    h.config().entities.iter().find(|e| e.table == "docs").map(|e| e.fields.clone()).unwrap_or_default()
}

fn field_of(fields: &[crate::recordstore::FieldSettings], column: &str) -> Option<crate::recordstore::FieldSettings> {
    fields.iter().find(|f| f.column == column).cloned()
}

fn open_designer(h: &mut Harness) {
    h.click("table:docs");
    h.click("design-table");
    assert!(h.exists("notes format"));
}

#[gpui_kit::test]
fn stores_attachment_files_in_the_record_store_and_reads_them_back_by_id(cx: &mut TestAppContext) {
    use base64::Engine as _;
    let mut h = Harness::new_document(cx);
    docs(&mut h);
    let mut config = h.config();
    config.entities.push(
        serde_json::from_value(json!({
            "id": "e1", "table": "docs", "concurrency": "optimistic",
            "fields": [{ "id": "f1", "column": "files", "format": "attachment" }],
        }))
        .unwrap(),
    );
    crate::update_document_config(h.label.clone(), config).expect("update config");
    let upload = |column: &str| {
        crate::record_attachments::upload_record_attachment(
            h.label.clone(),
            "docs".into(),
            column.into(),
            "../notes.txt".into(),
            None,
            base64::engine::general_purpose::STANDARD.encode("hello"),
        )
    };
    let file = upload("files").expect("upload");
    assert_eq!((file.name.as_str(), file.mime.as_str(), file.size), ("notes.txt", "text/plain", 5));
    let content = crate::record_attachments::read_record_attachment(h.label.clone(), file.id.clone()).unwrap();
    let bytes = base64::engine::general_purpose::STANDARD.decode(&content.content_base64).unwrap();
    assert_eq!(bytes, b"hello");
    let rejected = upload("notes").unwrap_err();
    assert!(rejected.message.contains("notes is not an attachment field"), "{}", rejected.message);
    assert_eq!(crate::record_attachments::remove_unused_record_attachments(h.label.clone()).unwrap(), 0);
}

#[gpui_kit::test]
fn sets_field_formats_in_the_table_designer(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    docs(&mut h);
    open_designer(&mut h);
    h.pick("notes format", "Rich text");
    h.pick("tags format", "Multiple choices");
    h.pick("files format", "Attachments");
    h.append("phone input mask", "(000) 000-0000");

    let mode = data_mode(&mut h);
    let designer = h.cx.read(|cx| mode.read(cx).designer().cloned()).expect("designer");
    let editor = h.cx.read(|cx| designer.read(cx).field_settings().clone());
    h.update(|window, cx| editor.update(cx, |e, cx| e.set_choices("tags", "red\nblue", window, cx)));
    h.settle();

    let fields = docs_fields(&h);
    assert_eq!(field_of(&fields, "notes").unwrap().format.as_deref(), Some("richText"));
    assert_eq!(field_of(&fields, "tags").unwrap().format.as_deref(), Some("multiSelect"));
    assert_eq!(field_of(&fields, "files").unwrap().format.as_deref(), Some("attachment"));
    assert_eq!(field_of(&fields, "phone").unwrap().input_mask.as_deref(), Some("(000) 000-0000"));
    assert_eq!(field_of(&fields, "tags").unwrap().options, ["red", "blue"]);
}

#[gpui_kit::test]
fn rejects_a_typed_value_that_does_not_fill_the_input_mask(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    docs(&mut h);
    open_designer(&mut h);
    h.append("phone input mask", "000-0000");
    assert_eq!(field_of(&docs_fields(&h), "phone").unwrap().input_mask.as_deref(), Some("000-0000"));
    h.go("close-designer");
    h.append("New phone", "12");
    h.press("enter");
    assert_eq!(h.label("draft-error"), "phone must match ___-____.");
    h.enter("New phone", "5551234");
    h.press("enter");
    assert_eq!(h.read_page("docs").rows.len(), 1);
    assert_eq!(h.read_page("docs").rows[0][1], DataValue::Text("5551234".into()));
}

#[gpui_kit::test]
fn shows_masked_and_formatted_fields_in_the_datasheet_and_edits_through_the_mask(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    docs(&mut h);
    open_designer(&mut h);
    h.pick("tags format", "Multiple choices");
    h.append("phone input mask", "000-0000");
    h.go("close-designer");
    h.insert_row(
        "docs",
        &[
            ("id", DataValue::Integer(1)),
            ("phone", DataValue::Text("5551234".into())),
            ("tags", DataValue::Text("[\"red\",\"blue\"]".into())),
        ],
    );
    h.click("Refresh");
    assert_eq!(h.label("cell:0:1"), "555-1234");
    assert_eq!(h.label("cell:0:3"), "red, blue");
    let s = sheet(&mut h);
    assert!(!h.cx.read(|cx| s.read(cx).editable(3)), "multiple choices are edited in a form");
    assert!(!h.exists("New tags"));

    h.click("cell:0:1");
    h.enter("cell-input", "555-9876");
    h.press("enter");
    assert_eq!(h.read_page("docs").rows[0][1], DataValue::Text("5559876".into()));
}

// sql-and-metadata: "persists saved-query metadata through public commands".
#[gpui_kit::test]
fn persists_saved_query_metadata_through_public_commands(cx: &mut TestAppContext) {
    let h = Harness::new_document(cx);
    let saved =
        crate::save_query(h.label.clone(), None, "All values".into(), "SELECT 42 AS value".into(), None).unwrap();
    assert_eq!((saved.saved_queries[0].name.as_str(), saved.saved_queries[0].sql.as_str()), ("All values", "SELECT 42 AS value"));
    let id = saved.saved_queries[0].id.clone();
    crate::save_query(h.label.clone(), Some(id.clone()), "Renamed".into(), "SELECT 43 AS value".into(), None).unwrap();
    let reloaded = crate::read_document_config(h.label.clone()).unwrap();
    assert_eq!(reloaded.saved_queries.len(), 1);
    assert_eq!((reloaded.saved_queries[0].id.clone(), reloaded.saved_queries[0].name.as_str()), (id.clone(), "Renamed"));
    let deleted = crate::delete_saved_query(h.label.clone(), id).unwrap();
    assert!(deleted.saved_queries.is_empty());
}
