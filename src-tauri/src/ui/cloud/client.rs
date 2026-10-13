//! The live cloud services: Supabase Auth, PostgREST and Edge Functions over blocking
//! HTTPS (what supabase-js did in src/cloud/client.ts), plus the Rust backend
//! commands. The session lives only in the local secret store (sealed with the
//! machine key), never in archives or logs.

use super::{
    errors::{CloudError, Failure, from_body, to_cloud_error},
    services::{CloudServices, Result},
    types::*,
};
use crate::{
    assets::ArchiveSizeReport,
    cloud::{
        commands::{self, DesktopAuthPoll, DesktopAuthStart, UploadResult},
        config,
        http::Progress,
        install::LocalInstallation,
        publish::Preflight,
        runtime_commands::{self, CloudRuntimeInfo, GrantResult},
    },
    installation::InstalledBundle,
    manager::SessionState,
};
use reqwest::{Method, blocking::Client};
use serde_json::{Value, json};
use std::{
    sync::{Mutex, OnceLock},
    time::Duration,
};

/// Same key supabase-js used, so sessions stored by earlier versions still load.
const STORAGE_KEY: &str = "sb-ixtable-auth";
/// The window label commands that ignore it are given.
const NO_WINDOW: &str = "main";
/// A session this close to expiring is refreshed before use.
const REFRESH_MARGIN_SECONDS: i64 = 60;

#[derive(Default)]
pub struct LiveServices;

fn http() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        // rustls needs a process-wide crypto provider; ring is the one we ship.
        let _ = rustls::crypto::ring::default_provider().install_default();
        Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(60))
            .user_agent(concat!("ixtable-desktop/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("HTTP client")
    })
}

fn configured() -> Result<config::CloudConfig> {
    config::required().map_err(Into::into)
}

fn offline() -> CloudError {
    to_cloud_error(Failure::Network)
}

// ---- Supabase Auth

fn auth_error(status: u16, body: &str) -> CloudError {
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let message = ["msg", "error_description", "message", "error"]
        .iter()
        .find_map(|k| parsed.get(*k).and_then(Value::as_str))
        .unwrap_or("Sign-in failed")
        .to_string();
    to_cloud_error(Failure::Auth { status: Some(status), message, retryable: status >= 500 })
}

/// `url` with `query` appended, percent-encoded.
fn with_query(url: String, query: &[(&str, String)]) -> String {
    match reqwest::Url::parse_with_params(&url, query) {
        Ok(parsed) => parsed.to_string(),
        Err(_) => url,
    }
}

