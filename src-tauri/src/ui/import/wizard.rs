//! Import wizard (port of `src/import/ImportWizard.tsx`, PRD 3.2): choose a CSV, XLSX,
//! JSON or Parquet file, check the preview and inferred types, map it to a new or
//! existing table and import. Rows are written through the RecordStore; rows that fail
//! a type or constraint check are listed in the report and skipped.
//!
//! Test ids: `import-dialog`, `choose-import-file`, `import-file-name`, `import-header`,
//! `Delimiter`, `Worksheet`, `import-status`, `preview-head:<column>` (label "name type"),
//! `preview-cell:<row>:<column>`, `import-into-new`, `import-into-existing`,
//! `Import table name`, `Existing table`, the target editors' ids (see `targets`),
//! `import-error`, `import-summary`, `import-aborted`, `report-row:<i>:row|field|problem`,
//! `report-more`, `import-run` and `import-close`.

use super::{
    mapping::{
        default_columns, default_mapping, default_primary_key, existing_table_target, name_from_path,
        new_table_target, ImportOptions,
    },
    targets::{FieldMappingEditor, NewTableColumns},
};
use crate::data::TableSchema;
use crate::ui::export::names::thousands;
use crate::import::{
    parse::ParsedFile,
    write::{ImportRequest, RowError},
};
use crate::manager::AppError;
use crate::ui::{
    dialogs::{self, FileFilter},
    prelude::*,
    schema::{
        controls::{check, error_line, input_text, modal_sized, new_input, select, set_input, text_input},
        logical::show_value,
    },
    shell::Shell,
};
use gpui_kit::component::input::{InputEvent, InputState};

const PREVIEW_SHOWN: usize = 10;

const DATA_FILES: &[FileFilter] = &[FileFilter {
    name: "Data files",
    extensions: &["csv", "tsv", "txt", "xlsx", "json", "ndjson", "parquet"],
}];

pub enum ImportEvent {
    /// After every import attempt; the table is none when it failed.
    Imported(Option<String>),
    Close,
}

struct Report {
    table: String,
    total_rows: u64,
    imported: u64,
    failed: u64,
    errors: Vec<RowError>,
    aborted: Option<String>,
}

pub struct ImportWizard {
    shell: Entity<Shell>,
    tables: Vec<TableSchema>,
    path: String,
    options: ImportOptions,
    delimiter: Entity<InputState>,
    preview: Option<ParsedFile>,
    new_table: bool,
    table_name: Entity<InputState>,
    columns: Entity<NewTableColumns>,
    existing: String,
    mapping: Entity<FieldMappingEditor>,
    report: Option<Report>,
    busy: bool,
    error: Option<AppError>,
    _subscriptions: Vec<Subscription>,
}
impl EventEmitter<ImportEvent> for ImportWizard {}

