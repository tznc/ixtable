//! Export file names and messages (port of `src/export/names.ts`, `filters.ts`).

use crate::data::{DataValue, Filter, FilterOperator};
use crate::export::ExportFormat;
use crate::ui::dialogs::FileFilter;

pub const EXPORT_FORMATS: [ExportFormat; 3] = [ExportFormat::Csv, ExportFormat::Xlsx, ExportFormat::Json];

/// The file extension of a format ("csv").
pub fn extension(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Csv => "csv",
        ExportFormat::Xlsx => "xlsx",
        ExportFormat::Json => "json",
    }
}

pub fn format_label(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Csv => "CSV",
        ExportFormat::Xlsx => "Excel (.xlsx)",
        ExportFormat::Json => "JSON",
    }
}

pub fn filter_name(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Csv => "CSV files",
        ExportFormat::Xlsx => "Excel workbooks",
        ExportFormat::Json => "JSON files",
    }
}

const CSV: &[FileFilter] = &[FileFilter { name: "CSV files", extensions: &["csv"] }];
const XLSX: &[FileFilter] = &[FileFilter { name: "Excel workbooks", extensions: &["xlsx"] }];
const JSON: &[FileFilter] = &[FileFilter { name: "JSON files", extensions: &["json"] }];

/// The save dialog's file filter for a format.
pub fn dialog_filters(format: ExportFormat) -> &'static [FileFilter] {
    match format {
        ExportFormat::Csv => CSV,
        ExportFormat::Xlsx => XLSX,
        ExportFormat::Json => JSON,
    }
}

const INVALID: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// A file name stem from an object name; "export" when nothing usable is left.
pub fn safe_file_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !INVALID.contains(c) && (*c as u32) >= 32 && *c as u32 != 127)
        .collect();
    // Windows rejects trailing dots and spaces.
    let stem = cleaned.trim().trim_end_matches(['.', ' ']).trim();
    if stem.is_empty() {
        "export".into()
    } else {
        stem.to_string()
    }
}

/// `name` with `.format` appended unless it already ends with it (case-insensitive).
pub fn with_extension(name: &str, format: ExportFormat) -> String {
    let ext = extension(format);
    if name.to_lowercase().ends_with(&format!(".{ext}")) {
        name.to_string()
    } else {
        format!("{name}.{ext}")
    }
}

/// Default file name offered by the save dialog for an object.
pub fn default_export_name(name: &str, format: ExportFormat) -> String {
    with_extension(&safe_file_stem(name), format)
}

/// `1234` as `1,234`.
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "Exported 1,234 rows"
pub fn exported_message(rows: u64) -> String {
    format!("Exported {} {}", thousands(rows), if rows == 1 { "row" } else { "rows" })
}

/// The table browser's "Filter first column" box as a `contains` filter (none when empty).
pub fn first_column_filter(column: Option<&str>, text: &str) -> Vec<Filter> {
    match column {
        Some(column) if !text.is_empty() => vec![Filter {
            column: column.to_string(),
            operator: FilterOperator::Contains,
            value: Some(DataValue::Text(text.to_string())),
            values: None,
        }],
        _ => vec![],
    }
}
