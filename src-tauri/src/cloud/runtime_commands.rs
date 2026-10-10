//! Tauri commands for cloud runtime users: install/update from a signed
//! personalized manifest, open the installed version offline, runtime
//! identity for the runtime bar and RBAC, and datasource key grants.
use super::{
    config, envelope, err, grants, http,
    install::{self, CloudRecord, LocalInstallation},
    manifest::{self, Expect},
};
use crate::manager::{AppError, SessionState};
use crate::recordstore::secrets::datasource_target;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::{json, Value};

/// Cloud apps installed on this computer (for offline use).
#[tauri::command]
pub fn cloud_installed_apps(window_label: String) -> Result<Vec<LocalInstallation>, AppError> {
    let _ = window_label;
    let Ok(entries) = std::fs::read_dir(install::cloud_root()) else {
        return Ok(vec![]);
    };
    let mut apps: Vec<LocalInstallation> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|id| crate::paths::is_safe_id(id))
        .filter_map(|id| install::read_local(&id).ok().flatten())
        .filter(|l| install::installed_dir(&l.app_id).ok().flatten().is_some())
        .collect();
    apps.sort_by(|a, b| a.app_name.cmp(&b.app_name));
    Ok(apps)
}

/// Installs or updates a cloud app to its newest published version and opens
/// it: `bundle-manifest` → manifest verification (pinned key, expiry,
/// identity) → streaming download (size-capped) → SHA-256 check → the
/// runtime installation flow (records preserved, migrations, health checks,
/// atomic activation, revert on failure).
#[tauri::command]
pub async fn cloud_install_app(
    window_label: String,
    access_token: String,
    app_id: String,
    user_id: String,
    email: String,
    transfer_id: Option<String>,
) -> Result<SessionState, AppError> {
    http::offload(move || {
        let cfg = config::required()?;
        let key = config::public_key()?;
        let local = install::local_installation(&app_id)?;
        let reply = http::call_function(
            &cfg,
            Some(&access_token),
            "bundle-manifest",
            &json!({
                "appId": app_id,
                "installationId": local.installation_id,
                "deviceName": local.device_name,
            }),
        )?;
        let reply: super::contract::BundleReply =
            super::contract::decode("bundle-manifest", reply)?;
        let signature = reply.signature;
        let verified = manifest::verify(
            &reply.manifest,
            &signature,
            &key,
            Utc::now(),
            &Expect {
                app_id: &app_id,
                user_id: &user_id,
                installation_id: &local.installation_id,
            },
        )?;
        let url = reply.archive_url.as_str();
        let incoming = install::cloud_root().join(".incoming");
        let download = http::download(
            &cfg,
            url,
            &incoming,
            verified.archive_size.min(super::MAX_DOWNLOAD_BYTES),
            &transfer_id.unwrap_or_default(),
        )?;
        manifest::check_archive(&verified, &download.sha256, download.size)?;
        install::install_verified(
            &window_label,
            &verified,
            &reply.manifest,
            &signature,
            &email,
            &download.path,
            &config::base64_key(&key),
        )
    })
    .await
}

