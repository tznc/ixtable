//! Ports of tests/unit/field-mask.test.ts, field-formats.test.ts (the rich text,
//! field value and datasheet parts) and datasheet-fields.test.ts (cell conversion).

use super::{
    datasheet::{datasheet_text, datasheet_value},
    mask::{apply_mask, mask_issue, mask_problem, mask_template, masked_text, parse_mask},
    richtext::{rich_text_plain, sanitize_rich_text},
    values::{
        AttachmentRef, attachments_value, choices_value, field_settings_for, field_text,
        field_text_rows, file_size, parse_attachments, parse_choices,
    },
};
use crate::archive::DocumentConfig;
use crate::data::DataValue;
use crate::ui::schema::testkit::column as col;
use crate::recordstore::{EntitySettings, FieldSettings};
use serde_json::{Map, Value, json};

fn field(id: &str, column: &str) -> FieldSettings {
    FieldSettings {
        id: id.into(),
        column: column.into(),
        ..Default::default()
    }
}

fn entities() -> DocumentConfig {
    let mut config: DocumentConfig = serde_json::from_value(json!({
        "version": 7, "name": "T", "activeMode": "data"
    }))
    .expect("config");
    config.entities = vec![EntitySettings {
        id: "e".into(),
        table: "docs".into(),
        fields: vec![
            FieldSettings {
                format: Some("richText".into()),
                ..field("1", "notes")
            },
            FieldSettings {
                format: Some("attachment".into()),
                ..field("2", "files")
            },
            FieldSettings {
                format: Some("multiSelect".into()),
                options: vec!["red".into(), "blue".into()],
                ..field("3", "tags")
            },
            FieldSettings {
                input_mask: Some("000-0000".into()),
                ..field("4", "phone")
            },
        ],
        ..Default::default()
    }];
    config
}

fn text(value: &str) -> DataValue {
    DataValue::Text(value.into())
}

// ---- field-mask.test.ts ----------------------------------------------------

#[test]
fn formats_typed_digits_and_stores_them_without_literals_by_default() {
    let phone = parse_mask("(000) 000-0000");
    let result = apply_mask(&phone, "5551234567");
    assert_eq!(result.display, "(555) 123-4567");
    assert_eq!(result.stored, "5551234567");
    assert!(result.complete);
    assert_eq!(result.rejected, 0);
    assert_eq!(mask_template(&phone), "(___) ___-____");
}

#[test]
fn stores_literals_with_section_zero_and_uses_the_given_placeholder() {
    let mask = parse_mask("000-0000;0;#");
    assert_eq!(apply_mask(&mask, "5551234").stored, "555-1234");
    assert_eq!(mask_template(&mask), "###-####");
}

#[test]
fn reformats_text_that_already_holds_the_literals() {
    let phone = parse_mask("(000) 000-0000");
    assert_eq!(apply_mask(&phone, "(555) 123-4567").display, "(555) 123-4567");
    assert_eq!(apply_mask(&phone, "555-123-4567").stored, "5551234567");
}

#[test]
fn leaves_trailing_literals_off_a_partial_value() {
    let phone = parse_mask("(000) 000-0000");
    assert_eq!(apply_mask(&phone, "555").display, "(555");
    assert_eq!(apply_mask(&phone, "5551").display, "(555) 1");
    assert!(!apply_mask(&phone, "555").complete);
}

#[test]
fn applies_case_conversion_and_letter_optional_and_escaped_slots() {
    let postcode = parse_mask(">LL0 0LL");
    assert_eq!(apply_mask(&postcode, "sw1a").display, "SW1");
    assert_eq!(apply_mask(&postcode, "sw1 2ab").display, "SW1 2AB");
    let optional = parse_mask("999\\-\"x\"0");
    assert_eq!(apply_mask(&optional, "7").display, "7");
    assert_eq!(apply_mask(&optional, "12-x3").display, "12-x3");
}

#[test]
fn splits_sections_only_outside_quotes_and_escapes() {
    assert!(parse_mask("\"a;b\"0;0").store_literals);
    assert_eq!(parse_mask("\"a;b\"0;0").slots.len(), 4);
    assert_eq!(parse_mask("0\\;0").slots.len(), 3);
}

#[test]
fn reports_values_that_do_not_fill_the_mask_and_passes_blanks() {
    assert_eq!(mask_problem("000-0000", Some(""), "Phone"), None);
    assert_eq!(mask_problem("000-0000", None, "Phone"), None);
    assert_eq!(
        mask_problem("000-0000", Some("555"), "Phone").as_deref(),
        Some("Phone must match ___-____.")
    );
    assert_eq!(
        mask_problem("000-0000", Some("555-12x4"), "Phone").as_deref(),
        Some("Phone must match ___-____.")
    );
    assert_eq!(mask_problem("000-0000", Some("5551234"), "Phone"), None);
    assert_eq!(masked_text("000-0000", "555-1234", "Phone").unwrap(), "5551234");
    assert!(masked_text("000-0000", "1", "Phone")
        .unwrap_err()
        .contains("Phone must match"));
}

