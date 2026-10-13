//! Converts a Microsoft Access database or template into a new document: tables with
//! their data, relationships, queries, forms, reports and macros
//! (docs/decisions/access-import.md). Shows what the file holds first, then a report of
//! what each object became. Port of src/access/AccessImportWizard.tsx.
//!
//! Test ids: dialog title `access-wizard`, `access-choose`, `access-file`,
//! `access-busy`, `access-error`, `access-import`, `access-cancel`, `access-open`;
//! contents card `access-contents` with `access-summary`, `access-count:<Label>`,
//! `access-compiled`, `access-table:<name>` ("name fields rows"), `access-warning:<i>`,
//! checkbox `access-include-data`; report card `access-report` with `access-created`,
//! `access-note-about-settings`, `report-row:<kind>` ("Tables: converted C, partly P, not
//! converted N"), `attention-toggle` ("What did not convert fully (N)"), and when shown
//! `attention:<kind>:<name>` with `attention-note:<kind>:<name>:<i>`; `report-warning:<i>`.

use super::summary::{
    ImportItem, items_needing_attention, items_of, kind_label, status_counts,
    total_rows,
};
use crate::{
    access::{Inventory, convert::Options},
    manager::{AppError, SessionState},
    ui::{
        dialogs::{self, FileFilter},
        forms::{self, eid},
        prelude::*,
    },
};
use gpui_kit::component::Disableable as _;

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
    result: Option<Imported>,
    show_attention: bool,
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
    format!("{} {word}{}", grouped(n), if n == 1 { "" } else { "s" })
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
            result: None,
            show_attention: false,
            focus: cx.focus_handle(),
            focus_dialog: true,
        }
    }

    fn is_busy(&self) -> bool {
        !self.busy.is_empty()
    }

    fn choose(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        cx.notify();
        let pick = dialogs::pick_file(cx, "Import Access database", &[ACCESS_FILES]);
        task::after(cx, pick, |this, picked, cx| {
            let Some(picked) = picked else { return };
            let path = picked.to_string_lossy().into_owned();
            this.path = path.clone();
            this.inventory = None;
            this.busy = "Reading the Access file…".into();
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
        });
    }

    fn import(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        self.busy = "Importing… large databases take a while".into();
        cx.notify();
        let (label, path, include_data) = (self.label.clone(), self.path.clone(), self.include_data);
        task::spawn_call(
            cx,
            move || {
                tauri::async_runtime::block_on(crate::access::import_access_file(
                    label,
                    path,
                    Some(Options { include_data }),
                ))
            },
            |this, result, cx| {
                this.busy.clear();
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
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
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

    fn contents(&self, inventory: &Inventory, cx: &mut Context<Self>) -> Stateful<Div> {
        let rows = total_rows(inventory);
        let objects = [
            ("Queries", inventory.queries.len()),
            ("Forms", inventory.forms.len()),
            ("Reports", inventory.reports.len()),
            ("Macros", inventory.macros.len()),
            ("Modules", inventory.modules.len()),
        ];
        let compiled = inventory
            .compiled
            .iter()
            .filter(|(kind, _)| kind == "form" || kind == "report")
            .count();
        let format: super::summary::AccessFormat = inventory.format.into();
        let mut card = forms::card("access-contents", "Access file contents", cx).child(widgets::text(
            "access-summary",
            format!(
                "{}: {}, {}, {}.",
                format.label(),
                plural(inventory.tables.len() as u64, "table"),
                plural(rows, "row"),
                plural(inventory.relationships as u64, "relationship"),
            ),
        ));
        for (label, n) in objects.into_iter().filter(|(_, n)| *n > 0) {
            card = card.child(widgets::text(eid(format!("access-count:{label}")), format!("{label}: {n}")));
        }
        if compiled > 0 {
            card = card.child(widgets::text(
                "access-compiled",
                format!(
                    "This file stores its {compiled} forms and reports compiled, so they cannot be read. ixtable creates a list and a detail form for each table instead."
                ),
            ));
        }
        for t in &inventory.tables {
            let count = t.rows.map_or("—".to_string(), grouped);
            card = card.child(widgets::text(
                eid(format!("access-table:{}", t.name)),
                format!("{} {} {}", t.name, t.columns, count),
            ));
        }
        for (i, w) in inventory.warnings.iter().enumerate() {
            card = card.child(widgets::text(eid(format!("access-warning:{i}")), w.clone()));
        }
        let entity = cx.entity();
        card.child(
            forms::check(
                "access-include-data",
                &format!("Import the data ({} rows)", grouped(rows)),
                self.include_data,
                move |on, _, cx| {
                    entity.update(cx, |this, cx| {
                        this.include_data = on;
                        cx.notify();
                    })
                },
            )
            .disabled(self.is_busy()),
        )
    }

    fn report(&self, done: &Imported, cx: &mut Context<Self>) -> Stateful<Div> {
        let attention = items_needing_attention(&done.items);
        let mut card = forms::card("access-report", "Access import report", cx)
            .child(widgets::text(
                "access-created",
                format!(
                    "Created a document with {} and {}.",
                    plural(done.tables as u64, "table"),
                    plural(done.rows, "row"),
                ),
            ))
            .child(widgets::text(
                "access-note-about-settings",
                "Settings › Access migration keeps this report, with links, review marks and export.",
            ));
        for c in status_counts(&done.items) {
            card = card.child(widgets::text(
                eid(format!("report-row:{}", c.kind)),
                format!(
                    "{}: converted {}, partly {}, not converted {}",
                    kind_label(&c.kind),
                    c.converted,
                    c.partial,
                    c.skipped
                ),
            ));
        }
        if !attention.is_empty() {
            card = card.child(
                forms::link("attention-toggle", &format!("What did not convert fully ({})", attention.len()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_attention = !this.show_attention;
                        cx.notify();
                    })),
            );
            if self.show_attention {
                for item in &attention {
                    let key = format!("{}:{}", item.kind, item.name);
                    card = card.child(widgets::text(
                        eid(format!("attention:{key}")),
                        format!("{} ({})", item.name, item.kind),
                    ));
                    for (i, note) in item.notes.iter().enumerate() {
                        card = card.child(widgets::text(
                            eid(format!("attention-note:{key}:{i}")),
                            note.clone(),
                        ));
                    }
                }
            }
        }
        for (i, w) in done.warnings.iter().enumerate() {
            card = card.child(widgets::text(eid(format!("report-warning:{i}")), w.clone()));
        }
        card
    }
}

impl Render for AccessImportWizard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.focus_dialog) {
            window.focus(&self.focus, cx);
        }
        let busy = self.is_busy();
        let mut dialog = forms::dialog("access-wizard", "Import Access database", cx);
        if self.result.is_none() {
            let shown = if self.path.is_empty() {
                ".accdb, .mdb, or an Access template (.accdt)".to_string()
            } else {
                file_name(&self.path)
            };
            dialog = dialog.child(
                forms::actions()
                    .child(
                        forms::secondary("access-choose", "Choose file…")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.choose(cx))),
                    )
                    .child(widgets::text("access-file", shown)),
            );
        }
        if busy {
            dialog = dialog.child(widgets::text("access-busy", self.busy.clone()));
        }
        if let (Some(inventory), None) = (self.inventory.clone(), &self.result) {
            dialog = dialog.child(self.contents(&inventory, cx));
        }
        if let Some(e) = &self.error {
            dialog = dialog.child(widgets::error_banner("access-error", e, None, cx));
        }
        if let Some(done) = &self.result {
            dialog = dialog.child(self.report(done, cx));
        }
        let actions = if self.result.is_some() {
            forms::actions().child(
                forms::primary("access-open", "Open document")
                    .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
            )
        } else {
            forms::actions()
                .child(
                    forms::primary("access-import", "Import")
                        .disabled(busy || self.inventory.is_none())
                        .on_click(cx.listener(|this, _, _, cx| this.import(cx))),
                )
                .child(
                    forms::secondary("access-cancel", "Cancel")
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                )
        };
        let entity = cx.entity();
        forms::overlay(forms::escapable(
            dialog.child(actions),
            &self.focus,
            busy,
            move |_, cx| entity.update(cx, |this, cx| this.close(cx)),
        ))
    }
}
