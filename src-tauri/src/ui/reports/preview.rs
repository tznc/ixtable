//! Paginated preview of a saved report with page navigation, zoom and PDF export (port of
//! `ReportPreview.tsx`). The Runtime and dashboards embed it too.
//!
//! Loading and layout run on a worker (`report_engine::data::load_report_data`,
//! `layout_report`); a change to the report, the saved queries or the other reports
//! starts a new load and cancels the one running. After `PROGRESS_DELAY_MS` the loading
//! line shows the elapsed seconds and a Cancel button.
//!
//! Print writes the same PDF as Export PDF to a file chosen in the save dialog: the
//! native window has no print pipeline of its own.
//!
//! Test ids: toolbar `prev-page`, `next-page`, `page-status` ("Page 1 of 2"), `zoom`
//! (a choice), `print`, `export-pdf`; message `preview-error`;
//! loading `report-loading`, `loading-seconds`, `cancel-load`, `load-cancelled`,
//! `retry-load`, `load-error`; `truncated`; `diagnostics-toggle` and `diagnostic-N`;
//! the page is `page_view`'s `report-page`.

use crate::ui::controls::choice;
use super::page_view::{page_image, page_view, PageImages};
use crate::manager::AppError;
use crate::report_engine::{
    data::{load_report_data, LoadControl, ReportData, REPORT_ROW_LIMIT},
    layout_report,
    pdf::{report_pdf_bytes, PdfAsset, PdfOptions},
    AssetInfo, LayoutOptions, ReportDocument,
};
use crate::ui::{dialogs, prelude::*, shell::Shell};
use base64::Engine as _;
use serde_json::{Map, Value};
use std::{collections::HashMap, time::Duration};

/// Progress and Cancel appear once loading takes this long (PRD §27.3).
pub const PROGRESS_DELAY_MS: u64 = 2000;
const TICK_MS: u64 = 250;
pub const ZOOMS: [f32; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];

static PDF_FILTERS: [dialogs::FileFilter; 1] = [dialogs::FileFilter {
    name: "PDF document",
    extensions: &["pdf"],
}];

/// A finished run: the laid-out document and what it was laid out from.
pub struct Loaded {
    pub doc: ReportDocument,
    pub data: ReportData,
    /// ISO timestamp of the run; the PDF's creation date.
    pub generated_at: String,
}

pub enum Status {
    Loading,
    Ready(Box<Loaded>),
    Cancelled,
    Error(String),
}

pub struct ReportPreview {
    shell: Entity<Shell>,
    report_id: String,
    /// Parameter values; they override the report's defaults.
    params: Option<Map<String, Value>>,
    actions: bool,
    pub status: Status,
    // The inputs the current run was started for.
    key: String,
    generation: u64,
    control: Option<LoadControl>,
    elapsed_ms: u64,
    pub page_index: usize,
    pub zoom: f32,
    pub error: String,
    diagnostics_open: bool,
    images: PageImages,
    ticker: Option<Task<()>>,
    _subscription: Subscription,
}

impl ReportPreview {
    pub fn new(
        shell: Entity<Shell>,
        report_id: &str,
        params: Option<Map<String, Value>>,
        actions: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
        Self {
            shell,
            report_id: report_id.to_string(),
            params,
            actions,
            status: Status::Loading,
            key: String::new(),
            generation: 0,
            control: None,
            elapsed_ms: 0,
            page_index: 0,
            zoom: 1.0,
            error: String::new(),
            diagnostics_open: false,
            images: HashMap::new(),
            ticker: None,
            _subscription: subscription,
        }
    }

    /// The laid-out document, once the run finished.
    pub fn document(&self) -> Option<&ReportDocument> {
        match &self.status {
            Status::Ready(loaded) => Some(&loaded.doc),
            _ => None,
        }
    }

    pub fn is_loading(&self) -> bool {
        matches!(self.status, Status::Loading)
    }

    /// Runs the report again with other parameter values.
    pub fn set_params(&mut self, params: Option<Map<String, Value>>, cx: &mut Context<Self>) {
        if self.params != params {
            self.params = params;
            cx.notify();
        }
    }

