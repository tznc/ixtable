//! HTTPS to ixtable Cloud: Edge Function calls and archive transfers.
//!
//! Everything here blocks; commands run it on a blocking thread (`offload`).
//! Error messages never contain URLs (signed URLs carry tokens) or bodies
//! of requests (they may carry credentials).
use super::{config::CloudConfig, err};
use crate::manager::AppError;
use reqwest::blocking::{Body, Client};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex, OnceLock},
    time::Duration,
};

/// Runs blocking cloud work off the async executor (and off the UI thread).
pub async fn offload<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| err("CLOUD_ERROR", e))?
}

fn build(timeout: Option<Duration>) -> Client {
    // rustls needs a process-wide crypto provider; ring is the one we ship.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut builder = Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .user_agent(concat!("ixtable-desktop/", env!("CARGO_PKG_VERSION")));
    builder = match timeout {
        Some(t) => builder.timeout(t),
        None => builder.timeout(None),
    };
    builder.build().expect("HTTP client")
}

/// JSON API calls (60 s).
fn api() -> &'static Client {
    static C: OnceLock<Client> = OnceLock::new();
    C.get_or_init(|| build(Some(Duration::from_secs(60))))
}
/// Archive transfers (no overall timeout).
fn transfer() -> &'static Client {
    static C: OnceLock<Client> = OnceLock::new();
    C.get_or_init(|| build(None))
}

fn net_err(e: reqwest::Error) -> AppError {
    let e = e.without_url();
    if e.is_timeout() {
        err("CLOUD_TIMEOUT", "ixtable Cloud did not answer in time")
    } else if e.is_connect() || e.is_request() {
        err(
            "CLOUD_OFFLINE",
            format!("ixtable Cloud is not reachable ({e})"),
        )
    } else {
        err("CLOUD_ERROR", e)
    }
}

/// Stable code for an HTTP status without a structured error body.
pub fn status_code(status: u16) -> &'static str {
    match status {
        401 => "UNAUTHENTICATED",
        402 => "ENTITLEMENT_REQUIRED",
        403 => "FORBIDDEN",
        404 => "NOT_FOUND",
        409 => "VERSION_CONFLICT",
        413 => "TOO_LARGE",
        422 => "VALIDATION",
        428 => "PENDING",
        429 => "RATE_LIMITED",
        500..=599 => "CLOUD_UNAVAILABLE",
        _ => "CLOUD_ERROR",
    }
}

/// Maps an error response (`{error:{code,message}}` or anything else) to AppError.
pub fn error_from(status: u16, body: &str) -> AppError {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let error = parsed.as_ref().map(|v| match v.get("error") {
        Some(e) if e.is_object() => e.clone(),
        _ => v.clone(),
    });
    let code = error
        .as_ref()
        .and_then(|e| e.get("code"))
        .and_then(Value::as_str)
        .filter(|c| {
            !c.is_empty()
                && c.len() <= 64
                && c.chars().all(|ch| ch.is_ascii_uppercase() || ch == '_')
        })
        .unwrap_or_else(|| status_code(status));
    let message = error
        .as_ref()
        .and_then(|e| e.get("message").or_else(|| e.get("msg")))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("ixtable Cloud answered with HTTP {status}"));
    err(code, message)
}

/// Calls an Edge Function: `POST {url}/functions/v1/{name}` with a JSON body.
pub fn call_function(
    cfg: &CloudConfig,
    access_token: Option<&str>,
    name: &str,
    body: &Value,
) -> Result<Value, AppError> {
    let bearer = access_token.unwrap_or(&cfg.anon_key);
    let response = api()
        .post(format!("{}/functions/v1/{name}", cfg.url))
        .header("apikey", &cfg.anon_key)
        .header("authorization", format!("Bearer {bearer}"))
        .json(body)
        .send()
        .map_err(net_err)?;
    let status = response.status().as_u16();
    let text = response.text().map_err(net_err)?;
    if !(200..300).contains(&status) {
        return Err(error_from(status, &text));
    }
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text)
        .map_err(|e| err("CLOUD_ERROR", format!("Unreadable reply from {name}: {e}")))
}

