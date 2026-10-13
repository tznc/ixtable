//! Exports the full result of the query shown in the editor. An unmodified saved query
//! exports by id; a draft or edited query exports its current SQL. Port of
//! src/query/QueryExport.tsx and src/export/ExportMenu.tsx.
//!
//! Test ids: `export` (button), `export/csv`, `export/xlsx`, `export/json` (menu),
//! `export-status`, `export-error`.

use super::{
    QueryMode,
    action_text::group_digits,
    choice::Opt,
    sql::{run_params, to_named_values},
};
use crate::{
    archive::SavedQuery,
    export::ExportFormat,
    ui::{
        dialogs::{self, FileFilter},
        prelude::*,
        task, widgets,
    },
};

#[derive(Default)]
pub struct ExportState {
    pub busy: bool,
    pub status: String,
    pub error: String,
}

const CSV: [FileFilter; 1] = [FileFilter { name: "CSV files", extensions: &["csv"] }];
const XLSX: [FileFilter; 1] = [FileFilter { name: "Excel workbooks", extensions: &["xlsx"] }];
const JSON: [FileFilter; 1] = [FileFilter { name: "JSON files", extensions: &["json"] }];

/// (menu id, label, extension, format, save-dialog filter)
const FORMATS: [(&str, &str, &str, ExportFormat, &[FileFilter]); 3] = [
    ("csv", "CSV", "csv", ExportFormat::Csv, &CSV),
    ("xlsx", "Excel (.xlsx)", "xlsx", ExportFormat::Xlsx, &XLSX),
    ("json", "JSON", "json", ExportFormat::Json, &JSON),
];

/// A file name stem from an object name; "export" when nothing usable is left.
pub fn safe_file_stem(name: &str) -> String {
    let kept: String = name
        .chars()
        .filter(|c| !"<>:\"/\\|?*".contains(*c) && !c.is_control())
        .collect();
    let stem = kept.trim().trim_end_matches(['.', ' ']).trim().to_string();
    if stem.is_empty() { "export".into() } else { stem }
}

/// `name` with `.format` appended unless it already ends with it (case-insensitive).
pub fn with_extension(name: &str, format: &str) -> String {
    if name.to_lowercase().ends_with(&format!(".{format}")) {
        name.to_string()
    } else {
        format!("{name}.{format}")
    }
}

/// Default file name offered by the save dialog for an object.
pub fn default_export_name(name: &str, format: &str) -> String {
    with_extension(&safe_file_stem(name), format)
}

/// "Exported 1,234 rows"
pub fn exported_message(rows: u64) -> String {
    format!("Exported {} {}", group_digits(rows), if rows == 1 { "row" } else { "rows" })
}

impl QueryMode {
    pub(super) fn render_export(
        &mut self,
        query: &SavedQuery,
        sql: &str,
        exact_saved: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let options: Vec<Opt> = FORMATS.iter().map(|f| Opt::new(f.0, f.1)).collect();
        let (name, sql) = (query.name.clone(), sql.to_string());
        let button = self
            .choice(
                "export",
                "",
                options,
                move |this, id, window, cx| this.export_as(&id, &name, &sql, exact_saved, window, cx),
                cx,
            )
            .disabled(self.export.busy)
            .labelled("Export…")
            .render(cx);
        h_flex()
            .gap_3()
            .items_center()
            .child(button)
            .when(!self.export.status.is_empty(), |el| {
                el.child(widgets::text("export-status", self.export.status.clone()))
            })
            .when(!self.export.error.is_empty(), |el| {
                el.child(self.error_line("export-error", &self.export.error, cx))
            })
            .into_any_element()
    }

    /// Asks for a destination, then writes the full result there.
    fn export_as(
        &mut self,
        format_id: &str,
        name: &str,
        sql: &str,
        exact_saved: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(&(_, _, ext, format, filters)) = FORMATS.iter().find(|f| f.0 == format_id) else {
            return;
        };
        self.export.status.clear();
        self.export.error.clear();
        let Some(d) = &self.draft else {
            return;
        };
        let (id, parameters) = (d.query.id.clone(), d.query.parameters.clone());
        let params = to_named_values(&run_params(&self.values));
        let (label, sql) = (self.label.clone(), sql.to_string());
        let pick = dialogs::save_file(cx, "Export data", &default_export_name(name, ext), filters);
        task::after_in(window, cx, pick, move |this, path, window, cx| {
            let Some(path) = path else {
                return;
            };
            let path = with_extension(&path.to_string_lossy(), ext);
            this.export.busy = true;
            task::spawn_call_in(
                window,
                cx,
                move || {
                    if exact_saved {
                        tauri::async_runtime::block_on(crate::export::export_saved_query(
                            label, id, params, vec![], vec![], format, path, None,
                        ))
                    } else {
                        tauri::async_runtime::block_on(crate::export::export_sql_query(
                            label,
                            sql,
                            params,
                            Some(parameters),
                            format,
                            path,
                            None,
                        ))
                    }
                },
                |this, result, _, cx| {
                    this.export.busy = false;
                    match result {
                        Ok(summary) => this.export.status = exported_message(summary.rows),
                        Err(e) => this.export.error = e.message,
                    }
                    cx.notify();
                },
            );
            cx.notify();
        });
    }
}