impl ImportWizard {
    pub fn new(shell: Entity<Shell>, tables: Vec<TableSchema>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let delimiter = new_input(window, cx, "Detect", "");
        let table_name = new_input(window, cx, "", "");
        let columns = cx.new(|_| NewTableColumns::new());
        let mapping = cx.new(|_| FieldMappingEditor::new());
        let subscriptions = vec![cx.subscribe_in(&delimiter, window, |this, _, e: &InputEvent, window, cx| {
            if !matches!(e, InputEvent::Change) {
                return;
            }
            let text = input_text(&this.delimiter, cx);
            if text != this.options.delimiter && this.preview.is_some() {
                this.options.delimiter = text;
                this.load(false, window, cx);
            }
        })];
        Self {
            existing: tables.first().map(|t| t.name.clone()).unwrap_or_default(),
            shell,
            tables,
            path: String::new(),
            options: ImportOptions::default(),
            delimiter,
            preview: None,
            new_table: true,
            table_name,
            columns,
            mapping,
            report: None,
            busy: false,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    fn choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pick = dialogs::pick_file(cx, "Import records", DATA_FILES);
        task::after_in(window, cx, pick, |this, path, window, cx| {
            let Some(path) = path else {
                return;
            };
            let path = path.to_string_lossy().into_owned();
            this.options = ImportOptions::default();
            this.path = path;
            cx.notify();
            this.load(true, window, cx);
        });
    }

    fn load(&mut self, rename: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.path.is_empty() {
            return;
        }
        self.busy = true;
        self.error = None;
        self.report = None;
        cx.notify();
        let label = self.shell.read(cx).label().to_string();
        let (path, options) = (self.path.clone(), self.options.clone());
        let existing = self.existing.clone();
        let name = name_from_path(&path);
        let work_path = path.clone();
        task::spawn_call_in(
            window,
            cx,
            move || {
                tauri::async_runtime::block_on(crate::import::preview_import_file(
                    label,
                    work_path,
                    Some(options.clean()),
                ))
            },
            move |this, result, window, cx| {
                this.busy = false;
                match result {
                    Ok(preview) => {
                        if rename {
                            set_input(&this.table_name, &name, window, cx);
                            set_input(&this.delimiter, "", window, cx);
                        }
                        let drafts = default_columns(&preview);
                        let key = default_primary_key(&drafts);
                        this.columns.update(cx, |c, cx| c.set(drafts, key, window, cx));
                        let table = this.tables.iter().find(|t| t.name == existing);
                        let mapping = default_mapping(&preview, table);
                        this.set_mapping(&preview, mapping, cx);
                        this.preview = Some(preview);
                    }
                    Err(e) => {
                        this.preview = None;
                        this.error = Some(e);
                    }
                }
                cx.notify();
            },
        );
    }

    fn set_mapping(&mut self, preview: &ParsedFile, mapping: super::mapping::FieldDrafts, cx: &mut Context<Self>) {
        let columns: Vec<(String, String)> = preview
            .columns
            .iter()
            .map(|c| (c.name.clone(), c.logical_type.to_string()))
            .collect();
        let fields = self
            .tables
            .iter()
            .find(|t| t.name == self.existing)
            .map(|t| t.columns.clone())
            .unwrap_or_default();
        self.mapping.update(cx, |m, cx| m.set(columns, &fields, mapping, cx));
    }

    fn change_existing(&mut self, table: &str, cx: &mut Context<Self>) {
        self.existing = table.to_string();
        if let Some(preview) = self.preview.clone() {
            let mapping = default_mapping(&preview, self.tables.iter().find(|t| t.name == table));
            self.set_mapping(&preview, mapping, cx);
        }
        cx.notify();
    }

    fn change_options(
        &mut self,
        patch: impl FnOnce(&mut ImportOptions),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        patch(&mut self.options);
        self.load(false, window, cx);
    }

    fn run(&mut self, cx: &mut Context<Self>) {
        let target = if self.new_table {
            let drafts = self.columns.read(cx).drafts().to_vec();
            let key = self.columns.read(cx).primary_key().to_string();
            match new_table_target(&input_text(&self.table_name, cx), &drafts, &key) {
                Ok(target) => target,
                Err(message) => {
                    self.error = Some(AppError::new("VALIDATION_ERROR", message));
                    cx.notify();
                    return;
                }
            }
        } else {
            existing_table_target(&self.existing, self.mapping.read(cx).mapping())
        };
        self.busy = true;
        self.error = None;
        cx.notify();
        let label = self.shell.read(cx).label().to_string();
        let request = ImportRequest {
            path: self.path.clone(),
            options: self.options.clean(),
            target,
        };
        task::spawn_call(
            cx,
            move || crate::import::import_now(&label, request),
            |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(report) => {
                        if let Some(state) = &report.state {
                            let state = state.clone();
                            this.shell.update(cx, |s, cx| s.apply_session(&state, cx));
                        }
                        let table = report.table.clone();
                        this.report = Some(Report {
                            table: report.table,
                            total_rows: report.total_rows,
                            imported: report.imported,
                            failed: report.failed,
                            errors: report.errors,
                            aborted: report.aborted,
                        });
                        cx.emit(ImportEvent::Imported(Some(table)));
                    }
                    Err(e) => {
                        this.error = Some(e);
                        // A failed import may still have created the table, so reload either way.
                        cx.emit(ImportEvent::Imported(None));
                    }
                }
                cx.notify();
            },
        );
    }
}