#[test]
fn flags_a_mask_with_nothing_to_type_into() {
    assert!(mask_issue("(--)").unwrap().contains("no characters"));
    assert_eq!(mask_issue("0"), None);
}

// ---- field-formats.test.ts -------------------------------------------------

#[test]
fn rich_text_keeps_formatting_tags_and_drops_scripts_handlers_and_unsafe_links() {
    let html = "<p onclick=\"x()\">Hi <b>bold</b> <script>alert(1)</script><a href=\"javascript:x\">j</a> <a href=\"https://e.com\">e</a><img src=x onerror=y></p>";
    assert_eq!(
        sanitize_rich_text(html),
        "<p>Hi <b>bold</b> <a>j</a> <a href=\"https://e.com\" rel=\"noopener noreferrer\" target=\"_blank\">e</a></p>"
    );
    assert_eq!(sanitize_rich_text("<span style=\"x\">t</span>"), "t");
}

#[test]
fn rich_text_reads_as_plain_text_with_block_breaks() {
    assert_eq!(
        rich_text_plain(Some("<p>One</p><ul><li>a</li><li>b</li></ul>")),
        "One\na\nb"
    );
    assert_eq!(rich_text_plain(Some("plain & simple")), "plain & simple");
    assert_eq!(rich_text_plain(None), "");
}

#[test]
fn parses_and_writes_attachment_and_choice_arrays() {
    let file = AttachmentRef {
        id: "f1".into(),
        name: "a.pdf".into(),
        mime: "application/pdf".into(),
        size: 2048,
        sha256: "x".into(),
    };
    let stored = serde_json::to_string(&json!([file, { "name": "no id" }])).unwrap();
    assert_eq!(parse_attachments(&stored), vec![file.clone()]);
    assert!(parse_attachments("not json").is_empty());
    assert_eq!(attachments_value(&[]), None);
    assert_eq!(parse_choices("[\"a\",\"b\"]"), vec!["a", "b"]);
    assert_eq!(parse_choices("legacy"), vec!["legacy"]);
    assert!(parse_choices("[broken").is_empty());
    assert_eq!(choices_value(&[]), None);
    assert_eq!(choices_value(&["a".into()]).as_deref(), Some("[\"a\"]"));
}

#[test]
fn shows_each_format_as_text() {
    let file = json!({ "id": "f1", "name": "a.pdf", "mime": "application/pdf", "size": 2048, "sha256": "x" });
    let one = json!([file]).to_string();
    let two = json!([file, file]).to_string();
    assert_eq!(
        field_text(Some("<p>Hi</p><p>there</p>"), Some("richText")),
        "Hi there"
    );
    assert_eq!(field_text(Some(&one), Some("attachment")), "a.pdf");
    assert_eq!(field_text(Some(&two), Some("attachment")), "2 files");
    assert_eq!(
        field_text(Some("[\"red\",\"blue\"]"), Some("multiSelect")),
        "red, blue"
    );
    assert_eq!(field_text(Some("5"), None), "5");
    assert_eq!(file_size(2048), "2 KB");
    assert_eq!(file_size(20_000_000), "20.0 MB");
}

#[test]
fn finds_field_settings_and_turns_report_rows_into_text() {
    let config = entities();
    assert_eq!(
        field_settings_for(&config, "docs", "phone").and_then(|f| f.input_mask.as_deref()),
        Some("000-0000")
    );
    assert!(field_settings_for(&config, "other", "phone").is_none());
    let row: Map<String, Value> = json!({ "notes": "<b>x</b>", "tags": "[\"red\"]", "id": 1 })
        .as_object()
        .cloned()
        .unwrap();
    let converted = field_text_rows(vec![row.clone()], &config, Some("docs"));
    assert_eq!(
        Value::Object(converted[0].clone()),
        json!({ "notes": "x", "tags": "red", "id": 1 })
    );
    assert_eq!(field_text_rows(vec![row.clone()], &config, None), vec![row]);
}

// ---- field-formats.test.ts (datasheet) -------------------------------------

#[test]
fn datasheet_applies_the_mask_before_the_logical_type_and_shows_formatted_cells_as_text() {
    let config = entities();
    let fields = &config.entities[0].fields;
    let (tags, phone) = (&fields[2], &fields[3]);
    assert_eq!(
        datasheet_value("555-1234", &col("phone", "text"), Some(phone)).unwrap(),
        text("5551234")
    );
    assert!(datasheet_value("12", &col("phone", "text"), Some(phone))
        .unwrap_err()
        .contains("phone must match"));
    assert_eq!(
        datasheet_value("NULL", &col("phone", "text"), Some(phone)).unwrap(),
        DataValue::Null
    );
    assert_eq!(
        datasheet_text(&text("5551234"), Some(phone)).as_deref(),
        Some("555-1234")
    );
    assert_eq!(
        datasheet_text(&text("[\"red\"]"), Some(tags)).as_deref(),
        Some("red")
    );
    assert_eq!(datasheet_text(&text("x"), None), None);
}
