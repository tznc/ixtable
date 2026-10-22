//! Bundled read-only file sources (port of `src/import/FileSourcesTab.tsx`, PRD 10): a
//! CSV, JSON or Parquet file is stored in the archive as an asset and read by queries,
//! reports and dashboards as `files.<name>`.
//!
//! Test ids: `add-file-source`, `Source name` (input), `source-header` (checkbox),
//! `source-file`, `source-preview`, `confirm-add-source`, `cancel-add-source`, `sources-error`,
//! `sources-empty`, per source `source:<name>` (label "files.name"),
//! `source-format:<name>`, `source-columns:<name>`, `source-rows:<name>`,
//! `Remove <name>`.

use crate::ui::controls::{check, error_line, input_text, new_input, set_input, text_input};
use super::mapping::{name_from_path, ImportOptions};
use crate::data::files::{CsvOptions, FileFormat};
use crate::import::{parse::ParsedFile, FileSource};
use crate::manager::AppError;
use crate::ui::{
    dialogs::{self, FileFilter},
    prelude::*,
    shell::Shell,
    store::StoreEvent,
    toast::Toast,
};
use gpui_kit::component::input::InputState;

const SOURCE_FILES: &[FileFilter] = &[FileFilter {
    name: "Data files",
    extensions: &["csv", "tsv", "txt", "json", "ndjson", "parquet"],
}];

struct Pending {
    path: String,
    options: ImportOptions,
    preview: ParsedFile,
}

struct Detail {
    id: String,
    columns: String,
    rows: Option<u64>,
    error: Option<String>,
}

pub struct FileSourcesTab {
    shell: Entity<Shell>,
    details: Vec<Detail>,
    pending: Option<Pending>,
    name: Entity<InputState>,
    busy: bool,
    error: Option<AppError>,
    _subscriptions: Vec<Subscription>,
}