    /// The text of every line of the shown page.
    pub fn page_texts(&self) -> Vec<String> {
        let Some(doc) = self.document() else {
            return vec![];
        };
        let index = self.page_index.min(doc.pages.len().saturating_sub(1));
        doc.pages
            .get(index)
            .map(|page| {
                page.items
                    .iter()
                    .flat_map(|item| match item {
                        crate::report_engine::Item::Text(t) => t.lines.iter().map(|l| l.text.clone()).collect(),
                        _ => vec![],
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn input_key(&self, cx: &App) -> Option<String> {
        let config = self.shell.read(cx).config(cx);
        let report = config.reports.iter().find(|r| r.id == self.report_id)?;
        serde_json::to_string(&(report, &config.saved_queries, &config.reports, &self.params)).ok()
    }

    /// Starts a run, replacing the one in progress.
    fn start(&mut self, cx: &mut Context<Self>) {
        if let Some(old) = self.control.take() {
            old.cancel();
        }
        let config = self.shell.read(cx).config(cx).clone();
        let Some(report) = config.reports.iter().find(|r| r.id == self.report_id).cloned() else {
            return;
        };
        let label = self.shell.read(cx).label().to_string();
        let mut values: Map<String, Value> = report.params.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (k, v) in self.params.iter().flatten() {
            values.insert(k.clone(), v.clone());
        }
        let control = LoadControl::new(&label);
        self.control = Some(control.clone());
        self.generation += 1;
        let generation = self.generation;
        self.status = Status::Loading;
        self.elapsed_ms = 0;
        self.start_ticker(cx);
        task::spawn_call(
            cx,
            move || run_report(&label, &report, &config, values, &control),
            move |this, result, cx| {
                if this.generation != generation {
                    return;
                }
                match result {
                    Ok(loaded) => {
                        this.images = loaded
                            .data
                            .assets
                            .iter()
                            .filter_map(|(id, a)| Some((id.clone(), page_image(&a.media_type, &a.data)?)))
                            .collect();
                        let last = loaded.doc.pages.len().saturating_sub(1);
                        this.page_index = this.page_index.min(last);
                        this.status = Status::Ready(Box::new(loaded));
                    }
                    Err(e) if e.code == "CANCELLED" => this.status = Status::Cancelled,
                    Err(e) => this.status = Status::Error(e.message),
                }
                cx.notify();
            },
        );
    }

    fn start_ticker(&mut self, cx: &mut Context<Self>) {
        self.ticker = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(TICK_MS)).await;
            let again = this.update(cx, |this, cx| {
                let loading = this.is_loading();
                if loading {
                    this.elapsed_ms += TICK_MS;
                    cx.notify();
                }
                loading
            });
            if !matches!(again, Ok(true)) {
                break;
            }
        }));
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        if let Some(control) = &self.control {
            control.cancel();
        }
        self.generation += 1;
        self.status = Status::Cancelled;
        cx.notify();
    }

    fn go(&mut self, index: usize, cx: &mut Context<Self>) {
        self.page_index = index;
        cx.notify();
    }

    /// Writes the report as a PDF to a file chosen in the save dialog.
    pub fn export_pdf(&mut self, cx: &mut Context<Self>) {
        let Status::Ready(loaded) = &self.status else {
            return;
        };
        let doc = loaded.doc.clone();
        let generated_at = loaded.generated_at.clone();
        let assets: HashMap<String, PdfAsset> = loaded
            .data
            .assets
            .iter()
            .map(|(id, a)| {
                (
                    id.clone(),
                    PdfAsset {
                        media_type: a.media_type.clone(),
                        data: a.data.clone(),
                    },
                )
            })
            .collect();
        let config = self.shell.read(cx).config(cx);
        let name = config
            .reports
            .iter()
            .find(|r| r.id == self.report_id)
            .map_or_else(|| "Report".to_string(), |r| r.name.clone());
        let (label, report_id) = (self.shell.read(cx).label().to_string(), self.report_id.clone());
        self.error.clear();
        cx.notify();
        let pick = dialogs::save_file(cx, "Export PDF", &format!("{name}.pdf"), &PDF_FILTERS);
        task::after(cx, pick, move |_, path, cx| {
            let Some(path) = path else {
                return;
            };
            let path = path.to_string_lossy().into_owned();
            let target = path.clone();
            task::spawn_call(
                cx,
                move || write_pdf_file(&label, &report_id, &name, &doc, &assets, &generated_at, &target),
                move |this, result, cx| {
                    match result {
                        Ok(warnings) => {
                            let placeholders = if warnings.is_empty() {
                                String::new()
                            } else {
                                format!(". Printed as placeholders: {}", warnings.join("; "))
                            };
                            let file = std::path::Path::new(&path)
                                .file_name()
                                .map_or_else(|| path.clone(), |f| f.to_string_lossy().into_owned());
                            let text = format!("Exported {file}{placeholders}");
                            this.shell.update(cx, |shell, cx| shell.toast(crate::ui::toast::Toast::success(text), cx));
                        }
                        Err(e) => this.error = e.message,
                    }
                    cx.notify();
                },
            );
        });
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (total, index) = match self.document() {
            Some(doc) => (doc.pages.len(), self.page_index.min(doc.pages.len().saturating_sub(1))),
            None => (0, 0),
        };
        let ready = self.document().is_some();
        let status = if ready { format!("Page {} of {total}", index + 1) } else { "No pages".to_string() };
        let entity = cx.entity();
        let zoom_options: Vec<(String, String)> = ZOOMS
            .iter()
            .map(|z| (z.to_string(), format!("{}%", (z * 100.0).round())))
            .collect();
        let zoom_entity = entity.clone();
        h_flex()
            .id("report-preview-toolbar")
            .gap_2()
            .items_center()
            .px_3()
            .py_1p5()
            .child(
                Button::new("prev-page")
                    .ghost()
                    .small()
                    .icon(IconName::ChevronLeft)
                    .tooltip("Previous page")
                    .disabled(!ready || index == 0)
                    .on_click(cx.listener(move |this, _, _, cx| this.go(index.saturating_sub(1), cx))),
            )
            .child(widgets::text("page-status", status))
            .child(
                Button::new("next-page")
                    .ghost()
                    .small()
                    .icon(IconName::ChevronRight)
                    .tooltip("Next page")
                    .disabled(!ready || index + 1 >= total)
                    .on_click(cx.listener(move |this, _, _, cx| this.go(index + 1, cx))),
            )
            .child(div().text_sm().child("Zoom"))
            .child(choice(
                "zoom",
                &self.zoom.to_string(),
                zoom_options,
                "100%",
                move |value, _, cx| {
                    if let Ok(z) = value.parse::<f32>() {
                        zoom_entity.update(cx, |this, cx| {
                            this.zoom = z;
                            cx.notify();
                        });
                    }
                },
            ))
            .when(self.actions, |el| {
                el.child(
                    Button::new("print")
                        .outline()
                        .small()
                        .label("Print")
                        .tooltip("Print to a PDF file")
                        .disabled(!ready)
                        .on_click(cx.listener(|this, _, _, cx| this.export_pdf(cx))),
                )
                .child(
                    Button::new("export-pdf")
                        .outline()
                        .small()
                        .label("Export PDF…")
                        .disabled(!ready)
                        .on_click(cx.listener(|this, _, _, cx| this.export_pdf(cx))),
                )
            })
    }

    fn render_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (danger, muted) = (theme.danger, theme.muted_foreground);
        let loading = self.is_loading();
        let slow = self.elapsed_ms >= PROGRESS_DELAY_MS;
        v_flex()
            .px_3()
            .gap_1()
            .when(!self.error.is_empty(), |el| {
                el.child(widgets::styled_text("preview-error", self.error.clone(), move |d| d.text_color(danger)))
            })
            .when(loading, |el| {
                el.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(widgets::styled_text("report-loading", "Loading report data…", move |d| {
                            d.text_color(muted)
                        }))
                        .when(slow, |row| {
                            row.child(widgets::text("loading-seconds", format!("{} s", self.elapsed_ms / 1000)))
                                .child(
                                    Button::new("cancel-load")
                                        .outline()
                                        .small()
                                        .label("Cancel")
                                        .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                                )
                        }),
                )
            })
            .when(matches!(self.status, Status::Cancelled), |el| {
                el.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(widgets::text("load-cancelled", "Report loading cancelled."))
                        .child(
                            Button::new("retry-load")
                                .outline()
                                .small()
                                .label("Retry")
                                .on_click(cx.listener(|this, _, _, cx| this.start(cx))),
                        ),
                )
            })
            .when_some(
                match &self.status {
                    Status::Error(message) => Some(message.clone()),
                    _ => None,
                },
                |el, message| {
                    el.child(widgets::styled_text(
                        "load-error",
                        format!("Could not load report data: {message}"),
                        move |d| d.text_color(danger),
                    ))
                },
            )
    }

    fn render_notes(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Status::Ready(loaded) = &self.status else {
            return div().into_any_element();
        };
        let truncated = loaded.data.truncated;
        let problems = loaded.doc.diagnostics.len();
        let warning = cx.theme().warning;
        v_flex()
            .px_3()
            .gap_1()
            .when(truncated, |el| {
                el.child(widgets::styled_text(
                    "truncated",
                    format!("The report shows only the first {} rows of a query.", with_commas(REPORT_ROW_LIMIT)),
                    move |d| d.text_color(warning),
                ))
            })
            .when(problems > 0, |el| {
                let summary = format!("{problems} expression problem{}", if problems == 1 { "" } else { "s" });
                el.child(
                    Button::new("diagnostics-toggle")
                        .ghost()
                        .small()
                        .start_label(None, summary)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.diagnostics_open = !this.diagnostics_open;
                            cx.notify();
                        })),
                )
                .when(self.diagnostics_open, |el| {
                    el.children(loaded.doc.diagnostics.iter().enumerate().map(|(i, d)| {
                        widgets::styled_text(format!("diagnostic-{}", i + 1), d.message.clone(), move |d| {
                            d.text_sm().text_color(warning)
                        })
                    }))
                })
            })
            .into_any_element()
    }
}

