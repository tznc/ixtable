//! Port of tests/unit/export-names.test.ts.

use super::names::{
    default_export_name, exported_message, first_column_filter, safe_file_stem, with_extension,
};
use crate::data::{DataValue, FilterOperator};
use crate::export::ExportFormat;

#[test]
fn strips_characters_invalid_on_any_platform() {
    assert_eq!(safe_file_stem("a<b>c:d\"e/f\\g|h?i*j"), "abcdefghij");
    assert_eq!(safe_file_stem("tab\u{0}le\u{1f}\n"), "table");
}

#[test]
fn trims_and_falls_back_to_export() {
    assert_eq!(safe_file_stem("  Orders 2026. "), "Orders 2026");
    assert_eq!(safe_file_stem(""), "export");
    assert_eq!(safe_file_stem(" /:*?\"<>| "), "export");
    assert_eq!(safe_file_stem("..."), "export");
}

#[test]
fn appends_the_extension_unless_present() {
    assert_eq!(with_extension("out", ExportFormat::Csv), "out.csv");
    assert_eq!(with_extension("out.CSV", ExportFormat::Csv), "out.CSV");
    assert_eq!(with_extension("out.csv", ExportFormat::Json), "out.csv.json");
    assert_eq!(default_export_name("Sales/Q1", ExportFormat::Xlsx), "SalesQ1.xlsx");
}

#[test]
fn formats_the_row_count() {
    assert_eq!(exported_message(1234), "Exported 1,234 rows");
    assert_eq!(exported_message(1), "Exported 1 row");
    assert_eq!(exported_message(0), "Exported 0 rows");
}

#[test]
fn builds_the_first_column_contains_filter() {
    assert!(first_column_filter(Some("name"), "").is_empty());
    assert!(first_column_filter(None, "x").is_empty());
    let filters = first_column_filter(Some("name"), "x");
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0].column, "name");
    assert!(matches!(filters[0].operator, FilterOperator::Contains));
    assert_eq!(filters[0].value, Some(DataValue::Text("x".into())));
}
