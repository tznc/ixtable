//! Tests for the field helpers (masks follow tests/unit/field-mask.test.ts).

use super::{
    apply_mask, field_text, mask_issue, mask_problem, mask_template, masked_text,
    parse_attachments, parse_choices, parse_mask, rich_text_plain,
};
use serde_json::{Value, json};

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
    assert!(parse_mask("0\\;0").slots.len() == 3);
}

#[test]
fn reports_values_that_do_not_fill_the_mask_and_passes_blanks() {
    let text = |s: &str| Value::String(s.into());
    assert_eq!(mask_problem("000-0000", &text(""), "Phone"), None);
    assert_eq!(mask_problem("000-0000", &Value::Null, "Phone"), None);
    assert_eq!(
        mask_problem("000-0000", &text("555"), "Phone").as_deref(),
        Some("Phone must match ___-____.")
    );
    assert_eq!(
        mask_problem("000-0000", &text("555-12x4"), "Phone").as_deref(),
        Some("Phone must match ___-____.")
    );
    assert_eq!(mask_problem("000-0000", &text("5551234"), "Phone"), None);
    assert_eq!(masked_text("000-0000", "555-1234", "Phone").unwrap(), "5551234");
    assert!(masked_text("000-0000", "1", "Phone").unwrap_err().contains("Phone must match"));
    assert!(mask_issue("(--)").unwrap().contains("no characters"));
    assert_eq!(mask_issue("0"), None);
}

#[test]
fn rich_text_reads_as_plain_text() {
    assert_eq!(rich_text_plain(&json!("plain")), "plain");
    assert_eq!(rich_text_plain(&json!("<p>One</p><p>Two &amp; three</p>")), "One\nTwo & three");
    assert_eq!(rich_text_plain(&json!("<b>x</b><script>bad()</script>y")), "xy");
    assert_eq!(rich_text_plain(&Value::Null), "");
}

#[test]
fn formatted_fields_show_as_text() {
    assert_eq!(field_text(&json!("<p>a</p><p>b</p>"), Some("richText")), "a b");
    let files = json!(r#"[{"id":"1","name":"a.pdf","mime":"","size":1,"sha256":""}]"#);
    assert_eq!(field_text(&files, Some("attachment")), "a.pdf");
    assert_eq!(parse_attachments(&files).len(), 1);
    assert_eq!(field_text(&json!(r#"["x","y"]"#), Some("multiSelect")), "x, y");
    assert_eq!(parse_choices(&json!("solo")), ["solo"]);
    assert_eq!(parse_choices(&Value::Null), Vec::<String>::new());
    assert_eq!(field_text(&json!(3), None), "3");
}
