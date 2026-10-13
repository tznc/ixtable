//! Opening a cloud app for its runtime user (PRD 22.3): install on first use, otherwise
//! `sync-check` and, when a newer checkpoint is published, update through the verified
//! install flow before opening. Offline, the last installed version runs. Port of
//! src/cloud/open.ts.
//!
//! Every function here blocks: call it from `task::spawn_call`.

use super::{
    contract,
    errors::CloudError,
    services::CloudServices,
    session::CloudRuntime,
    types::SyncCheck,
};
use crate::{
    cloud::runtime_commands::CloudRuntimeInfo,
    manager::SessionState,
    ui::{prelude::*, shell::{Shell, ShellEvent}},
};
use serde_json::json;
use std::sync::Arc;

pub struct OpenResult {
    pub state: SessionState,
    pub info: Arc<CloudRuntimeInfo>,
    pub notice: String,
}

/// Codes that mean "the cloud could not be asked", so the installed version may run.
const OFFLINE: [&str; 3] = ["CLOUD_OFFLINE", "CLOUD_TIMEOUT", "CLOUD_UNAVAILABLE"];

/// Post-update notice (PRD 22.3): the version, the migrations that ran on this
/// installation's records, and the release notes. Notes and migrations are best effort;
/// the update itself already succeeded.
pub fn update_notice(
    services: &dyn CloudServices,
    window: &str,
    version_id: Option<&str>,
    version: &str,
) -> String {
    let info = services.runtime_installation_info(window).ok();
    // The record says this apply did not change versions (e.g. already installed).
    if let Some(info) = &info {
        if !matches!(info.last_action.as_deref(), Some("update" | "downgrade")) {
            return format!("Running version {}.", info.version);
        }
    }
    let notes = version_id
        .and_then(|id| services.version_release_notes(id).ok())
        .unwrap_or_default();
    let migrations = info.map(|i| i.applied_migrations).unwrap_or_default();
    let mut text = format!("Updated to version {version}. Your records were kept.");
    if migrations.is_empty() {
        text.push_str(" No migrations were needed.");
    } else {
        text.push_str(&format!(" Migrations applied: {}.", migrations.join(", ")));
    }
    if !notes.trim().is_empty() {
        text.push_str(&format!("\nRelease notes:\n{}", notes.trim()));
    }
    text
}

/// Opens (installing or updating first) the cloud app `app_id` in window `window`.
pub fn open_cloud_app(
    services: &dyn CloudServices,
    window: &str,
    app_id: &str,
    transfer_id: &str,
) -> Result<OpenResult, CloudError> {
    let installed = services.installed_apps(window)?.into_iter().find(|a| a.app_id == app_id);
    let install = || -> Result<SessionState, CloudError> {
        let session = services.require_session()?;
        services.install_app(
            window,
            &session.access_token,
            app_id,
            &session.user_id,
            session.email.as_deref().unwrap_or(""),
            transfer_id,
        )
    };
    let (state, notice) = match installed {
        None => (install()?, "Installed from ixtable Cloud.".to_string()),
        Some(installed) => {
            let mut notice = String::new();
            let checked = services
                .invoke(
                    "sync-check",
                    json!({
                        "appId": app_id,
                        "installedVersionId": installed.version_id,
                        "installationId": installed.installation_id,
                    }),
                )
                .and_then(|reply| contract::sync_check(&reply));
            let sync: Option<SyncCheck> = match checked {
                Ok(sync) => Some(sync),
                Err(e) if OFFLINE.contains(&e.code.as_str()) => {
                    notice = format!(
                        "Offline: running the installed version {}.",
                        installed.version.as_deref().unwrap_or("")
                    )
                    .replace(" .", ".");
                    None
                }
                Err(e) => return Err(e),
            };
            match sync.filter(|s| !s.up_to_date) {
                Some(sync) => match install() {
                    Ok(state) => {
                        let latest = sync.latest.as_ref();
                        let version = latest
                            .map(|l| l.version.clone())
                            .or_else(|| state.bundle_version.clone())
                            .unwrap_or_default();
                        let notice = update_notice(
                            services,
                            window,
                            latest.map(|l| l.version_id.as_str()),
                            &version,
                        );
                        (state, notice)
                    }
                    Err(e) => {
                        // A failed update leaves the previous version installed; run it and say why.
                        if e.code == "REVOKED" || e.code == "FORBIDDEN" {
                            return Err(e);
                        }
                        let state = services.open_installed(window, app_id)?;
                        let notice = format!(
                            "The update could not be installed ({}). Running version {}.",
                            e.message,
                            installed.version.as_deref().unwrap_or("")
                        );
                        (state, notice)
                    }
                },
                None => (services.open_installed(window, app_id)?, notice),
            }
        }
    };
    let info = services.runtime_info(window)?;
    if info.app_id != app_id {
        return Err(CloudError::new("INSTALLATION_CORRUPT", "Opened the wrong application."));
    }
    Ok(OpenResult { state, info: Arc::new(info), notice })
}

/// Makes `result` the cloud runtime of the window (its role takes effect).
pub fn adopt_runtime(result: &OpenResult, cx: &mut App) {
    let runtime = CloudRuntime::global(cx);
    runtime.update(cx, |r, cx| r.set(&result.state.session_id, result.info.clone(), cx));
}

/// Opens `state` in the window that `shell` shows (replacing its session).
pub fn open_in_window(shell: &Entity<Shell>, state: SessionState, cx: &mut App) {
    shell.update(cx, |_, cx| cx.emit(ShellEvent::Opened(state)));
}

/// A notice to show in the window that opens next (the app frame reads it once).
struct OpenNotice(String);
impl Global for OpenNotice {}

pub fn set_open_notice(notice: &str, cx: &mut App) {
    cx.set_global(OpenNotice(notice.to_string()));
}

/// The notice left by `set_open_notice`, once.
pub fn take_open_notice(cx: &mut App) -> Option<String> {
    cx.try_global::<OpenNotice>().map(|n| n.0.clone()).filter(|n| !n.is_empty()).inspect(|_| {
        cx.set_global(OpenNotice(String::new()));
    })
}
