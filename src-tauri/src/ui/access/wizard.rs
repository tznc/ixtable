//! Converts a Microsoft Access database or template into a new document: tables with
//! their data, relationships, queries, forms, reports and macros
//! (docs/decisions/access-import.md). Shows what the file holds first, then a report of
//! what each object became. Port of src/access/AccessImportWizard.tsx.
//!
//! Test ids: dialog title `access-wizard` ("Import from Access"), `access-choose`,
//! `access-file`, `access-busy` (with the time left once it can be told), `access-progress`,
//! `access-notice` (a cancelled import, or a file that changed after it was read),
//! `access-error` (with
//! `access-error-hint` for files that are not Access files, and the link
//! `access-error-details-toggle` that shows the reader's text, `access-error-details`),
//! `access-import`, `access-cancel`, `access-open`. Contents:
//! `access-summary`, `access-count:<Label>`, `access-compiled`, table rows
//! `access-table:<name>` ("name: F fields, R rows"), `access-warning:<i>`, checkbox
//! `access-include-data`. Report: verdict `access-report`, `access-created`,
//! `access-note-about-settings`, `access-unsaved`, `report-row:<kind>` ("Tables: converted
//! C, partly P, not converted N"), `attention-toggle` ("What did not convert fully (N)"),
//! and when shown `attention:<kind>:<name>` with `attention-note:<kind>:<name>:<i>`;
//! `report-warning:<i>`.

use super::summary::{ImportItem, items_of, total_rows};
use crate::{
    access::{Inventory, convert::Options},
    manager::{AppError, SessionState},
    ui::{
        dialog::dialog,
        dialogs::{self, FileFilter},
        forms,
        prelude::*,
    },
};
use gpui_kit::component::{Disableable as _, progress::Progress};

mod sections;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Instant, SystemTime};

#[cfg(test)]
static CANCEL_AFTER_FIRST_TABLE: AtomicBool = AtomicBool::new(false);

/// Tests cannot click Cancel while an import runs, so this cancels once a table is copied.
#[cfg(test)]
pub fn cancel_after_first_table_for_test(on: bool) {
    CANCEL_AFTER_FIRST_TABLE.store(on, Ordering::SeqCst);
}

/// Size and modification time, to notice a file that changes after it was read.
fn stamp(path: &str) -> Option<(u64, Option<SystemTime>)> {
    std::fs::metadata(path).ok().map(|m| (m.len(), m.modified().ok()))
}

/// Rows copied so far, written by the import thread and read by the dialog.
#[derive(Default)]
struct Copied {
    /// Tables finished, with their row counts.
    finished: Vec<(String, u64)>,
    /// The table being copied and its rows so far.
    current: Option<(String, u64)>,
}

pub const ACCESS_FILES: FileFilter = FileFilter {
    name: "Access databases and templates",
    extensions: &["accdb", "mdb", "accdt"],
};

pub enum WizardEvent {
    /// Dismissed before anything was imported.
    Close,
    /// The imported document is open in the window's session.
    Opened(SessionState),
}

struct Imported {
    state: SessionState,
    items: Vec<ImportItem>,
    warnings: Vec<String>,
    tables: usize,
    rows: u64,
}

pub struct AccessImportWizard {
    label: String,
    path: String,
    inventory: Option<Inventory>,
    include_data: bool,
    busy: String,
    error: Option<AppError>,
    show_error_details: bool,
    /// A cancelled import or a changed file.
    notice: Option<String>,
    /// The file as it was when its contents were read.
    stamp: Option<(u64, Option<SystemTime>)>,
    /// Set by Cancel while an import runs.
    cancel: Arc<AtomicBool>,
    started: Option<Instant>,
    result: Option<Imported>,
    show_attention: Option<bool>,
    copied: Arc<Mutex<Copied>>,
    focus: FocusHandle,
    focus_dialog: bool,
}
impl EventEmitter<WizardEvent> for AccessImportWizard {}

/// 12345 -> "12,345".
fn grouped(n: u64) -> String {
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

fn plural(n: u64, word: &str) -> String {
    crate::ui::widgets::plural(n as usize, word)
}

fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().filter(|n| !n.is_empty()).unwrap_or(path).to_string()
}

impl AccessImportWizard {
    /// `label`: the window label the imported document opens in.
    pub fn new(label: String, cx: &mut Context<Self>) -> Self {
        Self {
            label,
            path: String::new(),
            inventory: None,
            include_data: true,
            busy: String::new(),
            error: None,
            show_error_details: false,
            notice: None,
            stamp: None,
            cancel: Arc::default(),
            started: None,
            result: None,
            show_attention: None,
            copied: Arc::default(),
            focus: cx.focus_handle(),
            focus_dialog: true,
        }
    }

