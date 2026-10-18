//! Backups: the size report shown before an upload (PRD 7.4), and a cloud runtime
//! installation's backups (PRD 23). Port of src/cloud/BackupSizeCheck.tsx and
//! InstallationBackups.tsx.
//!
//! Test ids. Size dialog `backup-size` ("Back up to ixtable Cloud?"): `backup-size-summary`,
//! `backup-size-limit` or `backup-size-over-limit`, `backup-size-largest:<i>`,
//! `backup-confirm` ("Back up now"), `backup-size-cancel`. Installation backups:
//! `backup-installation` ("Back up installation"), rows `backup:<i>` (text "WHEN · SIZE")
//! with `restore-backup:<i>` ("Restore…"), restore dialog `restore-backup` with
//! `restore-backup-note`, `restore-backup-confirm`, `restore-backup-cancel`, transfer bar
//! `backup-progress`, outcome `backups-notice` / `backups-error`.

use super::{
    action::{Action, action_status},
    contract,
    services::{CloudServices, Services},
    transfer::{Transfer, format_bytes as format_transfer_bytes, transfer_bar},
    types::{InstallationBackup, RestoreTarget},
};
use crate::{
    assets::ArchiveSizeReport,
    manager::SessionState,
    ui::{dialog::Dialog, forms, forms::eid, persistence::format::format_bytes, prelude::*},
};
use serde_json::json;
use std::sync::Arc;

/// Same text as Studio's version restore when the server sends none.
pub const POSTGRES_FALLBACK: &str = "This application keeps its records in an external PostgreSQL database. Restoring the archive brings back the definition and assets only; the database is not changed.";

/// A timestamp as the user's local date and time (the raw text when it is not a date).
pub fn when(created_at: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(created_at).map_or_else(
        |_| created_at.to_string(),
        |t| t.with_timezone(&chrono::Local).format("%b %-d, %Y, %H:%M").to_string(),
    )
}

/// The archive size report before a backup upload: an over-limit archive cannot be
/// backed up, and the report says what is large.
pub fn backup_size_dialog<V: 'static>(
    report: &ArchiveSizeReport,
    busy: bool,
    confirm: fn(&mut V, &mut Window, &mut Context<V>),
    cancel: fn(&mut V, &mut Context<V>),
    cx: &mut Context<V>,
) -> Dialog {
    let mut dialog = crate::ui::dialog::dialog("backup-size", "Back up to ixtable Cloud?").child(widgets::text(
        "backup-size-summary",
        format!(
            "{} in total; assets take {:.0}%. {}",
            format_bytes(report.total_bytes),
            report.asset_share * 100.0,
            if report.measured == "saved" {
                "Measured on the saved archive."
            } else {
                "Measured on a snapshot that includes unsaved changes."
            }
        ),
    ));
    dialog = dialog.child(if report.over_cloud_limit {
        widgets::callout(
            "backup-size-over-limit",
            widgets::Severity::Warning,
            &format!(
                "Over the cloud limit: this archive is larger than {}. It keeps working locally \
                 but cannot sync to ixtable Cloud until it is smaller.",
                format_bytes(report.cloud_limit_bytes)
            ),
            None,
            cx,
        )
        .into_any_element()
    } else {
        widgets::text(
            "backup-size-limit",
            format!("Within the {} cloud limit.", format_bytes(report.cloud_limit_bytes)),
        )
        .into_any_element()
    });
    dialog = dialog.child(super::largest::largest_files("backup-size-largest", &report.largest, report.total_bytes, cx));
    dialog
        .footer(
            forms::secondary("backup-size-cancel", "Cancel")
                .disabled(busy)
                .on_click(cx.listener(move |this, _, _, cx| cancel(this, cx))),
        )
        .footer(
            forms::primary("backup-confirm", "Back up now")
                .disabled(busy || report.over_cloud_limit)
                .on_click(cx.listener(move |this, _, window, cx| confirm(this, window, cx))),
        )
        .modal()
}

pub enum BackupsEvent {
    /// A restored copy to open in this window.
    Restored(SessionState),
}

struct PendingRestore {
    backup: InstallationBackup,
    target: RestoreTarget,
}

/// Runtime installation backups: size report first, this installation's backups, and
/// restore to a new local copy (`restore-url` with `backupId`). The installation itself
/// is never changed.
pub struct InstallationBackups {
    services: Arc<dyn CloudServices>,
    label: String,
    app_id: String,
    installation_id: String,
    backups: Option<Vec<InstallationBackup>>,
    size_check: Option<ArchiveSizeReport>,
    pending: Option<PendingRestore>,
    action: Action,
    transfer: Transfer,
}

impl EventEmitter<BackupsEvent> for InstallationBackups {}