fn auth_call(
    method: Method,
    path: &str,
    query: &[(&str, String)],
    bearer: Option<&str>,
    body: Option<Value>,
) -> Result<Value> {
    let cfg = configured()?;
    let mut request = http()
        .request(method, with_query(format!("{}/auth/v1/{path}", cfg.url), query))
        .header("apikey", &cfg.anon_key)
        .header("authorization", format!("Bearer {}", bearer.unwrap_or(&cfg.anon_key)));
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().map_err(|_| {
        to_cloud_error(Failure::Auth { status: None, message: String::new(), retryable: true })
    })?;
    let status = response.status().as_u16();
    let text = response.text().map_err(|_| offline())?;
    if !(200..300).contains(&status) {
        return Err(auth_error(status, &text));
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// A session reply with an absolute `expires_at`.
fn stamped(mut session: Value) -> Value {
    if session.get("expires_at").and_then(Value::as_i64).is_none() {
        let seconds = session.get("expires_in").and_then(Value::as_i64).unwrap_or(3600);
        session["expires_at"] = json!(now() + seconds);
    }
    session
}

fn session_of(value: &Value) -> Option<Session> {
    let user = value.get("user")?;
    Some(Session {
        access_token: value.get("access_token")?.as_str()?.to_string(),
        user_id: user.get("id")?.as_str()?.to_string(),
        email: user.get("email").and_then(Value::as_str).filter(|e| !e.is_empty()).map(str::to_string),
    })
}

fn store_session(value: &Value) {
    let _ = commands::cloud_auth_storage_set(NO_WINDOW.into(), STORAGE_KEY.into(), value.to_string());
}

fn clear_session() {
    let _ = commands::cloud_auth_storage_remove(NO_WINDOW.into(), STORAGE_KEY.into());
}

fn stored() -> Option<Value> {
    let raw = commands::cloud_auth_storage_get(NO_WINDOW.into(), STORAGE_KEY.into()).ok()??;
    serde_json::from_str(&raw).ok()
}

/// The `exp` of a JWT (seconds), if it has one.
fn token_expiry(jwt: &str) -> Option<i64> {
    let payload = jwt.split('.').nth(1)?;
    let bytes = crate::cloud::b64_any(payload).ok()?;
    serde_json::from_slice::<Value>(&bytes).ok()?.get("exp")?.as_i64()
}

// ---- Edge Functions and PostgREST

fn function_call(access_token: &str, name: &str, body: &Value) -> Result<Value> {
    let cfg = configured()?;
    let response = http()
        .post(format!("{}/functions/v1/{name}", cfg.url))
        .header("apikey", &cfg.anon_key)
        .header("authorization", format!("Bearer {access_token}"))
        .json(body)
        .send()
        .map_err(|_| offline())?;
    let status = response.status().as_u16();
    let text = response.text().map_err(|_| offline())?;
    let parsed: Option<Value> = serde_json::from_str(&text).ok();
    if !(200..300).contains(&status) {
        return Err(to_cloud_error(Failure::Http { status, body: parsed }));
    }
    Ok(parsed.unwrap_or(Value::Null))
}

fn is_postgres_code(code: &str) -> bool {
    let pgrst = code.strip_prefix("PGRST").is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()));
    let sqlstate = code.len() == 5
        && code.chars().all(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
        && code.chars().any(|c| c.is_ascii_digit());
    pgrst || sqlstate
}

fn rest_error(status: u16, body: &str) -> CloudError {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    if let Some(v) = &parsed {
        if let Some(code) = v.get("code").and_then(Value::as_str).filter(|c| is_postgres_code(c)) {
            let message = v.get("message").and_then(Value::as_str).unwrap_or("").to_string();
            return to_cloud_error(Failure::Postgrest { code: code.into(), message });
        }
    }
    from_body(status, parsed.as_ref().unwrap_or(&Value::Null))
}

/// The JWT used for reads: the signed-in user's, else the anon key.
fn read_token(cfg: &config::CloudConfig) -> String {
    LiveServices.load_session().map_or_else(|| cfg.anon_key.clone(), |s| s.access_token)
}

fn rest_request(
    method: Method,
    table: &str,
    query: &[(&str, String)],
    extra: &[(&str, &str)],
    body: Option<Value>,
) -> Result<reqwest::blocking::Response> {
    let cfg = configured()?;
    let mut request = http()
        .request(method, with_query(format!("{}/rest/v1/{table}", cfg.url), query))
        .header("apikey", &cfg.anon_key)
        .header("authorization", format!("Bearer {}", read_token(&cfg)));
    for (k, v) in extra {
        request = request.header(*k, *v);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().map_err(|_| offline())?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let text = response.text().unwrap_or_default();
        return Err(rest_error(status, &text));
    }
    Ok(response)
}

fn rows(table: &str, query: &[(&str, String)]) -> Result<Vec<Value>> {
    let response = rest_request(Method::GET, table, query, &[], None)?;
    let value: Value = response.json().map_err(|_| offline())?;
    Ok(value.as_array().cloned().unwrap_or_default())
}

/// A to-one embed arrives as an object (or a one-element array without schema types).
fn one(value: Option<&Value>) -> Option<&Value> {
    match value {
        Some(Value::Array(items)) => items.first(),
        Some(Value::Null) | None => None,
        Some(other) => Some(other),
    }
}

fn text(row: &Value, key: &str) -> String {
    row.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn eq(value: &str) -> String {
    format!("eq.{value}")
}

fn require_session() -> Result<Session> {
    LiveServices
        .load_session()
        .ok_or_else(|| CloudError::new("UNAUTHENTICATED", "Sign in to ixtable Cloud first."))
}

fn run<T: Send + 'static>(
    work: impl std::future::Future<Output = std::result::Result<T, crate::manager::AppError>>,
) -> Result<T> {
    tauri::async_runtime::block_on(work).map_err(Into::into)
}

impl CloudServices for LiveServices {
    fn load_session(&self) -> Option<Session> {
        // One refresh at a time: a refresh token is single-use.
        static REFRESHING: Mutex<()> = Mutex::new(());
        let _guard = REFRESHING.lock().unwrap_or_else(|e| e.into_inner());
        let current = stored()?;
        let expires = current.get("expires_at").and_then(Value::as_i64).unwrap_or(0);
        if expires - now() > REFRESH_MARGIN_SECONDS {
            return session_of(&current);
        }
        let refresh = current.get("refresh_token")?.as_str()?.to_string();
        match auth_call(
            Method::POST,
            "token",
            &[("grant_type", "refresh_token".into())],
            None,
            Some(json!({ "refresh_token": refresh })),
        ) {
            Ok(next) => {
                let next = stamped(next);
                store_session(&next);
                session_of(&next)
            }
            // A refused refresh token ends the session; being offline keeps it stored.
            Err(e) if e.code == "UNAUTHENTICATED" => {
                clear_session();
                None
            }
            Err(_) => None,
        }
    }

    fn sign_in(&self, email: &str, password: &str) -> Result<Option<Session>> {
        let reply = auth_call(
            Method::POST,
            "token",
            &[("grant_type", "password".into())],
            None,
            Some(json!({ "email": email, "password": password })),
        )?;
        let session = stamped(reply);
        store_session(&session);
        Ok(session_of(&session))
    }

    fn sign_up(&self, email: &str, password: &str) -> Result<Option<Session>> {
        let reply = auth_call(
            Method::POST,
            "signup",
            &[],
            None,
            Some(json!({ "email": email, "password": password })),
        )?;
        if reply.get("access_token").is_none() {
            return Ok(None);
        }
        let session = stamped(reply);
        store_session(&session);
        Ok(session_of(&session))
    }

    fn request_password_reset(&self, email: &str) -> Result<()> {
        let site = configured()?.site_url;
        let query = if site.is_empty() {
            vec![]
        } else {
            vec![("redirect_to", format!("{}/reset-password", site.trim_end_matches('/')))]
        };
        auth_call(Method::POST, "recover", &query, None, Some(json!({ "email": email }))).map(|_| ())
    }

    fn adopt_session(&self, tokens: SessionTokens) -> Result<Option<Session>> {
        let user = auth_call(Method::GET, "user", &[], Some(&tokens.access_token), None)?;
        let expires = token_expiry(&tokens.access_token).unwrap_or_else(|| now() + 3600);
        let session = json!({
            "access_token": tokens.access_token,
            "refresh_token": tokens.refresh_token,
            "token_type": "bearer",
            "expires_at": expires,
            "user": user,
        });
        store_session(&session);
        Ok(session_of(&session))
    }

    fn sign_out(&self) {
        let _ = commands::cloud_sign_out_local(NO_WINDOW.into());
        if let Some(current) = stored() {
            if let Some(token) = current.get("access_token").and_then(Value::as_str) {
                let _ = auth_call(Method::POST, "logout", &[("scope", "local".into())], Some(token), None);
            }
        }
        clear_session();
    }

    fn invoke(&self, name: &str, body: Value) -> Result<Value> {
        let session = require_session()?;
        function_call(&session.access_token, name, &body)
    }

    fn list_organizations(&self) -> Result<Vec<Organization>> {
        let session = require_session()?;
        let data = rows(
            "org_members",
            &[("select", "role, organizations(id, name)".into()), ("user_id", eq(&session.user_id))],
        )?;
        Ok(data
            .iter()
            .filter_map(|row| {
                let org = one(row.get("organizations"))?;
                Some(Organization { id: text(org, "id"), name: text(org, "name"), role: text(row, "role") })
            })
            .collect())
    }

    fn create_organization(&self, name: &str) -> Result<Organization> {
        let session = require_session()?;
        let response = rest_request(
            Method::POST,
            "organizations",
            &[("select", "id, name".into())],
            &[("prefer", "return=representation"), ("accept", "application/vnd.pgrst.object+json")],
            Some(json!({ "name": name, "created_by": session.user_id })),
        )?;
        let org: Value = response.json().map_err(|_| offline())?;
        Ok(Organization { id: text(&org, "id"), name: text(&org, "name"), role: "owner".into() })
    }

    fn get_app(&self, app_id: &str) -> Result<Option<CloudApp>> {
        let data = rows(
            "cloud_apps",
            &[
                ("select", "id, org_id, owner_id, name, document_id, backups_enabled, head_version_id".into()),
                ("id", eq(app_id)),
            ],
        )?;
        Ok(data.first().map(|row| CloudApp {
            id: text(row, "id"),
            org_id: text(row, "org_id"),
            owner_id: text(row, "owner_id"),
            name: text(row, "name"),
            document_id: text(row, "document_id"),
            backups_enabled: row.get("backups_enabled").and_then(Value::as_bool).unwrap_or(false),
            head_version_id: row.get("head_version_id").and_then(Value::as_str).map(str::to_string),
        }))
    }

    fn list_versions(&self, app_id: &str) -> Result<Vec<AppVersion>> {
        let data = rows(
            "app_versions",
            &[
                (
                    "select",
                    "id, version, created_at, archive_sha256, archive_size, release_notes, status, min_runtime_version, resolution"
                        .into(),
                ),
                ("app_id", eq(app_id)),
                ("order", "created_at.desc".into()),
            ],
        )?;
        Ok(data
            .iter()
            .map(|row| AppVersion {
                id: text(row, "id"),
                version: text(row, "version"),
                created_at: text(row, "created_at"),
                archive_sha256: text(row, "archive_sha256"),
                archive_size: row.get("archive_size").and_then(Value::as_u64).unwrap_or(0),
                release_notes: text(row, "release_notes"),
                status: text(row, "status"),
                min_runtime_version: text(row, "min_runtime_version"),
                resolution: row.get("resolution").and_then(Value::as_str).map(str::to_string),
            })
            .collect())
    }

    fn list_installation_backups(&self, app_id: &str, installation_id: &str) -> Result<Vec<InstallationBackup>> {
        let data = rows(
            "installation_backups",
            &[
                ("select", "id, installation_id, archive_sha256, archive_size, created_at".into()),
                ("app_id", eq(app_id)),
                ("installation_id", eq(installation_id)),
                ("order", "created_at.desc".into()),
            ],
        )?;
        data.iter().map(super::contract::backup_row).collect()
    }

    fn version_release_notes(&self, version_id: &str) -> Result<String> {
        let data = rows("app_versions", &[("select", "release_notes".into()), ("id", eq(version_id))])?;
        Ok(data.first().map(|row| text(row, "release_notes")).unwrap_or_default())
    }

    fn runtime_capacity(&self, app_id: &str) -> Result<u32> {
        let members = self.count_members(app_id).unwrap_or(0);
        let data = rows(
            "subscriptions",
            &[("select", "status, plans(runtime_user_allowance)".into()), ("app_id", eq(app_id))],
        )
        .unwrap_or_default();
        let allowance = data
            .first()
            .and_then(|row| one(row.get("plans")))
            .and_then(|plan| plan.get("runtime_user_allowance"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        Ok(members.max(allowance))
    }

    fn list_member_apps(&self) -> Result<Vec<MemberApp>> {
        let session = require_session()?;
        let members = rows(
            "app_members",
            &[
                ("select", "app_id, role_id, status, cloud_apps(id, name)".into()),
                ("user_id", eq(&session.user_id)),
                ("status", eq("active")),
            ],
        )?;
        let roles = rows("app_roles", &[("select", "app_id, id, name".into())]).unwrap_or_default();
        Ok(members
            .iter()
            .filter_map(|row| {
                let app = one(row.get("cloud_apps"))?;
                let (app_id, role_id) = (text(row, "app_id"), text(row, "role_id"));
                let role_name = roles
                    .iter()
                    .find(|r| text(r, "app_id") == app_id && text(r, "id") == role_id)
                    .map(|r| text(r, "name"))
                    .unwrap_or_default();
                Some(MemberApp { app_id, name: text(app, "name"), role_id, role_name })
            })
            .collect())
    }

    fn desktop_auth_start(&self, window: &str, provider: &str) -> Result<DesktopAuthStart> {
        commands::cloud_desktop_auth_start(window.into(), Some(provider.into())).map_err(Into::into)
    }

    fn desktop_auth_poll(&self, window: &str, state: &str) -> Result<DesktopAuthPoll> {
        run(commands::cloud_desktop_auth_poll(window.into(), state.into()))
    }

    fn publish_preflight(&self, window: &str, runtime_users: u32) -> Result<Preflight> {
        run(commands::cloud_publish_preflight(window.into(), Some(runtime_users)))
    }

    fn upload_archive(
        &self,
        window: &str,
        access_token: &str,
        app_id: &str,
        kind: &str,
        transfer_id: &str,
    ) -> Result<UploadResult> {
        run(commands::cloud_upload_archive(
            window.into(),
            access_token.into(),
            app_id.into(),
            kind.into(),
            Some(transfer_id.into()),
        ))
    }

    fn upload_credential(
        &self,
        window: &str,
        access_token: &str,
        app_id: &str,
        scope: &str,
        user_id: Option<String>,
        password: Option<String>,
        username: Option<String>,
    ) -> Result<()> {
        run(commands::cloud_upload_credential(
            window.into(),
            access_token.into(),
            app_id.into(),
            scope.into(),
            user_id,
            password,
            username,
        ))
        .map(|_| ())
    }

    fn restore_copy(
        &self,
        window: &str,
        signed_url: &str,
        sha256: &str,
        size: u64,
        version_id: Option<String>,
        transfer_id: &str,
    ) -> Result<SessionState> {
        run(commands::cloud_restore_copy(
            window.into(),
            signed_url.into(),
            sha256.into(),
            size,
            version_id,
            Some(transfer_id.into()),
        ))
    }

    fn transfer_progress(&self, transfer_id: &str) -> Option<Progress> {
        commands::cloud_transfer_progress(NO_WINDOW.into(), transfer_id.into()).ok().flatten()
    }

    fn installed_apps(&self, window: &str) -> Result<Vec<LocalInstallation>> {
        runtime_commands::cloud_installed_apps(window.into()).map_err(Into::into)
    }

    fn install_app(
        &self,
        window: &str,
        access_token: &str,
        app_id: &str,
        user_id: &str,
        email: &str,
        transfer_id: &str,
    ) -> Result<SessionState> {
        run(runtime_commands::cloud_install_app(
            window.into(),
            access_token.into(),
            app_id.into(),
            user_id.into(),
            email.into(),
            Some(transfer_id.into()),
        ))
    }

    fn open_installed(&self, window: &str, app_id: &str) -> Result<SessionState> {
        runtime_commands::cloud_open_installed(window.into(), app_id.into()).map_err(Into::into)
    }

    fn runtime_info(&self, window: &str) -> Result<CloudRuntimeInfo> {
        runtime_commands::cloud_runtime_info(window.into()).map_err(Into::into)
    }

    fn key_grant(&self, window: &str, access_token: &str) -> Result<GrantResult> {
        run(runtime_commands::cloud_key_grant(window.into(), access_token.into()))
    }

    fn release_credentials(&self, window: &str) -> Result<()> {
        runtime_commands::cloud_release_credentials(window.into()).map_err(Into::into)
    }

    fn archive_size_report(&self, window: &str) -> Result<ArchiveSizeReport> {
        crate::assets::archive_size_report(window.into()).map_err(Into::into)
    }

    fn runtime_installation_info(&self, window: &str) -> Result<InstalledBundle> {
        crate::installation_commands::runtime_installation_info(window.into()).map_err(Into::into)
    }
}

impl LiveServices {
    /// Active runtime members of an app.
    fn count_members(&self, app_id: &str) -> Result<u32> {
        let response = rest_request(
            Method::HEAD,
            "app_members",
            &[("select", "user_id".into()), ("app_id", eq(app_id)), ("status", eq("active"))],
            &[("prefer", "count=exact")],
            None,
        )?;
        let range = response
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        Ok(range.rsplit('/').next().and_then(|n| n.parse().ok()).unwrap_or(0))
    }
}