/// Signed storage URLs minted inside the stack may name an internal host
/// (`http://kong:8000/storage/v1/...`) or be relative; storage paths are
/// always served by the configured API host.
pub fn rebase(cfg_url: &str, signed: &str) -> String {
    let base = cfg_url.trim_end_matches('/');
    if signed.starts_with('/') {
        let prefixed = if signed.starts_with("/storage/v1/") {
            signed.to_string()
        } else {
            format!("/storage/v1{signed}")
        };
        return format!("{base}{prefixed}");
    }
    match reqwest::Url::parse(signed) {
        Ok(u) if u.path().starts_with("/storage/v1/") => {
            let query = u.query().map(|q| format!("?{q}")).unwrap_or_default();
            format!("{base}{}{query}", u.path())
        }
        _ => signed.to_string(),
    }
}

/// Transfer progress by id, polled by the UI (`cloud_transfer_progress`).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub done: u64,
    pub total: u64,
    pub phase: String,
}
static PROGRESS: LazyLock<Mutex<HashMap<String, Progress>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn set_progress(id: &str, done: u64, total: u64, phase: &str) {
    if id.is_empty() {
        return;
    }
    let mut map = PROGRESS.lock().unwrap_or_else(|e| e.into_inner());
    if map.len() > 64 && !map.contains_key(id) {
        map.clear();
    }
    map.insert(
        id.into(),
        Progress {
            done,
            total,
            phase: phase.into(),
        },
    );
}
pub fn progress(id: &str) -> Option<Progress> {
    PROGRESS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(id)
        .cloned()
}

struct Counting<R> {
    inner: R,
    id: String,
    done: u64,
    total: u64,
}
impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.done += n as u64;
        set_progress(&self.id, self.done, self.total, "upload");
        Ok(n)
    }
}

/// Upload attempts before giving up (§23: retryable upload).
pub const UPLOAD_ATTEMPTS: u32 = 4;

/// Whether a failed transfer may succeed when simply sent again: the cloud
/// was unreachable, timed out, overloaded (5xx) or rate limited. Auth,
/// precondition and validation failures (4xx) are final.
pub fn is_transient(e: &AppError) -> bool {
    matches!(
        e.code.as_str(),
        "CLOUD_OFFLINE" | "CLOUD_TIMEOUT" | "CLOUD_UNAVAILABLE" | "RATE_LIMITED"
    )
}

/// Runs `attempt` up to `attempts` times, sleeping `base * 2^n` between
/// transient failures. Returns the last error otherwise.
pub fn with_retry<T>(
    attempts: u32,
    base: Duration,
    mut attempt: impl FnMut(u32) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let mut n = 0;
    loop {
        match attempt(n) {
            Err(e) if is_transient(&e) && n + 1 < attempts => {
                std::thread::sleep(base * 2u32.pow(n));
                n += 1;
            }
            other => return other,
        }
    }
}

/// Streams a file to a signed upload URL (`PUT`, raw body), retrying
/// transient failures with exponential backoff. `x-upsert: false` stays, so
/// a retry never overwrites an object another upload created.
pub fn upload_file(
    cfg: &CloudConfig,
    signed_url: &str,
    path: &Path,
    progress_id: &str,
) -> Result<(), AppError> {
    upload_file_with(
        cfg,
        signed_url,
        path,
        progress_id,
        UPLOAD_ATTEMPTS,
        Duration::from_secs(1),
    )
}

pub(crate) fn upload_file_with(
    cfg: &CloudConfig,
    signed_url: &str,
    path: &Path,
    progress_id: &str,
    attempts: u32,
    base: Duration,
) -> Result<(), AppError> {
    with_retry(attempts, base, |_| {
        upload_once(cfg, signed_url, path, progress_id)
    })
}