/// 100000 as "100,000".
fn with_commas(n: u64) -> String {
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

/// Loads the report's data and lays it out.
fn run_report(
    label: &str,
    report: &crate::reports::Report,
    config: &crate::archive::DocumentConfig,
    values: Map<String, Value>,
    control: &LoadControl,
) -> Result<Loaded, AppError> {
    let data = load_report_data(label, report, config, &values, control)?;
    let generated = chrono::Utc::now();
    let options = LayoutOptions {
        params: Some(values),
        now: Some(generated),
        tables: data.tables.clone(),
        assets: data
            .assets
            .iter()
            .map(|(id, a)| (id.clone(), AssetInfo { media_type: a.media_type.clone() }))
            .collect(),
        subreports: data.subreports.clone(),
    };
    let doc = layout_report(report, &data.rows, &options);
    Ok(Loaded {
        doc,
        data,
        generated_at: generated.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    })
}

/// Builds the PDF and writes it through the backend command (which checks the role may
/// read the report). Returns why images printed as placeholders.
fn write_pdf_file(
    label: &str,
    report_id: &str,
    name: &str,
    doc: &ReportDocument,
    assets: &HashMap<String, PdfAsset>,
    generated_at: &str,
    path: &str,
) -> Result<Vec<String>, AppError> {
    let options = PdfOptions {
        title: Some(name.to_string()),
        creation_date: generated_at.to_string(),
        ..PdfOptions::default()
    };
    let (bytes, warnings) = report_pdf_bytes(doc, assets, options)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    crate::reports::write_report_pdf(label.to_string(), path.to_string(), encoded, Some(report_id.to_string()))?;
    Ok(warnings)
}

impl Render for ReportPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(key) = self.input_key(cx) else {
            return widgets::empty_state("report-missing", "Report not found.", "It may have been deleted.", cx)
                .into_any_element();
        };
        if key != self.key {
            self.key = key;
            self.start(cx);
        }
        let page = match &self.status {
            Status::Ready(loaded) => {
                let total = loaded.doc.pages.len();
                let index = self.page_index.min(total.saturating_sub(1));
                loaded.doc.pages.get(index).map(|page| {
                    page_view(&loaded.doc, page, self.zoom, &format!("Page {} of {total}", index + 1), &self.images)
                })
            }
            _ => None,
        };
        v_flex()
            .id("report-preview")
            .size_full()
            .min_h_0()
            .child(self.render_toolbar(cx))
            .child(self.render_status(cx))
            .child(self.render_notes(cx))
            .child(
                div()
                    .id("report-page-scroller")
                    .flex_1()
                    .min_h_0()
                    .overflow_scroll()
                    .p_4()
                    .bg(cx.theme().muted)
                    .when_some(page, |el, page| el.child(page)),
            )
            .into_any_element()
    }
}
