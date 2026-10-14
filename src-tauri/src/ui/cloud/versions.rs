//! Version history (RLS-scoped), restore to a new local copy, and developer backups.
//! Port of src/cloud/VersionsPanel.tsx.
//!
//! Test ids. Card `cloud-versions` ("Published versions"); `versions-loading`,
//! `versions-empty`; rows `version:<i>` (text "VERSION · status · when · size") with
//! `restore-version:<i>` ("Restore to new local copy"); PostgreSQL dialog
//! `restore-warning` with `restore-warning-text`, `restore-warning-confirm`,
//! `restore-warning-cancel`; `backup-start` ("Back up to cloud"), the size dialog of
//! `backups`, transfer bar `versions-progress`, outcome `versions-notice` / `versions-error`.

use super::{
    action::{Action, action_status},
    backups::{backup_size_dialog, when},
    contract,
    errors::CloudError,
    publish::save_for_upload,
    services::{CloudServices, Services},
    transfer::{Transfer, format_bytes, transfer_bar},
    types::{AppVersion, CloudApp, RestoreTarget},
};
use crate::{
    assets::ArchiveSizeReport,
    manager::SessionState,
    ui::{dialogs, forms, forms::eid, prelude::*, shell::Shell},
};
use serde_json::json;
use std::sync::Arc;

pub enum VersionsEvent {
    /// A restored copy to open in this window.
    Restored(SessionState),
}

struct PendingRestore {
    version: AppVersion,
    target: RestoreTarget,
}

const POSTGRES_WARNING: &str = "This application keeps its records in an external PostgreSQL database. Restoring the archive brings back the definition and assets only; the database is not changed.";

pub struct VersionsPanel {
    shell: Entity<Shell>,
    services: Arc<dyn CloudServices>,
    app: Option<CloudApp>,
    versions: Option<Vec<AppVersion>>,
    pending: Option<PendingRestore>,
    size_check: Option<ArchiveSizeReport>,
    action: Action,
    transfer: Transfer,
    focus: FocusHandle,
}

impl EventEmitter<VersionsEvent> for VersionsPanel {}

impl VersionsPanel {
    pub fn new(shell: Entity<Shell>, app: Option<CloudApp>, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            shell,
            services: Services::get(cx),
            app: None,
            versions: None,
            pending: None,
            size_check: None,
            action: Action::default(),
            transfer: Transfer::default(),
            focus: cx.focus_handle(),
        };
        this.set_app(app, cx);
        this
    }

    /// Shows the versions of `app` (None: nothing to show yet).
    pub fn set_app(&mut self, app: Option<CloudApp>, cx: &mut Context<Self>) {
        self.app = app;
        self.reload(cx);
        cx.notify();
    }

    /// Reads the version list again (after a publish).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(app_id) = self.app.as_ref().map(|a| a.id.clone()) else { return };
        let services = self.services.clone();
        task::spawn_call(cx, move || services.list_versions(&app_id), |this, result, cx| {
            this.versions = Some(result.unwrap_or_default());
            cx.notify();
        });
    }

    pub fn app(&self) -> Option<&CloudApp> {
        self.app.as_ref()
    }

    fn app_id(&self) -> String {
        self.app.as_ref().map(|a| a.id.clone()).unwrap_or_default()
    }

    fn prepare_restore(&mut self, version: AppVersion, window: &mut Window, cx: &mut Context<Self>) {
        self.action.begin();
        let (services, app_id, id) = (self.services.clone(), self.app_id(), version.id.clone());
        task::spawn_call_in(
            window,
            cx,
            move || {
                let reply = services.invoke("restore-url", json!({ "appId": app_id, "versionId": id }))?;
                contract::restore_url(&reply)
            },
            move |this, result, window, cx| match result {
                Err(e) => {
                    this.action.finish(Err(e));
                    cx.notify();
                }
                // PostgreSQL records live outside the archive: say so before anything changes.
                Ok(target) if target.is_postgres => {
                    this.action.finish(Ok(String::new()));
                    this.pending = Some(PendingRestore { version, target });
                    window.focus(&this.focus, cx);
                    cx.notify();
                }
                Ok(target) => this.restore(PendingRestore { version, target }, window, cx),
            },
        );
        cx.notify();
    }

    fn restore(&mut self, pending: PendingRestore, _: &mut Window, cx: &mut Context<Self>) {
        self.pending = None;
        let label = self.shell.read(cx).label().to_string();
        let (dirty, name) = {
            let doc = &self.shell.read(cx).doc;
            (doc.dirty, doc.name.clone())
        };
        self.action.begin();
        let services = self.services.clone();
        let PendingRestore { version, target } = pending;
        let ask = dirty.then(|| {
            dialogs::confirm(
                cx,
                "Restore",
                &format!("Discard unsaved changes in {name} and open the restored copy?"),
            )
        });
        let transfer_id = self.transfer.begin(services.clone(), |v: &mut Self| &mut v.transfer, cx);
        let run = move |this: &mut Self, go: bool, cx: &mut Context<Self>| {
            if !go {
                this.transfer.end();
                this.action.finish(Ok(String::new()));
                cx.notify();
                return;
            }
            task::spawn_call(
                cx,
                move || {
                    services.restore_copy(
                        &label,
                        &target.signed_url,
                        &target.sha256,
                        target.size,
                        Some(version.id.clone()),
                        &transfer_id,
                    )
                },
                |this, result, cx| {
                    this.transfer.end();
                    match result {
                        Ok(state) => {
                            this.action.finish(Ok(String::new()));
                            cx.emit(VersionsEvent::Restored(state));
                        }
                        Err(e) => this.action.finish(Err(e)),
                    }
                    cx.notify();
                },
            );
        };
        match ask {
            Some(ask) => task::after(cx, ask, run),
            None => run(self, true, cx),
        }
        cx.notify();
    }

    // PRD 7.4: show the size report first; over-limit archives are not uploaded.
    fn check_size(&mut self, cx: &mut Context<Self>) {
        self.action.begin();
        let (services, label) = (self.services.clone(), self.shell.read(cx).label().to_string());
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
        let label = self.shell.read(cx).label().to_string();
        let (services, app_id) = (self.services.clone(), self.app_id());
        let transfer_id = self.transfer.begin(services.clone(), |v: &mut Self| &mut v.transfer, cx);
        task::spawn_call(
            cx,
            move || -> Result<(Option<SessionState>, Result<u64, CloudError>), CloudError> {
                let state = save_for_upload(&label, "backing it up")?;
                let backed_up = (|| {
                    let session = services.require_session()?;
                    let upload =
                        services.upload_archive(&label, &session.access_token, &app_id, "backup", &transfer_id)?;
                    let reply = services.invoke(
                        "backup-commit",
                        json!({
                            "appId": app_id,
                            "uploadId": upload.upload_id,
                            "installationId": upload.installation_id,
                        }),
                    )?;
                    contract::backup_commit(&reply)?;
                    Ok(upload.size)
                })();
                Ok((Some(state), backed_up))
            },
            |this, result, cx| {
                this.transfer.end();
                match result {
                    Err(e) => this.action.finish(Err(e)),
                    Ok((state, outcome)) => {
                        if let Some(state) = state {
                            this.shell.update(cx, |s, cx| s.apply_session(&state, cx));
                        }
                        this.action.finish(
                            outcome.map(|size| format!("Backed up {} to ixtable Cloud.", format_bytes(size))),
                        );
                    }
                }
                cx.notify();
            },
        );
        cx.notify();
    }
}

