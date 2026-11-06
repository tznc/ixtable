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
    explain(&e.to_string())
}

/// Separates the plain message from the reader's own words in an import error.
pub const DETAILS: &str = "\n\nDetails: ";

/// Turns a reader error into a message for the user, with the reader's text kept as
/// details after [`DETAILS`].
pub fn explain(raw: &str) -> AppError {
    let lower = raw.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    let plain = |s: &str| {
        let mut c = s.chars();
        c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
    };
    let message = if has(&["has a password", "is encrypted", "larger than 1 gb"]) {
        // Already written for the user.
        return AppError::new("IMPORT_FAILED", plain(raw));
    } else if has(&["not an access", "msisam", "no template/database/objects part"]) {
        "This file is not an Access database or template."
    } else if has(&["os error", "no such file", "permission denied", "is a directory"]) {
        "The file could not be opened."
    } else {
        "The file is damaged or incomplete. Open it in Access, run Compact and Repair, then import it again."
    };
    AppError::new("IMPORT_FAILED", format!("{message}{DETAILS}{raw}"))
}

/// Runs reader work off the UI thread. A panic in the reader means the file has a shape
/// it did not expect, so the user hears that, not the panic text.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| {
        crate::logging::warn("access", &format!("reader panic: {e}"));
        AppError::new(
            "IMPORT_FAILED",
            "The file could not be read. It may be damaged; open it in Access, run Compact and Repair, and try again.",
        )
    })?
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

/// Creates a new untitled document from an Access file. `progress` hears each table as
/// its rows are copied: (Access table name, rows copied so far).
pub async fn import_access_file(
    window_label: String,
    path: String,
    options: Option<crate::access::convert::Options>,
    mut progress: Option<Box<dyn FnMut(&str, u64) + Send>>,
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
            &mut |table, rows| {
                if let Some(report) = progress.as_mut() {
                    report(table, rows);
                }
            },
        )
        .map_err(failed)?;
        let m = crate::manager()?;
        // A panic while the new document is half built must not leave it open.
        let created = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            convert::create(m, &window_label, file.as_ref(), conversion)
        }));
        let (state, report) = match created {
            Ok(r) => r?,
            Err(panic) => {
                let _ = m.close(&window_label, true);
                std::panic::resume_unwind(panic)
            }
        };
        Ok(AccessImport { state, report })
    })
    .await
}

/// Saves the migration report text the Studio renders (Markdown or CSV).
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