fn badge_text(text: impl IntoElement, cx: &App) -> Div {
    div().text_sm().text_color(cx.theme().muted_foreground).child(text)
}

impl Render for ImportWizard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let busy = self.busy;
        let theme = cx.theme();
        let border = theme.border;
        let file_name = self
            .path
            .rsplit(['\\', '/'])
            .next()
            .filter(|n| !n.is_empty())
            .map_or("CSV, XLSX, JSON, or Parquet".to_string(), String::from);
        let mut body = v_flex().gap_3().child(
            h_flex()
                .gap_3()
                .items_center()
                .child(
                    Button::new("choose-import-file")
                        .outline()
                        .icon(IconName::FolderOpen)
                        .label("Choose file…")
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, window, cx| this.choose(window, cx))),
                )
                .child(badge_text(widgets::text("import-file-name", file_name), cx)),
        );
        if let Some(preview) = self.preview.clone() {
            use crate::data::files::FileFormat;
            let is_csv = preview.format == FileFormat::Csv;
            let is_xlsx = preview.format == FileFormat::Xlsx;
            if is_csv || is_xlsx {
                let header_entity = entity.clone();
                let sheet_entity = entity.clone();
                let sheet = if self.options.sheet.is_empty() {
                    preview.sheets.first().cloned().unwrap_or_default()
                } else {
                    self.options.sheet.clone()
                };
                body = body.child(
                    h_flex()
                        .gap_4()
                        .items_center()
                        .child(check(
                            "import-header",
                            "First row has column names",
                            self.options.header,
                            move |value, window, cx| {
                                header_entity
                                    .update(cx, |this, cx| this.change_options(|o| o.header = value, window, cx))
                            },
                        ))
                        .when(is_csv, |el| {
                            el.child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child("Delimiter")
                                    .child(div().w(px(80.)).child(text_input(&self.delimiter, "Delimiter", "Delimiter"))),
                            )
                        })
                        .when(is_xlsx, |el| {
                            el.child(
                                h_flex().gap_2().items_center().child("Worksheet").child(select(
                                    "Worksheet",
                                    &sheet,
                                    preview.sheets.iter().map(|s| (s.clone(), s.clone())).collect(),
                                    "Choose a worksheet",
                                    move |value, window, cx| {
                                        let value = value.to_string();
                                        sheet_entity
                                            .update(cx, |this, cx| this.change_options(|o| o.sheet = value, window, cx))
                                    },
                                )),
                            )
                        }),
                );
            }
            let shown = PREVIEW_SHOWN.min(preview.rows.len());
            body = body
                .child(badge_text(
                    widgets::text(
                        "import-status",
                        format!("{} rows \u{b7} showing the first {shown}", thousands(preview.total_rows)),
                    ),
                    cx,
                ))
                .child(
                    v_flex()
                        .id("import-preview")
                        .max_h(px(220.))
                        .overflow_scroll()
                        .border_1()
                        .border_color(border)
                        .child(h_flex().children(preview.columns.iter().map(|c| {
                            div()
                                .w(px(150.))
                                .flex_none()
                                .px_2()
                                .py_1()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(widgets::text(
                                    SharedString::from(format!("preview-head:{}", c.name)),
                                    format!("{} {}", c.name, c.logical_type),
                                ))
                        })))
                        .children(preview.rows.iter().take(PREVIEW_SHOWN).enumerate().map(|(r, row)| {
                            h_flex().children(row.iter().enumerate().map(|(j, v)| {
                                div().w(px(150.)).flex_none().px_2().truncate().child(widgets::text(
                                    SharedString::from(format!("preview-cell:{r}:{j}")),
                                    show_value(v),
                                ))
                            }))
                        })),
                );
            let (new_e, existing_e) = (entity.clone(), entity.clone());
            body = body.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child("Import into")
                    .child(
                        Button::new("import-into-new")
                            .outline()
                            .small()
                            .label("A new table")
                            .selected(self.new_table)
                            .disabled(busy)
                            .on_click(move |_, _, cx| new_e.update(cx, |this, cx| {
                                this.new_table = true;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("import-into-existing")
                            .outline()
                            .small()
                            .label("An existing table")
                            .selected(!self.new_table)
                            .disabled(busy || self.tables.is_empty())
                            .on_click(move |_, _, cx| existing_e.update(cx, |this, cx| {
                                this.new_table = false;
                                cx.notify();
                            })),
                    ),
            );
            if self.new_table {
                body = body
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child("Table name")
                            .child(div().w(px(240.)).child(text_input(&self.table_name, "Import table name", "Table name"))),
                    )
                    .child(self.columns.clone());
            } else {
                let pick = entity.clone();
                body = body
                    .child(h_flex().gap_2().items_center().child("Table").child(select(
                        "Existing table",
                        &self.existing,
                        self.tables.iter().map(|t| (t.name.clone(), t.name.clone())).collect(),
                        "Choose a table",
                        move |value, _, cx| {
                            let value = value.to_string();
                            pick.update(cx, |this, cx| this.change_existing(&value, cx));
                        },
                    )))
                    .child(self.mapping.clone());
            }
        }
        if let Some(error) = &self.error {
            body = body.child(error_line("import-error", &format!("{} {}", error.code, error.message), cx));
        }
        if let Some(report) = &self.report {
            let skipped = if report.failed > 0 {
                format!(" {} rows were skipped.", thousands(report.failed))
            } else {
                String::new()
            };
            body = body.child(
                v_flex()
                    .gap_1()
                    .child(widgets::text(
                        "import-summary",
                        format!(
                            "Imported {} of {} rows into {}.{skipped}",
                            thousands(report.imported),
                            thousands(report.total_rows),
                            report.table
                        ),
                    ))
                    .when_some(report.aborted.clone(), |el, aborted| {
                        el.child(div().text_color(cx.theme().danger).child(widgets::text(
                            "import-aborted",
                            format!(
                                "The import stopped after {} rows: {aborted}",
                                thousands(report.total_rows)
                            ),
                        )))
                    })
                    .children(report.errors.iter().enumerate().map(|(i, e)| {
                        h_flex()
                            .gap_3()
                            .child(div().w(px(60.)).child(widgets::text(
                                SharedString::from(format!("report-row:{i}:row")),
                                e.row.to_string(),
                            )))
                            .child(div().w(px(140.)).child(widgets::text(
                                SharedString::from(format!("report-row:{i}:field")),
                                e.column.clone().unwrap_or_else(|| "\u{2014}".into()),
                            )))
                            .child(div().flex_1().child(widgets::text(
                                SharedString::from(format!("report-row:{i}:problem")),
                                e.message.clone(),
                            )))
                    }))
                    .when(report.failed > report.errors.len() as u64, |el| {
                        el.child(widgets::text(
                            "report-more",
                            format!("Only the first {} problems are listed.", report.errors.len()),
                        ))
                    }),
            );
        }
        let done = self.report.is_some();
        body = body.child(
            h_flex()
                .gap_2()
                .justify_end()
                .child(
                    Button::new("import-run")
                        .label(if busy && self.preview.is_some() { "Importing…" } else { "Import" })
                        .disabled(busy || self.preview.is_none() || done)
                        .on_click(cx.listener(|this, _, _, cx| this.run(cx))),
                )
                .child(
                    Button::new("import-close")
                        .outline()
                        .label(if done { "Done" } else { "Cancel" })
                        .disabled(busy)
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(ImportEvent::Close))),
                ),
        );
        modal_sized("import-dialog", "Import records", body, 900.0, cx)
    }
}