    fn is_busy(&self) -> bool {
        !self.busy.is_empty()
    }

    fn choose(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        self.show_error_details = false;
        self.notice = None;
        cx.notify();
        let pick = dialogs::pick_file(cx, "Import from Access", &[ACCESS_FILES]);
        task::after(cx, pick, |this, picked, cx| {
            let Some(picked) = picked else { return };
            this.path = picked.to_string_lossy().into_owned();
            this.read_contents(cx);
        });
    }

    /// Reads what the chosen file holds.
    fn read_contents(&mut self, cx: &mut Context<Self>) {
        let path = self.path.clone();
        self.inventory = None;
        self.stamp = stamp(&path);
        self.busy = "Reading the Access file…".into();
        cx.notify();
        task::spawn_call(
            cx,
            move || tauri::async_runtime::block_on(crate::access::inspect_access_file(path)),
            |this, result, cx| {
                this.busy.clear();
                match result {
                    Ok(inventory) => this.inventory = Some(inventory),
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
    }

    fn import(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        self.show_error_details = false;
        self.notice = None;
        if stamp(&self.path) != self.stamp {
            self.notice = Some("The file changed since it was read. Check its contents again, then import.".into());
            self.read_contents(cx);
            return;
        }
        self.busy = "Converting tables, queries, forms and reports…".into();
        *self.copied.lock().unwrap() = Copied::default();
        self.cancel = Arc::default();
        self.started = Some(Instant::now());
        cx.notify();
        let (label, path, include_data) = (self.label.clone(), self.path.clone(), self.include_data);
        let cancel = self.cancel.clone();
        let copied = self.copied.clone();
        #[cfg(test)]
        let test_cancel = self.cancel.clone();
        let report: Box<dyn FnMut(&str, u64) + Send> = Box::new(move |table, rows| {
            #[cfg(test)]
            if CANCEL_AFTER_FIRST_TABLE.load(Ordering::SeqCst) {
                test_cancel.store(true, Ordering::SeqCst);
            }
            let mut c = copied.lock().unwrap();
            if let Some((name, n)) = c.current.take() {
                if name != table {
                    c.finished.push((name, n));
                }
            }
            c.current = Some((table.to_string(), rows));
        });
        // Redraws the progress while the import runs.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(200)).await;
                let running = this
                    .update(cx, |this, cx| {
                        cx.notify();
                        this.is_busy()
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
            }
        })
        .detach();
        task::spawn_call(
            cx,
            move || {
                tauri::async_runtime::block_on(crate::access::import_access_file(
                    label,
                    path,
                    Some(Options { include_data, cancel: Some(cancel) }),
                    Some(report),
                ))
            },
            |this, result, cx| {
                this.busy.clear();
                this.started = None;
                match result {
                    Ok(done) => {
                        this.result = Some(Imported {
                            items: items_of(&done.report),
                            warnings: done.report.warnings.clone(),
                            tables: done.report.tables,
                            rows: done.report.rows,
                            state: done.state,
                        })
                    }
                    Err(e) if e.code == "IMPORT_CANCELLED" => this.notice = Some(e.message),
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
    }

    /// Cancel: stops a running import, otherwise closes the dialog.
    fn cancel_or_close(&mut self, cx: &mut Context<Self>) {
        if self.is_busy() && self.started.is_some() {
            self.cancel.store(true, Ordering::SeqCst);
            self.busy = "Cancelling…".into();
            cx.notify();
        } else {
            self.cancel(cx);
        }
    }

    // Once imported, the new document is open: every way out shows it.
    fn close(&mut self, cx: &mut Context<Self>) {
        match &self.result {
            Some(done) => cx.emit(WizardEvent::Opened(done.state.clone())),
            None => cx.emit(WizardEvent::Close),
        }
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(WizardEvent::Close);
    }

    /// Copy progress: a line (`access-busy`) and a bar (`access-progress`) over all rows.
    fn progress(&self, cx: &App) -> Div {
        let total = self.inventory.as_ref().map_or(0, total_rows);
        let copied = self.copied.lock().unwrap();
        let done: u64 = copied.finished.iter().map(|(_, n)| n).sum();
        let cancelling = self.cancel.load(Ordering::SeqCst);
        let (line, value) = match &copied.current {
            Some((table, n)) if self.include_data && total > 0 && !cancelling => {
                let of = self
                    .inventory
                    .as_ref()
                    .and_then(|i| i.tables.iter().find(|t| &t.name == table))
                    .and_then(|t| t.rows)
                    .map(|rows| format!(" of {}", grouped(rows)))
                    .unwrap_or_default();
                let left = self
                    .started
                    .and_then(|t| crate::access::convert::time_left(done + n, total, t.elapsed()))
                    .map(|l| format!(" · {l}"))
                    .unwrap_or_default();
                (
                    format!("Copying {table}: {} rows{of}{left}", grouped(*n)),
                    ((done + n) as f32 / total as f32 * 100.).min(100.),
                )
            }
            _ => (self.busy.clone(), 0.),
        };
        let theme = cx.theme();
        v_flex()
            .gap_1()
            .child(widgets::styled_text("access-busy", line, |d| {
                d.ty(Type::Body).text_color(theme.muted_foreground)
            }))
            .child(Progress::new("access-progress").value(value).loading(value == 0.))
    }
}

impl Render for AccessImportWizard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.focus_dialog) {
            window.focus(&self.focus, cx);
        }
        let busy = self.is_busy();
        let mut panel = dialog("access-wizard", "Import from Access");
        if self.result.is_none() {
            let theme = cx.theme();
            if self.inventory.is_none() {
                panel = panel.child(widgets::styled_text(
                    "access-intro",
                    "Creates a new ixtable document from a Microsoft Access database (.accdb, .mdb) or template (.accdt). Your Access file is not changed.",
                    |d| d.ty(Type::Body).text_color(theme.muted_foreground),
                ));
            }
            let (choose, shown) = if self.path.is_empty() {
                (forms::primary("access-choose", "Choose file…"), "No file chosen".to_string())
            } else {
                (forms::secondary("access-choose", "Choose another file…"), file_name(&self.path))
            };
            panel = panel.child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(choose.disabled(busy).on_click(cx.listener(|this, _, _, cx| this.choose(cx))))
                    .child(widgets::styled_text("access-file", shown, |d| d.ty(Type::Body))),
            );
        }
        if let (Some(inventory), None) = (self.inventory.clone(), &self.result) {
            panel = panel.child(self.contents(&inventory, cx));
        }
        if busy {
            panel = panel.child(self.progress(cx));
        }
        if let Some(notice) = &self.notice {
            panel = panel.child(widgets::styled_text("access-notice", notice.clone(), |d| d.ty(Type::Body)));
        }
        if let Some(e) = &self.error {
            let (shown, details) = match e.message.split_once(crate::access::DETAILS) {
                Some((shown, details)) => (shown.to_string(), Some(details.to_string())),
                None => (e.message.clone(), None),
            };
            let not_access = shown.starts_with("This file is not an Access");
            let banner = AppError { code: e.code.clone(), message: shown };
            let open = self.show_error_details;
            panel = panel.child(
                v_flex()
                    .gap_1()
                    .child(widgets::error_banner("access-error", &banner, None, cx))
                    .when(not_access && self.inventory.is_none(), |el| {
                        el.child(forms::muted("Choose a .accdb, .mdb or .accdt file.", cx).id("access-error-hint"))
                    })
                    .when_some(details, |el, details| {
                        el.child(
                            h_flex().child(
                                forms::link("access-error-details-toggle", if open { "Hide details" } else { "Show details" })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.show_error_details = !open;
                                        cx.notify();
                                    })),
                            ),
                        )
                        .when(open, |el| {
                            el.child(widgets::styled_text("access-error-details", details, |d| {
                                d.ty(Type::Caption).text_color(cx.theme().muted_foreground)
                            }))
                        })
                    }),
            );
        }
        if let Some(done) = &self.result {
            panel = panel.child(self.report(done, cx));
        }
        panel = if self.result.is_some() {
            panel.footer(
                forms::primary("access-open", "Open document")
                    .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
            )
        } else {
            panel
                .footer(
                    // While an import runs, Cancel stops it; reading a file is quick.
                    forms::secondary("access-cancel", "Cancel")
                        .disabled(busy && self.started.is_none())
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_or_close(cx))),
                )
                .footer(
                    forms::primary("access-import", "Import")
                        .disabled(busy || self.inventory.is_none())
                        .on_click(cx.listener(|this, _, _, cx| this.import(cx))),
                )
        };
        let entity = cx.entity();
        panel
            .on_escape(&self.focus, busy, move |_, cx| entity.update(cx, |this, cx| this.close(cx)))
            .modal()
    }
}