fn upload_once(
    cfg: &CloudConfig,
    signed_url: &str,
    path: &Path,
    progress_id: &str,
) -> Result<(), AppError> {
    let file = fs::File::open(path).map_err(|e| err("IO_ERROR", e))?;
    let size = file.metadata().map_err(|e| err("IO_ERROR", e))?.len();
    set_progress(progress_id, 0, size, "upload");
    let body = Body::sized(
        Counting {
            inner: file,
            id: progress_id.into(),
            done: 0,
            total: size,
        },
        size,
    );
    let response = transfer()
        .put(rebase(&cfg.url, signed_url))
        .header("apikey", &cfg.anon_key)
        .header("content-type", "application/octet-stream")
        .header("x-upsert", "false")
        .body(body)
        .send()
        .map_err(net_err)?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let text = response.text().unwrap_or_default();
        return Err(error_from(status, &text));
    }
    set_progress(progress_id, size, size, "uploaded");
    Ok(())
}

/// A downloaded file in a private scratch location, removed on drop unless kept.
pub struct Download {
    pub path: PathBuf,
    pub sha256: String,
    pub size: u64,
    keep: bool,
}
impl Download {
    pub fn keep(mut self) -> PathBuf {
        self.keep = true;
        self.path.clone()
    }
}
impl Drop for Download {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Streams `url` into `dir`, hashing as it goes; fails (TOO_LARGE) past `max`.
pub fn download(
    cfg: &CloudConfig,
    url: &str,
    dir: &Path,
    max: u64,
    progress_id: &str,
) -> Result<Download, AppError> {
    crate::paths::ensure_private_dir(dir).map_err(|e| err("IO_ERROR", e))?;
    let mut response = transfer()
        .get(rebase(&cfg.url, url))
        .header("apikey", &cfg.anon_key)
        .send()
        .map_err(net_err)?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let text = response.text().unwrap_or_default();
        return Err(error_from(status, &text));
    }
    let total = response.content_length().unwrap_or(0);
    if total > max {
        return Err(too_large(total, max));
    }
    let mut out = Download {
        path: dir.join(format!(".download-{}.ixt", uuid::Uuid::new_v4())),
        sha256: String::new(),
        size: 0,
        keep: false,
    };
    let mut file = private_file(&out.path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = response
            .read(&mut buf)
            .map_err(|e| err("CLOUD_OFFLINE", format!("Download interrupted: {e}")))?;
        if n == 0 {
            break;
        }
        out.size += n as u64;
        if out.size > max {
            return Err(too_large(out.size, max));
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| err("IO_ERROR", e))?;
        set_progress(progress_id, out.size, total, "download");
    }
    file.sync_all().map_err(|e| err("IO_ERROR", e))?;
    out.sha256 = format!("{:x}", hasher.finalize());
    set_progress(progress_id, out.size, out.size, "downloaded");
    Ok(out)
}

fn too_large(size: u64, max: u64) -> AppError {
    err(
        "TOO_LARGE",
        format!(
            "The archive is larger than {} MB ({} MB so far); it was not used",
            max / (1024 * 1024),
            size / (1024 * 1024)
        ),
    )
}

pub(crate) fn private_file(path: &Path) -> Result<fs::File, AppError> {
    let mut o = fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path).map_err(|e| err("IO_ERROR", e))
}

/// SHA-256 (hex) and size of a file, streamed.
pub fn hash_file(path: &Path) -> Result<(String, u64), AppError> {
    let mut file = fs::File::open(path).map_err(|e| err("IO_ERROR", e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut size = 0u64;
    loop {
        let n = file.read(&mut buf).map_err(|e| err("IO_ERROR", e))?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buf[..n]);
    }
    Ok((format!("{:x}", hasher.finalize()), size))
}
