//! Microsoft Access import (`docs/decisions/access-import.md`, format notes in
//! `docs/access-format.md`): reads template packages (`.accdt`) and database
//! files (`.accdb`, `.mdb`) into [`model::AccessDb`], then converts them into a
//! new ixtable document.
pub mod accdt;
pub mod blob;
pub mod convert;
pub mod jet;
pub mod model;
pub mod query_def;
pub mod text_format;
pub mod translate;
pub mod xml;

use crate::manager::AppError;
use model::AccessFile;
use std::path::Path;

/// Opens an Access file of any supported kind, by content.
pub fn open(path: &Path) -> Result<Box<dyn AccessFile>, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut magic = [0u8; 4];
    use std::io::{Read, Seek};
    let n = f.read(&mut magic).map_err(|e| e.to_string())?;
    f.rewind().map_err(|e| e.to_string())?;
    if n >= 2 && &magic[..2] == b"PK" {
        return Ok(Box::new(accdt::TemplatePackage::open(f)?));
    }
    Ok(Box::new(jet::JetFile::open(f)?))
}

fn failed(e: impl std::fmt::Display) -> AppError {
    AppError::new("IMPORT_FAILED", e.to_string())
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(failed)?
}

/// One table in the inventory.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableSummary {
    pub name: String,
    pub columns: usize,
    pub rows: Option<u64>,
}

/// What an Access file holds, shown before the import.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub format: model::SourceFormat,
    pub tables: Vec<TableSummary>,
    pub relationships: usize,
    pub queries: Vec<String>,
    pub forms: Vec<String>,
    pub reports: Vec<String>,
    pub macros: Vec<String>,
    pub modules: Vec<String>,
    /// Compiled objects of a binary file as (`kind`, `name`): listed, but not readable.
    pub compiled: Vec<(String, String)>,
    pub warnings: Vec<String>,
}

pub fn inventory(path: &Path) -> Result<Inventory, String> {
    let mut file = open(path)?;
    let compiled = file.compiled_objects();
    let mut db = file.db().clone();
    // Templates know their row counts only by reading the sample data.
    for t in db.tables.iter_mut().filter(|t| t.row_count.is_none()) {
        let mut n = 0u64;
        file.rows(&t.name, &mut |_| {
            n += 1;
            Ok(())
        })?;
        t.row_count = Some(n);
    }
    let names = |v: &[model::DesignObject]| v.iter().map(|o| o.name.clone()).collect::<Vec<_>>();
    Ok(Inventory {
        format: db.format,
        tables: db
            .tables
            .iter()
            .map(|t| TableSummary {
                name: t.name.clone(),
                columns: t.columns.len(),
                rows: t.row_count,
            })
            .collect(),
        relationships: db.relationships.len(),
        queries: db.queries.iter().map(|q| q.name.clone()).collect(),
        forms: names(&db.forms),
        reports: names(&db.reports),
        macros: names(&db.macros),
        modules: db.modules.iter().map(|m| m.name.clone()).collect(),
        compiled,
        warnings: db.warnings.clone(),
    })
}

/// Lists the objects of an Access file (`.accdb`, `.mdb`, `.accdt`).
#[tauri::command]
pub async fn inspect_access_file(
    path: String,
) -> Result<crate::access::Inventory, crate::manager::AppError> {
    blocking(move || inventory(Path::new(&path)).map_err(failed)).await
}

/// The new document and what each Access object became.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessImport {
    pub state: crate::manager::SessionState,
    pub report: convert::report::ImportReport,
}

/// Creates a new untitled document from an Access file.
#[tauri::command]
pub async fn import_access_file(
    window_label: String,
    path: String,
    options: Option<crate::access::convert::Options>,
) -> Result<crate::access::AccessImport, crate::manager::AppError> {
    blocking(move || {
        let path = std::path::PathBuf::from(path);
        let mut file = open(&path).map_err(failed)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let conversion = convert::convert(
            file.as_mut(),
            &name,
            &options.unwrap_or_default(),
            &mut |_, _| {},
        )
        .map_err(failed)?;
        let (state, report) =
            convert::create(crate::manager()?, &window_label, file.as_ref(), conversion)?;
        Ok(AccessImport { state, report })
    })
    .await
}

/// Saves the migration report text the Studio renders (Markdown or CSV).
#[tauri::command]
pub fn write_access_report(
    window_label: String,
    path: String,
    text: String,
) -> Result<(), AppError> {
    crate::manager()?.state(&window_label)?;
    crate::authz::require_unrestricted(&window_label, "export the migration report")?;
    write_report_text(Path::new(&path), &text)
}

/// Writes report text to a `.md`, `.csv` or `.txt` file.
pub fn write_report_text(path: &Path, text: &str) -> Result<(), AppError> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !["md", "csv", "txt"].contains(&ext.as_str()) {
        return Err(AppError::new(
            "VALIDATION_ERROR",
            "the migration report is saved as a .md, .csv or .txt file",
        ));
    }
    std::fs::write(path, text).map_err(|e| AppError::new("IO_ERROR", e))
}

#[cfg(test)]
mod tests;