impl FileSourcesTab {
    pub fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = shell.read(cx).store.clone();
        let name = new_input(window, cx, "", "");
        let subscriptions = vec![
            cx.subscribe(&store, |this, _, e: &StoreEvent, cx| {
                if matches!(e, StoreEvent::Changed) {
                    this.refresh(cx);
                    cx.notify();
                }
            }),
            cx.observe(&name, |_, _, cx| cx.notify()),
        ];
        let mut this = Self {
            shell,
            details: vec![],
            pending: None,
            name,
            busy: false,
            error: None,
            _subscriptions: subscriptions,
        };
        this.refresh(cx);
        this
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let label = self.shell.read(cx).label().to_string();
        task::spawn_call(
            cx,
            move || tauri::async_runtime::block_on(crate::import::file_source_info(label)),
            |this, result, cx| {
                match result {
                    Ok(infos) => {
                        this.details = infos
                            .into_iter()
                            .map(|i| Detail {
                                id: i.id,
                                columns: i.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>().join(", "),
                                rows: i.row_count,
                                error: i.error,
                            })
                            .collect();
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
    }

    fn sources(&self, cx: &App) -> Vec<FileSource> {
        self.shell.read(cx).config(cx).file_sources.clone()
    }

    fn choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pick = dialogs::pick_file(cx, "Add file source", SOURCE_FILES);
        task::after_in(window, cx, pick, |this, path, window, cx| {
            if let Some(path) = path {
                let path = path.to_string_lossy().into_owned();
                let name = name_from_path(&path);
                this.load(path, name, ImportOptions::default(), window, cx);
            }
        });
    }

    fn load(&mut self, path: String, name: String, options: ImportOptions, window: &mut Window, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        cx.notify();
        let label = self.shell.read(cx).label().to_string();
        let (work_path, work_options) = (path.clone(), options.clone());
        task::spawn_call_in(
            window,
            cx,
            move || {
                tauri::async_runtime::block_on(crate::import::preview_import_file(
                    label,
                    work_path,
                    Some(work_options.clean()),
                ))
            },
            move |this, result, window, cx| {
                this.busy = false;
                match result {
                    Ok(preview) if preview.format == FileFormat::Xlsx => {
                        this.error = Some(AppError::new("IMPORT_FAILED", "Import XLSX files into a table instead."));
                    }
                    Ok(preview) => {
                        set_input(&this.name, &name, window, cx);
                        this.pending = Some(Pending { path, options, preview });
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
    }

    fn add(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = &self.pending else {
            return;
        };
        let name = input_text(&self.name, cx);
        self.busy = true;
        self.error = None;
        cx.notify();
        let label = self.shell.read(cx).label().to_string();
        let (path, format) = (pending.path.clone(), pending.preview.format);
        let options = pending.options.clean();
        task::spawn_call(
            cx,
            move || crate::assets::import_asset(label, path, None),
            move |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(imported) => {
                        this.shell.update(cx, |s, cx| s.apply_session(&imported.state, cx));
                        let source = FileSource {
                            id: uuid::Uuid::new_v4().to_string(),
                            name: name.clone(),
                            asset_id: imported.asset.id,
                            format,
                            csv: CsvOptions {
                                header: options.header,
                                delimiter: options.delimiter,
                            },
                        };
                        this.edit(&format!("Add file source {name}"), cx, move |sources| sources.push(source));
                        this.pending = None;
                        this.shell.update(cx, |shell, cx| shell.toast(Toast::success(format!("Added files.{name}.")), cx));
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
    }

    fn confirm_remove(&mut self, source: &FileSource, cx: &mut Context<Self>) {
        let ask = dialogs::confirm(
            cx,
            "Remove file source",
            &format!("Remove files.{}? Queries, reports and dashboards that read it will stop working.", source.name),
        );
        let source = source.clone();
        task::after(cx, ask, move |this, yes, cx| {
            if yes {
                this.remove(&source, cx);
            }
        });
    }

    fn remove(&mut self, source: &FileSource, cx: &mut Context<Self>) {
        let id = source.id.clone();
        self.edit(&format!("Remove file source {}", source.name), cx, move |sources| {
            sources.retain(|s| s.id != id)
        });
        let text = format!(
            "Removed files.{}. Its file stays in Assets until you remove unused assets.",
            source.name
        );
        self.shell.update(cx, |shell, cx| shell.toast(Toast::success(text), cx));
        cx.notify();
    }

    fn edit(&mut self, label: &str, cx: &mut Context<Self>, f: impl FnOnce(&mut Vec<FileSource>)) {
        let store = self.shell.read(cx).store.clone();
        let result = store.update(cx, |s, cx| s.update(label, |c| f(&mut c.file_sources), cx));
        if let Err(e) = result {
            self.error = Some(e);
        }
    }
}

impl Render for FileSourcesTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sources = self.sources(cx);
        let busy = self.busy;
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        let entity = cx.entity();
        let mut panel = v_flex()
            .gap_3()
            .child(widgets::section_heading("tab-title", "File sources"))
            .child(div().ty(Type::Body).text_color(muted).child(
                "CSV, JSON, and Parquet files stored inside this application and read like tables. Query them as files.<name> in saved queries, reports, and dashboards. They are read-only; to edit the rows, import the file into a table instead.",
            ))
            .child(
                h_flex().child(
                    Button::new("add-file-source")
                        .icon(IconName::FolderOpen)
                        .label("Add file source")
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, window, cx| this.choose(window, cx))),
                ),
            );
        if let Some(pending) = &self.pending {
            let header = pending.options.header;
            let is_csv = pending.preview.format == FileFormat::Csv;
            let (path, options) = (pending.path.clone(), pending.options.clone());
            let summary = format!(
                "{} \u{b7} {}",
                widgets::plural(pending.preview.total_rows as usize, "row"),
                pending
                    .preview
                    .columns
                    .iter()
                    .map(|c| format!("{} ({})", c.name, c.logical_type))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let file_name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
            let reload = entity.clone();
            panel = panel.child(
                v_flex()
                    .gap_2()
                    .p_3()
                    .border_1()
                    .border_color(border)
                    .rounded_md()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("New file source"))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child("File")
                            .child(div().ty(Type::Body).text_color(muted).child(widgets::text("source-file", file_name))),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child("Source name")
                            .child(div().w(px(240.)).child(text_input(&self.name, "Source name", "Source name"))),
                    )
                    .when(is_csv, |el| {
                        el.child(check("source-header", "First row has column names", header, move |value, window, cx| {
                            let (path, options) = (path.clone(), options.clone());
                            reload.update(cx, |this, cx| {
                                let name = input_text(&this.name, cx);
                                let options = ImportOptions { header: value, ..options };
                                this.load(path, name, options, window, cx);
                            });
                        }))
                    })
                    .child(div().ty(Type::Body).text_color(muted).child(widgets::text("source-preview", summary)))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("confirm-add-source")
                                    .label("Add source")
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, _, cx| this.add(cx))),
                            )
                            .child(
                                Button::new("cancel-add-source")
                                    .outline()
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.pending = None;
                                        cx.notify();
                                    })),
                            ),
                    ),
            );
        }
        if let Some(error) = &self.error {
            panel = panel.child(error_line("sources-error", &format!("{} {}", error.code, error.message), cx));
        }
        if sources.is_empty() {
            panel = panel.child(widgets::text("sources-empty", "No file sources yet."));
        } else {
            let head = |label: &'static str| div().ty(Type::Label).text_color(muted).child(label);
            panel = panel.child(
                h_flex()
                    .id("sources-header")
                    .gap_4()
                    .items_center()
                    .py_1()
                    .border_b_1()
                    .border_color(border)
                    .child(head("Name").w(px(160.)))
                    .child(head("Format").w(px(80.)))
                    .child(head("Columns").flex_1())
                    .child(head("Rows").w(px(80.)))
                    .child(div().w(px(90.)))
                    .test_support(),
            );
            panel = panel.children(sources.iter().map(|source| {
                let detail = self.details.iter().find(|d| d.id == source.id);
                let name = source.name.clone();
                let columns = match detail {
                    Some(Detail { error: Some(e), .. }) => e.clone(),
                    Some(d) => d.columns.clone(),
                    None => String::new(),
                };
                let rows = detail
                    .and_then(|d| d.rows)
                    .map_or("\u{2014}".to_string(), crate::ui::export::names::thousands);
                let removed = source.clone();
                let format = format!("{:?}", source.format).to_uppercase();
                h_flex()
                    .id(SharedString::from(format!("source:{name}")))
                    .aria_label(format!("files.{name}"))
                    .gap_4()
                    .items_center()
                    .py_1()
                    .border_b_1()
                    .border_color(border)
                    .child(div().w(px(160.)).child(format!("files.{name}")))
                    .child(div().w(px(80.)).child(widgets::text(SharedString::from(format!("source-format:{name}")), format)))
                    .child(div().flex_1().child(widgets::text(SharedString::from(format!("source-columns:{name}")), columns)))
                    .child(div().w(px(80.)).child(widgets::text(SharedString::from(format!("source-rows:{name}")), rows)))
                    .child(
                        Button::new(SharedString::from(format!("Remove {name}")))
                            .danger()
                            .outline()
                            .xsmall()
                            .label("Remove")
                            .tooltip(format!("Remove files.{name}"))
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, _, cx| this.confirm_remove(&removed, cx))),
                    )
                    .test_support()
            }));
        }
        panel
    }
}
