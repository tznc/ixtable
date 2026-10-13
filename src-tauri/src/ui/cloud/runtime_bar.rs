//! Sidebar block of a cloud runtime session: licensee and fingerprint, role, sync
//! (auto-update), PostgreSQL key grant with renewal, and installation backups. Port of
//! src/cloud/CloudRuntimeBar.tsx.
//!
//! Test ids: `cloud-runtime` (kicker, the region), `cloud-runtime-name`,
//! `cloud-runtime-version` ("Version 1.0.0"), `cloud-runtime-role` ("Role: Sales"),
//! `cloud-license` ("Licensed to EMAIL · fingerprint FP"), `cloud-grant` (database access
//! status), `cloud-revoked`, `cloud-sync` ("Sync now"), `cloud-busy`,
//! `cloud-update-notice`, `cloud-runtime-error`, transfer bar `cloud-transfer`; the
//! backups add theirs (`backups`).

use super::{
    backups::{BackupsEvent, InstallationBackups},
    errors::{CloudError, cloud_error_notice},
    open::{adopt_runtime, open_cloud_app, open_in_window, set_open_notice},
    services::{CloudServices, Services},
    session::{CloudRuntime, RuntimeChanged},
    transfer::{Transfer, transfer_bar},
};
use crate::{
    cloud::runtime_commands::CloudRuntimeInfo,
    ui::{forms, prelude::*, shell::Shell},
};
use std::{sync::Arc, time::Duration};

/// Renew the 24-hour key grant this long before it expires.
const RENEW_BEFORE: Duration = Duration::from_secs(3600);
const MIN_WAIT: Duration = Duration::from_secs(60);

fn short(fingerprint: &str) -> String {
    fingerprint.chars().take(16).collect()
}

/// How long until the grant expiring at `expires_at` should be renewed (never under a
/// minute, so a failing renewal cannot spin).
pub fn renew_wait(expires_at: &str, now: chrono::DateTime<chrono::Utc>) -> Option<Duration> {
    let expiry = chrono::DateTime::parse_from_rfc3339(expires_at).ok()?.with_timezone(&chrono::Utc);
    let remaining = (expiry - now).to_std().unwrap_or(Duration::ZERO);
    Some(remaining.saturating_sub(RENEW_BEFORE).max(MIN_WAIT))
}

pub struct CloudRuntimeBar {
    shell: Entity<Shell>,
    services: Arc<dyn CloudServices>,
    runtime: Entity<CloudRuntime>,
    // The installation the grant and backups below were set up for.
    for_session: Option<String>,
    grant_expires: Option<String>,
    grant_error: Option<String>,
    revoked: bool,
    backups_enabled: bool,
    backups: Option<Entity<InstallationBackups>>,
    renew: Option<Task<()>>,
    busy: String,
    notice: String,
    error: Option<CloudError>,
    transfer: Transfer,
    _subscriptions: Vec<Subscription>,
}

impl CloudRuntimeBar {
    pub fn new(shell: Entity<Shell>, cx: &mut Context<Self>) -> Self {
        let runtime = CloudRuntime::global(cx);
        let subscriptions = vec![
            cx.subscribe(&runtime, |this, _, _: &RuntimeChanged, cx| this.sync_installation(cx)),
            cx.observe(&runtime, |_, _, cx| cx.notify()),
        ];
        let mut this = Self {
            shell,
            services: Services::get(cx),
            runtime,
            for_session: None,
            grant_expires: None,
            grant_error: None,
            revoked: false,
            backups_enabled: false,
            backups: None,
            renew: None,
            busy: String::new(),
            notice: String::new(),
            error: None,
            transfer: Transfer::default(),
            _subscriptions: subscriptions,
        };
        this.sync_installation(cx);
        this
    }

    fn info(&self, cx: &App) -> Option<Arc<CloudRuntimeInfo>> {
        self.runtime.read(cx).active().map(|a| a.info.clone())
    }

    /// Starts what an installation needs (key grant, backup availability) when the
    /// installation shown changed.
    fn sync_installation(&mut self, cx: &mut Context<Self>) {
        let active = self.runtime.read(cx).active().map(|a| (a.session_id.clone(), a.info.clone()));
        let Some((session_id, info)) = active else {
            self.for_session = None;
            return;
        };
        if self.for_session.as_deref() == Some(session_id.as_str()) {
            return;
        }
        self.for_session = Some(session_id);
        self.grant_expires = None;
        self.grant_error = None;
        self.revoked = false;
        self.backups = None;
        self.backups_enabled = false;
        self.request_grant(cx);
        let (services, app_id) = (self.services.clone(), info.app_id.clone());
        task::spawn_call(cx, move || services.get_app(&app_id), |this, result, cx| {
            this.backups_enabled = matches!(result, Ok(Some(app)) if app.backups_enabled);
            cx.notify();
        });
    }