/// Opens the installed version of a cloud app without contacting the cloud.
#[tauri::command]
pub fn cloud_open_installed(
    window_label: String,
    app_id: String,
) -> Result<SessionState, AppError> {
    install::open_installed(&window_label, &app_id)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudRuntimeInfo {
    pub app_id: String,
    pub app_name: String,
    pub version_id: String,
    pub version: String,
    pub user_id: String,
    pub email: String,
    /// Assigned runtime role (None: nothing is allowed unless `owner`).
    pub role_id: Option<String>,
    pub role_name: Option<String>,
    pub role_permissions: Value,
    /// Signed owner flag from the manifest: developer access.
    pub owner: bool,
    pub installation_id: String,
    pub fingerprint: String,
    pub issued_at: String,
    pub expires_at: String,
    pub public_key_fingerprint: String,
    pub postgres: bool,
    pub datasource_id: String,
    pub grant_expires_at: Option<String>,
}

fn info_from(record: CloudRecord, window: &str) -> Result<CloudRuntimeInfo, AppError> {
    let ds = crate::manager()?.config(window)?.datasource;
    let m = record.manifest;
    Ok(CloudRuntimeInfo {
        app_id: m.app_id,
        app_name: m.app_name,
        version_id: m.version_id,
        version: m.version,
        user_id: m.user_id,
        email: record.email,
        role_id: m.role_id,
        role_name: m.role_name,
        role_permissions: m.role_permissions,
        owner: m.owner,
        installation_id: m.installation_id,
        fingerprint: m.fingerprint,
        issued_at: m.issued_at,
        expires_at: m.expires_at,
        public_key_fingerprint: record.public_key_fingerprint,
        postgres: ds.is_postgres(),
        grant_expires_at: grants::expires_at(&ds).map(|t| t.to_rfc3339()),
        datasource_id: ds.id,
    })
}

/// Identity of the open cloud installation (runtime bar, RBAC role).
#[tauri::command]
pub fn cloud_runtime_info(window_label: String) -> Result<CloudRuntimeInfo, AppError> {
    let dir = install::session_dir(&window_label)?;
    // Owner flag and role come only from the re-verified signed manifest.
    info_from(install::verified_record(&dir)?, &window_label)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantResult {
    pub needed: bool,
    pub expires_at: Option<String>,
    pub attached: bool,
    pub error: Option<String>,
}

fn reattach(window: &str) -> Result<crate::recordstore::commands::DatasourceStatus, AppError> {
    crate::recordstore::commands::connect_datasource(window.to_string())
}

/// Opens a key grant's credential envelope for `ds` and returns the
/// (per-user username, password) it carries. The envelope must be sealed for
/// this application, datasource, and datasource target; a v2 per-user
/// username must be a valid PostgreSQL user.
pub(crate) fn open_credential(
    app_id: &str,
    ds: &crate::recordstore::DatasourceConfig,
    reply: &super::contract::GrantReply,
) -> Result<(Option<String>, String), AppError> {
    let aad = reply.envelope.aad.clone();
    let prefix = format!("ixtable-credential/1|{}|{}|", app_id, ds.id);
    if !aad.starts_with(&prefix) {
        return Err(err(
            "CREDENTIAL_DECRYPT",
            "The credential envelope is for another application or datasource",
        ));
    }
    let plain = envelope::open(
        &reply.envelope.ciphertext,
        &reply.envelope.nonce,
        &aad,
        &reply.dek,
    )?;
    let cred: envelope::Credential = serde_json::from_slice(&plain).map_err(|_| {
        err(
            "CREDENTIAL_DECRYPT",
            "The decrypted credential is unreadable",
        )
    })?;
    drop(plain);
    if cred.target != datasource_target(ds) {
        return Err(err(
            "CREDENTIAL_TARGET_MISMATCH",
            "The delivered credential was sealed for another database server or user; ask the developer to upload it again",
        ));
    }
    let user = cred.user.filter(|u| !u.is_empty());
    if let Some(u) = &user {
        crate::recordstore::secrets::validate_login_user(u).map_err(|_| {
            err(
                "CREDENTIAL_DECRYPT",
                "The delivered credential names an invalid database user",
            )
        })?;
    }
    Ok((user, cred.password))
}

/// Requests a key grant for the open cloud installation's PostgreSQL
/// datasource, decrypts the credential envelope in memory, and connects.
/// A REVOKED/FORBIDDEN answer drops any held credential and disconnects.
#[tauri::command]
pub async fn cloud_key_grant(
    window_label: String,
    access_token: String,
) -> Result<GrantResult, AppError> {
    http::offload(move || {
        let dir = install::session_dir(&window_label)?;
        let record = install::verified_record(&dir)?;
        let ds = crate::manager()?.config(&window_label)?.datasource;
        if !ds.is_postgres() {
            return Ok(GrantResult { needed: false, expires_at: None, attached: true, error: None });
        }
        let target = datasource_target(&ds);
        let cfg = config::required()?;
        let m = &record.manifest;
        let reply = http::call_function(
            &cfg,
            Some(&access_token),
            "key-grant",
            &json!({ "appId": m.app_id, "installationId": m.installation_id, "datasourceId": ds.id }),
        );
        let reply = match reply {
            Ok(r) => r,
            Err(e) => {
                if matches!(e.code.as_str(), "REVOKED" | "FORBIDDEN" | "ENTITLEMENT_REQUIRED" | "NOT_FOUND") {
                    grants::clear(&window_label, &target);
                    let _ = reattach(&window_label);
                    crate::logging::warn("cloud", &format!("key grant refused: {}", e.code));
                }
                return Err(e);
            }
        };
        let reply: super::contract::GrantReply = super::contract::decode("key-grant", reply)?;
        let (user, password) = open_credential(&m.app_id, &ds, &reply)?;
        // Never trust a grant longer than the 24-hour renewal interval.
        let max = Utc::now() + Duration::hours(24);
        let expires = reply
            .expires_at
            .as_deref()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or(max)
            .min(max);
        grants::put(&window_label, &target, user, password, expires);
        let status = reattach(&window_label)?;
        Ok(GrantResult {
            needed: true,
            expires_at: Some(expires.to_rfc3339()),
            attached: status.attached,
            error: status.error,
        })
    })
    .await
}

/// Drops the credential held for the window's datasource (sign-out, revoke).
#[tauri::command]
pub fn cloud_release_credentials(window_label: String) -> Result<(), AppError> {
    if let Ok(config) = crate::manager()?.config(&window_label) {
        grants::clear(&window_label, &datasource_target(&config.datasource));
        if config.datasource.is_postgres() {
            let _ = reattach(&window_label);
        }
    }
    Ok(())
}
