//! Export of a list form's whole source (port of `ListExport.tsx` and a simplified
//! `ExportMenu`): the list's current sort, search and row filter go to a CSV, XLSX or JSON
//! file. Disabled when the row filter cannot run in DuckDB, so an export never holds rows the
//! list hides. The export commands enforce the role's read permission.
//!
//! Test ids: `export` (opens the formats), `export:csv`, `export:xlsx`, `export:json`,
//! `export-status` (the result line) and `export-disabled` (why exporting is unavailable).

use crate::data::{Filter, NamedValue, Sort};
use crate::design::{Form, SourceKind};
use crate::export::{ExportFormat, export_saved_query, export_table};
use crate::forms::data::{export_filters, source_params};
use crate::forms::values::to_data_value;
use crate::ui::dialogs::{self, FileFilter};
use crate::ui::prelude::*;
use serde_json::{Map, Value};

const CSV: &[FileFilter] = &[FileFilter { name: "CSV", extensions: &["csv"] }];
const XLSX: &[FileFilter] = &[FileFilter { name: "Excel workbook", extensions: &["xlsx"] }];
const JSON: &[FileFilter] = &[FileFilter { name: "JSON", extensions: &["json"] }];

fn spec(format: ExportFormat) -> (&'static str, &'static str, &'static [FileFilter]) {
    match format {
        ExportFormat::Csv => ("csv", "CSV", CSV),
        ExportFormat::Xlsx => ("xlsx", "Excel", XLSX),
        ExportFormat::Json => ("json", "JSON", JSON),
    }
}

/// The export control of a list.
pub struct ExportMenu {
    label: String,
    form: Option<Form>,
    sorts: Vec<Sort>,
    filters: Vec<Filter>,
    scope: Map<String, Value>,
    /// Row filters as DuckDB filters: None while unknown, Some(None) when they cannot be
    /// applied by an export.
    row_filters: Option<Option<Vec<Filter>>>,
    key: String,
    pending_check: bool,
    open: bool,
    status: Option<String>,
}

impl ExportMenu {
    pub fn new(label: String) -> Self {
        Self {
            label,
            form: None,
            sorts: vec![],
            filters: vec![],
            scope: Map::new(),
            row_filters: None,
            key: String::new(),
            pending_check: false,
            open: false,
            status: None,
        }
    }

    /// Takes the list's current form, order, search and scope; re-checks the row filter
    /// when the form or scope changed.
    pub fn configure(&mut self, form: Form, sorts: Vec<Sort>, filters: Vec<Filter>, scope: Map<String, Value>) {
        let key = serde_json::json!([&form, &scope]).to_string();
        let changed = key != self.key;
        self.key = key;
        self.form = Some(form);
        self.sorts = sorts;
        self.filters = filters;
        self.scope = scope;
        self.pending_check = changed;
    }
}

impl ExportMenu {
    /// True once the row filter was checked and exporting can proceed.
    pub fn ready(&self) -> bool {
        matches!(self.row_filters, Some(Some(_)))
    }

    fn check(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.form.clone() else { return };
        self.row_filters = None;
        let (label, scope) = (self.label.clone(), self.scope.clone());
        let key = self.key.clone();
        task::spawn_call(
            cx,
            move || export_filters(&label, &form, &scope),
            move |this, found, cx| {
                if this.key == key {
                    this.row_filters = Some(found);
                    cx.notify();
                }
            },
        );
    }

    fn export(&mut self, format: ExportFormat, cx: &mut Context<Self>) {
        let (Some(form), Some(Some(row_filters))) = (self.form.clone(), self.row_filters.clone()) else {
            return;
        };
        self.open = false;
        let (_, _, filters) = spec(format);
        let (label, sorts, scope) = (self.label.clone(), self.sorts.clone(), self.scope.clone());
        let mut all = self.filters.clone();
        all.extend(row_filters);
        let default_name = format!("{}.{}", form.name, spec(format).0);
        let picked = dialogs::save_file(cx, "Export", &default_name, filters);
        task::after(cx, picked, move |this, path, cx| {
            let Some(path) = path else { return };
            this.status = Some("Exporting…".into());
            cx.notify();
            let path = path.to_string_lossy().to_string();
            task::spawn_call(
                cx,
                move || run(&label, &form, &sorts, all, &scope, format, path),
                |this, result, cx| {
                    this.status = Some(match result {
                        Ok(summary) => summary,
                        Err(message) => format!("Export failed: {message}"),
                    });
                    cx.notify();
                },
            );
        });
    }
}

fn run(
    label: &str,
    form: &Form,
    sorts: &[Sort],
    filters: Vec<Filter>,
    scope: &Map<String, Value>,
    format: ExportFormat,
    path: String,
) -> Result<String, String> {
    let source = form.source.as_ref().ok_or("This list has no source.")?;
    let summary = match source.kind {
        SourceKind::Table => {
            let table = source.table.clone().unwrap_or_default();
            tauri::async_runtime::block_on(export_table(
                label.to_string(),
                table,
                sorts.to_vec(),
                filters,
                format,
                path,
                None,
            ))
        }
        SourceKind::Query => {
            let bound = source_params(form, scope)?;
            let params: Vec<NamedValue> = bound
                .iter()
                .map(|(column, value)| NamedValue { column: column.clone(), value: to_data_value(value) })
                .collect();
            tauri::async_runtime::block_on(export_saved_query(
                label.to_string(),
                source.query_id.clone().unwrap_or_default(),
                params,
                sorts.to_vec(),
                filters,
                format,
                path,
                None,
            ))
        }
    }
    .map_err(|e| e.message)?;
    Ok(format!("Exported {} rows to {}", summary.rows, summary.path))
}

impl Render for ExportMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.pending_check {
            self.pending_check = false;
            self.check(cx);
        }
        let has_source = self.form.as_ref().and_then(|f| f.source.as_ref()).is_some_and(|s| {
            s.table.as_deref().is_some_and(|t| !t.is_empty())
                || s.query_id.as_deref().is_some_and(|q| !q.is_empty())
        });
        if !has_source {
            return div().into_any_element();
        }
        let ready = self.ready();
        let blocked = matches!(self.row_filters, Some(None));
        h_flex()
            .gap_1()
            .items_center()
            .child(
                Button::new("export")
                    .small()
                    .label("Export")
                    .disabled(!ready)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open = !this.open;
                        cx.notify();
                    })),
            )
            .when(self.open, |d| {
                d.children([ExportFormat::Csv, ExportFormat::Xlsx, ExportFormat::Json].map(|format| {
                    let (key, label, _) = spec(format);
                    Button::new(SharedString::from(format!("export:{key}")))
                        .small()
                        .outline()
                        .label(label)
                        .on_click(cx.listener(move |this, _, _, cx| this.export(format, cx)))
                }))
            })
            .when(blocked, |d| {
                d.child(widgets::text("export-disabled", "This list's filter can't be applied to an export"))
            })
            .children(self.status.clone().map(|s| widgets::text("export-status", s)))
            .into_any_element()
    }
}
