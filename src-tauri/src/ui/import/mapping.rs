//! Import drafts and targets (port of `src/import/mapping.ts`).

use crate::data::TableSchema;
use crate::import::{
    parse::{ParseOptions, ParsedFile},
    write::{FieldMapping, ImportTarget, NewColumn},
};

/// A new-table column draft: `include` false leaves the source column out.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnDraft {
    pub source: String,
    pub name: String,
    pub logical_type: String,
    pub include: bool,
}

/// Source column name to target field name ("" skips the column), in file order.
pub type FieldDrafts = Vec<(String, String)>;

/// The file options as the wizard edits them: an empty delimiter or sheet means
/// "detect" and "the first one".
#[derive(Clone, Debug, PartialEq)]
pub struct ImportOptions {
    pub header: bool,
    pub delimiter: String,
    pub sheet: String,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            header: true,
            delimiter: String::new(),
            sheet: String::new(),
        }
    }
}

impl ImportOptions {
    /// Options as sent to the backend.
    pub fn clean(&self) -> ParseOptions {
        ParseOptions {
            format: None,
            header: self.header,
            delimiter: (!self.delimiter.is_empty()).then(|| self.delimiter.clone()),
            sheet: (!self.sheet.is_empty()).then(|| self.sheet.clone()),
        }
    }
}

/// An identifier safe to use unquoted: lower-case letters, digits and `_`.
pub fn identifier(raw: &str, fallback: &str) -> String {
    let mut cleaned = String::new();
    let mut in_run = false;
    for c in raw.trim().to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' {
            cleaned.push(c);
            in_run = false;
        } else if !in_run {
            // A run of other characters is one underscore.
            cleaned.push('_');
            in_run = true;
        }
    }
    let cleaned = cleaned.trim_matches('_').to_string();
    if cleaned.is_empty() {
        return fallback.to_string();
    }
    let starts_ok = cleaned.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c == '_');
    let named = if starts_ok { cleaned } else { format!("_{cleaned}") };
    named.chars().take(63).collect()
}

/// The file's base name without its extension, as an identifier.
pub fn name_from_path(path: &str) -> String {
    let base = path.rsplit(['\\', '/']).next().unwrap_or("");
    let stem = match base.rfind('.') {
        Some(at) if at + 1 < base.len() => &base[..at],
        _ => base,
    };
    identifier(stem, "imported")
}

pub fn default_columns(preview: &ParsedFile) -> Vec<ColumnDraft> {
    let mut used: Vec<String> = Vec::new();
    preview
        .columns
        .iter()
        .enumerate()
        .map(|(i, column)| {
            let base = identifier(&column.name, &format!("column_{}", i + 1));
            let mut name = base.clone();
            let mut n = 2;
            while used.contains(&name) {
                name = format!("{base}_{n}");
                n += 1;
            }
            used.push(name.clone());
            ColumnDraft {
                source: column.name.clone(),
                name,
                logical_type: column.logical_type.to_string(),
                include: true,
            }
        })
        .collect()
}

/// The draft named `id`, which becomes the key instead of a generated one.
pub fn default_primary_key(columns: &[ColumnDraft]) -> String {
    columns
        .iter()
        .find(|c| c.include && c.name == "id")
        .map(|c| c.name.clone())
        .unwrap_or_default()
}

/// Maps each source column to the field with the same name (ignoring case and punctuation).
pub fn default_mapping(preview: &ParsedFile, table: Option<&TableSchema>) -> FieldDrafts {
    let fields: Vec<&str> = table
        .map(|t| t.columns.iter().filter(|c| !c.generated).map(|c| c.name.as_str()).collect())
        .unwrap_or_default();
    preview
        .columns
        .iter()
        .map(|column| {
            let wanted = identifier(&column.name, "");
            let field = fields.iter().find(|f| identifier(f, "") == wanted);
            (column.name.clone(), field.map(|f| f.to_string()).unwrap_or_default())
        })
        .collect()
}

pub fn new_table_target(table: &str, columns: &[ColumnDraft], primary_key: &str) -> Result<ImportTarget, String> {
    let columns = columns
        .iter()
        .filter(|c| c.include)
        .map(|c| {
            Ok(NewColumn {
                source: c.source.clone(),
                name: c.name.clone(),
                logical_type: c
                    .logical_type
                    .parse()
                    .map_err(|_| format!("{} is not a column type", c.logical_type))?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(ImportTarget::NewTable {
        table: table.to_string(),
        columns,
        primary_key: (!primary_key.is_empty()).then(|| primary_key.to_string()),
    })
}

pub fn existing_table_target(table: &str, mapping: &FieldDrafts) -> ImportTarget {
    ImportTarget::ExistingTable {
        table: table.to_string(),
        mapping: mapping
            .iter()
            .filter(|(_, field)| !field.is_empty())
            .map(|(source, field)| FieldMapping {
                source: source.clone(),
                field: field.clone(),
            })
            .collect(),
    }
}