    /// First grant on open, then renewal before each grant expires (24-hour interval).
    fn request_grant(&mut self, cx: &mut Context<Self>) {
        let Some(info) = self.info(cx) else { return };
        if !info.postgres {
            return;
        }
        let (services, label) = (self.services.clone(), self.shell.read(cx).label().to_string());
        let session_id = self.for_session.clone();
        task::spawn_call(
            cx,
            move || {
                let session = services.require_session()?;
                match services.key_grant(&label, &session.access_token) {
                    Ok(grant) => Ok(grant),
                    Err(e) => {
                        if e.code == "REVOKED" || e.code == "FORBIDDEN" {
                            let _ = services.release_credentials(&label);
                        }
                        Err(e)
                    }
                }
            },
            move |this, result, cx| {
                if this.for_session != session_id {
                    return;
                }
                match result {
                    Ok(grant) => {
                        this.grant_expires = grant.expires_at.clone();
                        this.grant_error = grant.error.clone();
                        this.revoked = false;
                        this.shell.update(cx, |s, cx| s.database_changed(cx));
                        if let Some(wait) = grant.expires_at.as_deref().and_then(|e| renew_wait(e, chrono::Utc::now())) {
                            this.renew = Some(cx.spawn(async move |this, cx| {
                                cx.background_executor().timer(wait).await;
                                let _ = this.update(cx, |this, cx| this.request_grant(cx));
                            }));
                        }
                    }
                    Err(e) => {
                        if e.code == "REVOKED" || e.code == "FORBIDDEN" {
                            this.revoked = true;
                            this.shell.update(cx, |s, cx| s.database_changed(cx));
                        }
                        this.error = Some(e);
                    }
                }
                cx.notify();
            },
        );
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        let Some(info) = self.info(cx) else { return };
        self.busy = "Checking for updates…".into();
        self.error = None;
        self.notice.clear();
        let (services, label) = (self.services.clone(), self.shell.read(cx).label().to_string());
        let transfer_id = self.transfer.begin(services.clone(), |v: &mut Self| &mut v.transfer, cx);
        let active_session = self.for_session.clone();
        task::spawn_call(
            cx,
            move || open_cloud_app(services.as_ref(), &label, &info.app_id, &transfer_id),
            move |this, result, cx| {
                this.transfer.end();
                this.busy.clear();
                match result {
                    Err(e) => this.error = Some(e),
                    Ok(opened) => {
                        this.notice = if opened.notice.is_empty() {
                            "You have the newest published version.".into()
                        } else {
                            opened.notice.clone()
                        };
                        adopt_runtime(&opened, cx);
                        if active_session.as_deref() != Some(opened.state.session_id.as_str()) {
                            set_open_notice(&this.notice, cx);
                            open_in_window(&this.shell, opened.state, cx);
                        }
                    }
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    fn backups_view(&mut self, info: &CloudRuntimeInfo, cx: &mut Context<Self>) -> Option<Entity<InstallationBackups>> {
        if !self.backups_enabled {
            return None;
        }
        if self.backups.is_none() {
            let label = self.shell.read(cx).label().to_string();
            let (app_id, installation_id) = (info.app_id.clone(), info.installation_id.clone());
            let view = cx.new(|cx| InstallationBackups::new(label, app_id, installation_id, cx));
            let shell = self.shell.clone();
            self._subscriptions.push(cx.subscribe(&view, move |_, _, event: &BackupsEvent, cx| {
                let BackupsEvent::Restored(state) = event;
                open_in_window(&shell, state.clone(), cx);
            }));
            self.backups = Some(view);
        }
        self.backups.clone()
    }
}

impl Render for CloudRuntimeBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(info) = self.info(cx) else { return div().into_any_element() };
        let busy = !self.busy.is_empty();
        let mut bar = v_flex()
            .gap_2()
            .p_2()
            .child(widgets::text("cloud-runtime", "CLOUD APPLICATION"))
            .child(widgets::styled_text("cloud-runtime-name", info.app_name.clone(), |d| {
                d.font_weight(FontWeight::SEMIBOLD)
            }))
            .child(widgets::text("cloud-runtime-version", format!("Version {}", info.version)))
            .child(widgets::text(
                "cloud-runtime-role",
                format!("Role: {}", info.role_name.as_deref().unwrap_or("none")),
            ))
            .child(widgets::text(
                "cloud-license",
                format!("Licensed to {} · fingerprint {}", info.email, short(&info.fingerprint)),
            ));
        if info.postgres {
            bar = bar.child(widgets::text(
                "cloud-grant",
                if self.revoked {
                    "Access revoked: the database credential was removed.".to_string()
                } else if let Some(expires) = &self.grant_expires {
                    format!("Database access authorized until {}", super::backups::when(expires))
                } else {
                    self.grant_error.clone().unwrap_or_else(|| "Requesting database access…".into())
                },
            ));
        }
        if self.revoked {
            bar = bar.child(widgets::text(
                "cloud-revoked",
                "Your access to this application was revoked by its developer. Contact them to restore it.",
            ));
        }
        bar = bar.child(
            forms::secondary("cloud-sync", "Sync now")
                .disabled(busy)
                .on_click(cx.listener(|this, _, _, cx| this.sync(cx))),
        );
        if let Some(backups) = self.backups_view(&info, cx) {
            bar = bar.child(backups);
        }
        if busy {
            bar = bar.child(widgets::text("cloud-busy", self.busy.clone()));
        }
        if let Some(progress) =
            transfer_bar("cloud-transfer", "Transfer progress", self.transfer.progress.as_ref(), cx)
        {
            bar = bar.child(progress);
        }
        if !self.notice.is_empty() {
            bar = bar.child(widgets::text("cloud-update-notice", self.notice.clone()));
        }
        if let Some(e) = self.error.as_ref().filter(|_| !self.revoked) {
            bar = bar.child(cloud_error_notice("cloud-runtime-error", e, cx));
        }
        bar.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::renew_wait;
    use chrono::{TimeZone, Utc};
    use std::time::Duration;

    #[test]
    fn renews_an_hour_before_expiry_but_never_sooner_than_a_minute() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(renew_wait("2026-01-02T00:00:00Z", now), Some(Duration::from_secs(23 * 3600)));
        assert_eq!(renew_wait("2026-01-01T00:30:00Z", now), Some(Duration::from_secs(60)));
        assert_eq!(renew_wait("2025-12-31T00:00:00Z", now), Some(Duration::from_secs(60)));
        assert_eq!(renew_wait("soon", now), None);
    }
}
