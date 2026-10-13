//! Tauri commands for ixtable Cloud: configuration, sign-in storage and
//! browser hand-off, publishing (preflight, archive upload, credentials),
//! restore, and transfer progress. Runtime commands are in `runtime_commands`.
use super::{config, envelope, err, http, install, pkce, publish, restore};
use crate::manager::{AppError, SessionState};
use crate::recordstore::secrets::{self, SecretStore};
use serde::Serialize;
use serde_json::{json, Value};

/// Effective cloud URL, anon key, and website URL for supabase-js.
pub fn cloud_config(window_label: String) -> Result<config::CloudConfig, AppError> {
    let _ = window_label;
    Ok(config::current())
}

/// Secret-store id of a supabase-js storage key. Only Supabase auth keys are
/// accepted, so the webview cannot read other local secrets through this.
fn auth_key(key: &str) -> Result<String, AppError> {
    let ok = key.starts_with("sb-")
        && key.len() <= 128
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    if !ok {
        return Err(err("VALIDATION", "Unsupported cloud session key"));
    }
    Ok(format!("cloud-auth:{key}"))
}

fn store() -> Result<SecretStore, AppError> {
    SecretStore::default_location().map_err(|e| err("IO_ERROR", e))
}

/// supabase-js session storage, sealed in the local secret store and bound
/// to the cloud URL (a session for another cloud is never released).
pub fn cloud_auth_storage_get(
    window_label: String,
    key: String,
) -> Result<Option<String>, AppError> {
    let _ = window_label;
    let id = auth_key(&key)?;
    Ok(store()?
        .get_for(&id, &config::current().url)
        .unwrap_or(None))
}
pub fn cloud_auth_storage_set(
    window_label: String,
    key: String,
    value: String,
) -> Result<(), AppError> {
    let _ = window_label;
    let id = auth_key(&key)?;
    store()?
        .put_for(&id, &value, &config::current().url)
        .map_err(|e| err("IO_ERROR", e))
}
pub fn cloud_auth_storage_remove(window_label: String, key: String) -> Result<(), AppError> {
    let _ = window_label;
    let id = auth_key(&key)?;
    store()?.delete(&id).map_err(|e| err("IO_ERROR", e))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAuthStart {
    pub state: String,
    pub url: String,
    pub opened: bool,
}

fn browser_disabled() -> bool {
    cfg!(test) || std::env::var_os("IXTABLE_CLOUD_NO_BROWSER").is_some()
}

/// Starts Google/Microsoft sign-in in the browser (PKCE S256). Returns the
/// approval URL so the UI can offer it when no browser could be opened.
pub fn cloud_desktop_auth_start(
    window_label: String,
    provider: Option<String>,
) -> Result<DesktopAuthStart, AppError> {
    let _ = window_label;
    let cfg = config::required()?;
    let p = pkce::begin();
    let url = pkce::approve_url(&cfg.site_url, &p, provider.as_deref());
    let opened = !browser_disabled() && tauri_plugin_opener::open_url(&url, None::<&str>).is_ok();
    Ok(DesktopAuthStart {
        state: p.state,
        url,
        opened,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAuthPoll {
    /// `pending` (not approved yet) or `approved` (session present).
    pub status: String,
    pub session: Option<Value>,
}

/// Polls `desktop-auth-exchange` with the verifier of a started sign-in.
pub async fn cloud_desktop_auth_poll(
    window_label: String,
    state: String,
) -> Result<DesktopAuthPoll, AppError> {
    let _ = window_label;
    http::offload(move || {
        let verifier = pkce::verifier(&state)
            .ok_or_else(|| err("AUTH_EXPIRED", "This sign-in has expired; start it again"))?;
        let cfg = config::required()?;
        let body = json!({ "state": state, "codeVerifier": verifier });
        match http::call_function(&cfg, None, "desktop-auth-exchange", &body) {
            Ok(reply) => {
                pkce::finish(&state);
                Ok(DesktopAuthPoll {
                    status: "approved".into(),
                    session: Some(super::contract::session(reply)?),
                })
            }
            Err(e) if e.code == "PENDING" => Ok(DesktopAuthPoll {
                status: "pending".into(),
                session: None,
            }),
            Err(e) => {
                pkce::finish(&state);
                Err(e)
            }
        }
    })
    .await
}

/// Forgets in-memory cloud credentials (sign-out).
pub fn cloud_sign_out_local(window_label: String) -> Result<(), AppError> {
    let _ = window_label;
    super::grants::clear_all();
    Ok(())
}

/// Publish preflight: size report, validation, security summary, blockers.
pub async fn cloud_publish_preflight(
    window_label: String,
    runtime_users: Option<u32>,
) -> Result<publish::Preflight, AppError> {
    http::offload(move || publish::preflight(&window_label, runtime_users.unwrap_or(0))).await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadResult {
    pub upload_id: String,
    pub path: String,
    pub sha256: String,
    pub size: u64,
    pub installation_id: Option<String>,
}

fn staged_for(window: &str) -> Result<publish::Staged, AppError> {
    let m = crate::manager()?;
    if !m.state(window)?.runtime_only {
        return publish::stage_saved(window);
    }
    // Runtime installation backup: pack the installation's current state.
    let dir = m
        .recovery_root
        .parent()
        .map(|p| p.join("cloud-upload"))
        .unwrap_or_else(|| std::env::temp_dir().join("ixtable-cloud-upload"));
    crate::paths::ensure_private_dir(&dir).map_err(|e| err("IO_ERROR", e))?;
    let path = dir.join(format!("{}.ixt", uuid::Uuid::new_v4()));
    let snap = m.with_session(window, |s| Ok(m.snapshot(s)))?;
    let mut staged = publish::Staged {
        path,
        sha256: String::new(),
        size: 0,
    };
    m.write_snapshot(&snap, &staged.path)?;
    let (sha, size) = http::hash_file(&staged.path)?;
    if size > super::MAX_ARCHIVE_BYTES {
        return Err(err(
            "TOO_LARGE",
            "The installation backup is larger than 500 MB",
        ));
    }
    staged.sha256 = sha;
    staged.size = size;
    Ok(staged)
}

fn signed_upload_url(reply: &super::contract::UploadUrlReply) -> String {
    let url = &reply.signed_url;
    match reply.token.as_deref() {
        Some(token) if !url.contains("token=") => {
            let sep = if url.contains('?') { '&' } else { '?' };
            format!("{url}{sep}token={token}")
        }
        _ => url.to_string(),
    }
}

/// Uploads the saved archive (Studio) or an installation snapshot (Runtime)
/// to a signed URL from `archive-upload-url`. `kind` is `version` or `backup`.
/// Progress is pollable with `cloud_transfer_progress(transferId)`.
pub async fn cloud_upload_archive(
    window_label: String,
    access_token: String,
    app_id: String,
    kind: String,
    transfer_id: Option<String>,
) -> Result<UploadResult, AppError> {
    http::offload(move || {
        super::check_id("application", &app_id)?;
        if kind != "version" && kind != "backup" {
            return Err(err("VALIDATION", "kind must be version or backup"));
        }
        let cfg = config::required()?;
        let staged = staged_for(&window_label)?;
        let installation_id = if kind == "backup" {
            Some(install::local_installation(&app_id)?.installation_id)
        } else {
            None
        };
        let progress = transfer_id.unwrap_or_default();
        http::set_progress(&progress, 0, staged.size, "prepare");
        let reply = http::call_function(
            &cfg,
            Some(&access_token),
            "archive-upload-url",
            &json!({
                "appId": app_id,
                "kind": kind,
                "size": staged.size,
                "sha256": staged.sha256,
                "installationId": installation_id,
            }),
        )?;
        let reply: super::contract::UploadUrlReply =
            super::contract::decode("archive-upload-url", reply)?;
        http::upload_file(&cfg, &signed_upload_url(&reply), &staged.path, &progress)?;
        crate::logging::info(
            "cloud",
            &format!(
                "uploaded {kind} archive for {app_id} ({} bytes)",
                staged.size
            ),
        );
        Ok(UploadResult {
            upload_id: reply.upload_id,
            path: reply.path,
            sha256: staged.sha256.clone(),
            size: staged.size,
            installation_id,
        })
    })
    .await
}

/// Encrypts the datasource credential with a fresh DEK and sends the envelope
/// to `credential-envelope`. `scope` is `shared` or `user` (then `userId`).
/// Without `password`, the password stored for the datasource is used.
pub async fn cloud_upload_credential(
    window_label: String,
    access_token: String,
    app_id: String,
    scope: String,
    user_id: Option<String>,
    password: Option<String>,
    username: Option<String>,
) -> Result<Value, AppError> {
    http::offload(move || {
        crate::installation::ensure_studio(&window_label)?;
        let cfg = config::required()?;
        let ds = crate::manager()?.config(&window_label)?.datasource;
        if !ds.is_postgres() {
            return Err(err(
                "VALIDATION",
                "Only PostgreSQL datasources have credentials to deliver",
            ));
        }
        let user_id = user_id.filter(|u| !u.is_empty());
        match (scope.as_str(), &user_id) {
            ("shared", None) | ("user", Some(_)) => {}
            _ => {
                return Err(err(
                    "VALIDATION",
                    "Use scope shared, or scope user with a user id",
                ))
            }
        }
        // A per-user credential may name its own database user (PRD §9.3).
        let username = username
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty());
        if let Some(u) = &username {
            if scope != "user" {
                return Err(err(
                    "VALIDATION",
                    "Only a per-user credential can name its own database user",
                ));
            }
            secrets::validate_login_user(u)?;
            if password.as_deref().is_none_or(str::is_empty) {
                return Err(err(
                    "VALIDATION",
                    "Enter the password of that database user",
                ));
            }
        }
        let password = match password.filter(|p| !p.is_empty()) {
            Some(p) => p,
            None => secrets::datasource_credential(&ds)?.ok_or_else(|| {
                err(
                    "CREDENTIAL_MISSING",
                    "Store the datasource password in Datasource settings first",
                )
            })?,
        };
        let plain = serde_json::to_vec(&envelope::Credential {
            v: if username.is_some() { 2 } else { 1 },
            kind: "postgres".into(),
            user: username,
            password,
            target: secrets::datasource_target(&ds),
        })
        .map_err(|e| err("IO_ERROR", e))?;
        let aad = envelope::aad_for(&app_id, &ds.id, &scope, user_id.as_deref());
        let sealed = envelope::seal(&plain, &aad)?;
        drop(plain);
        let reply = http::call_function(
            &cfg,
            Some(&access_token),
            "credential-envelope",
            &json!({
                "appId": app_id,
                "datasourceId": ds.id,
                "scope": scope,
                "userId": user_id,
                "ciphertext": sealed.ciphertext,
                "nonce": sealed.nonce,
                "aad": sealed.aad,
                "dek": sealed.dek,
            }),
        )?;
        Ok(reply)
    })
    .await
}

/// Downloads an archive from a `restore-url` signed URL, verifies its
/// SHA-256 and size, and opens it as a new untitled copy (new document id).
pub async fn cloud_restore_copy(
    window_label: String,
    signed_url: String,
    sha256: String,
    size: u64,
    version_id: Option<String>,
    transfer_id: Option<String>,
) -> Result<SessionState, AppError> {
    http::offload(move || {
        let cfg = config::required()?;
        let scratch = crate::manager()?
            .recovery_root
            .parent()
            .map(|p| p.join("cloud-download"))
            .unwrap_or_else(|| std::env::temp_dir().join("ixtable-cloud-download"));
        let download = http::download(
            &cfg,
            &signed_url,
            &scratch,
            super::MAX_DOWNLOAD_BYTES,
            &transfer_id.unwrap_or_default(),
        )?;
        if !download.sha256.eq_ignore_ascii_case(&sha256) || download.size != size {
            return Err(err(
                "ARCHIVE_CHECKSUM",
                "The downloaded archive does not match its recorded checksum; nothing was restored",
            ));
        }
        restore::open_copy(&window_label, &download.path, version_id.as_deref())
    })
    .await
}

/// Progress of an upload or download started with `transferId`.
pub fn cloud_transfer_progress(
    window_label: String,
    transfer_id: String,
) -> Result<Option<http::Progress>, AppError> {
    let _ = window_label;
    Ok(http::progress(&transfer_id))
}