impl InstallationBackups {
    pub fn new(label: String, app_id: String, installation_id: String, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            services: Services::get(cx),
            label,
            app_id,
            installation_id,
            backups: None,
            size_check: None,
            pending: None,
            action: Action::default(),
            transfer: Transfer::default(),
        };
        this.load(cx);
        this
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let (services, app, installation) =
            (self.services.clone(), self.app_id.clone(), self.installation_id.clone());
        task::spawn_call(
            cx,
            move || services.list_installation_backups(&app, &installation),
            |this, result, cx| {
                this.backups = Some(result.unwrap_or_default());
                cx.notify();
            },
        );
    }

    fn check_size(&mut self, cx: &mut Context<Self>) {
        self.pending = None;
        self.action.begin();
        let (services, label) = (self.services.clone(), self.label.clone());
        task::spawn_call(cx, move || services.archive_size_report(&label), |this, result, cx| {
            match result {
                Ok(report) => {
                    this.action.finish(Ok(String::new()));
                    this.size_check = Some(report);
                }
                Err(e) => this.action.finish(Err(e)),
            }
            cx.notify();
        });
        cx.notify();
    }

    fn backup(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.size_check = None;
        self.action.begin();
        let services = self.services.clone();
        let (label, app_id, installation_id) =
            (self.label.clone(), self.app_id.clone(), self.installation_id.clone());
        let transfer_id = self.transfer.begin(services.clone(), |v: &mut Self| &mut v.transfer, cx);
        task::spawn_call(
            cx,
            move || {
                let session = services.require_session()?;
                let upload = services.upload_archive(&label, &session.access_token, &app_id, "backup", &transfer_id)?;
                let reply = services.invoke(
                    "backup-commit",
                    json!({
                        "appId": app_id,
                        "uploadId": upload.upload_id,
                        "installationId": upload.installation_id.clone().unwrap_or(installation_id),
                    }),
                )?;
                contract::backup_commit(&reply)?;
                Ok(upload.size)
            },
            |this, result, cx| {
                this.transfer.end();
                this.action.finish(result.map(|size| {
                    format!("Backed up {} of this installation.", format_transfer_bytes(size))
                }));
                this.load(cx);
                cx.notify();
            },
        );
        cx.notify();
    }

    fn prepare_restore(&mut self, backup: InstallationBackup, cx: &mut Context<Self>) {
        self.size_check = None;
        self.action.begin();
        let (services, app_id) = (self.services.clone(), self.app_id.clone());
        let id = backup.id.clone();
        task::spawn_call(
            cx,
            move || {
                let reply = services.invoke("restore-url", json!({ "appId": app_id, "backupId": id }))?;
                contract::restore_url(&reply)
            },
            move |this, result, cx| {
                match result {
                    Ok(target) => {
                        this.action.finish(Ok(String::new()));
                        this.pending = Some(PendingRestore { backup, target });
                    }
                    Err(e) => this.action.finish(Err(e)),
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    fn restore(&mut self, cx: &mut Context<Self>) {
        let Some(PendingRestore { target, .. }) = self.pending.take() else { return };
        self.action.begin();
        let (services, label) = (self.services.clone(), self.label.clone());
        let transfer_id = self.transfer.begin(services.clone(), |v: &mut Self| &mut v.transfer, cx);
        task::spawn_call(
            cx,
            move || {
                services.restore_copy(&label, &target.signed_url, &target.sha256, target.size, None, &transfer_id)
            },
            |this, result, cx| {
                this.transfer.end();
                match result {
                    Ok(state) => {
                        this.action.finish(Ok(String::new()));
                        cx.emit(BackupsEvent::Restored(state));
                    }
                    Err(e) => this.action.finish(Err(e)),
                }
                cx.notify();
            },
        );
        cx.notify();
    }
}

impl Render for InstallationBackups {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.action.busy;
        let mut block = v_flex().gap_2().child(
            forms::secondary("backup-installation", "Back up installation")
                .disabled(busy)
                .on_click(cx.listener(|this, _, _, cx| this.check_size(cx))),
        );
        if let Some(report) = self.size_check.clone() {
            // The sidebar is narrow: the dialog floats above the window instead.
            block = block.child(backup_size_dialog(
                &report,
                busy,
                |this: &mut Self, window, cx| this.backup(window, cx),
                |this: &mut Self, cx| {
                    this.size_check = None;
                    cx.notify();
                },
                cx,
            ));
        }
        for (i, item) in self.backups.clone().unwrap_or_default().into_iter().enumerate() {
            let at = when(&item.created_at);
            block = block.child(
                v_flex()
                    .gap_1()
                    .child(widgets::text(
                        eid(format!("backup:{i}")),
                        format!("{at} · {}", format_transfer_bytes(item.archive_size)),
                    ))
                    .child(
                        forms::link(eid(format!("restore-backup:{i}")), "Restore…")
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, _, cx| this.prepare_restore(item.clone(), cx))),
                    ),
            );
        }
        if let Some(pending) = &self.pending {
            let postgres = pending.target.is_postgres;
            let mut dialog = crate::ui::dialog::dialog(
                "restore-backup",
                &format!("Restore the backup from {}?", when(&pending.backup.created_at)),
            )
            .child(
                "It opens as a new local copy in this window. This installation and its records \
                 are not changed.",
            );
            if postgres {
                dialog = dialog.child(widgets::callout(
                    "restore-backup-note",
                    widgets::Severity::Warning,
                    &format!(
                        "PostgreSQL records are not restored. {}",
                        pending.target.warning.as_deref().unwrap_or(POSTGRES_FALLBACK)
                    ),
                    None,
                    cx,
                ));
            }
            block = block.child(
                dialog
                    .footer(
                        forms::secondary("restore-backup-cancel", "Cancel")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pending = None;
                                cx.notify();
                            })),
                    )
                    .footer(
                        forms::primary(
                            "restore-backup-confirm",
                            if postgres { "Restore definition only" } else { "Restore to new local copy" },
                        )
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| this.restore(cx))),
                    )
                    .modal(),
            );
        }
        if let Some(bar) =
            transfer_bar("backup-progress", "Backup transfer progress", self.transfer.progress.as_ref(), cx)
        {
            block = block.child(bar);
        }
        block.child(action_status("backups", &self.action, cx))
    }
}