impl Render for VersionsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(app) = self.app.clone() else { return div().into_any_element() };
        let busy = self.action.busy;
        let mut card = forms::card("cloud-versions", "Published versions", cx);
        card = match &self.versions {
            None => card.child(widgets::text("versions-loading", "Loading versions…")),
            Some(list) if list.is_empty() => {
                card.child(widgets::text("versions-empty", "Nothing has been published yet."))
            }
            Some(list) => {
                let mut rows = v_flex().gap_1();
                for (i, version) in list.iter().enumerate() {
                    let mut line = format!("{} · {}", version.version, version.status);
                    if let Some(resolution) = &version.resolution {
                        line.push_str(&format!(" · {resolution}"));
                    }
                    line.push_str(&format!(" · {} · {}", when(&version.created_at), format_bytes(version.archive_size)));
                    if !version.release_notes.is_empty() {
                        line.push_str(&format!(" — {}", version.release_notes));
                    }
                    let picked = version.clone();
                    rows = rows.child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(widgets::text(eid(format!("version:{i}")), line))
                            .child(
                                forms::link(eid(format!("restore-version:{i}")), "Restore to new local copy")
                                    .disabled(busy)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.prepare_restore(picked.clone(), window, cx)
                                    })),
                            ),
                    );
                }
                card.child(rows)
            }
        };
        if let Some(pending) = &self.pending {
            let entity = cx.entity();
            card = card.child(
                crate::ui::dialog::dialog("restore-warning", "PostgreSQL records are not restored")
                    .child(widgets::text(
                        "restore-warning-text",
                        pending.target.warning.clone().unwrap_or_else(|| POSTGRES_WARNING.to_string()),
                    ))
                    .footer(
                        forms::secondary("restore-warning-cancel", "Cancel")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pending = None;
                                cx.notify();
                            })),
                    )
                    .footer(
                        forms::primary("restore-warning-confirm", "Restore definition only")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(pending) = this.pending.take() {
                                    this.restore(pending, window, cx);
                                }
                            })),
                    )
                    .on_escape(&self.focus, busy, move |_, cx| {
                        entity.update(cx, |this, cx| {
                            this.pending = None;
                            cx.notify();
                        })
                    })
                    .modal(),
            );
        }
        if app.backups_enabled {
            card = card.child(
                forms::actions().child(
                    forms::secondary("backup-start", "Back up to cloud")
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| this.check_size(cx))),
                ),
            );
        }
        if let Some(report) = self.size_check.clone() {
            card = card.child(backup_size_dialog(
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
        if let Some(bar) =
            transfer_bar("versions-progress", "Transfer progress", self.transfer.progress.as_ref(), cx)
        {
            card = card.child(bar);
        }
        card.child(action_status("versions", &self.action, cx)).into_any_element()
    }
}
