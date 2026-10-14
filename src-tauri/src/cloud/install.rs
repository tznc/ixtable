//! Cloud installations (PRD §21.2, §22.3).
//!
//! A cloud app installs through the same flow as a manual runtime bundle
//! (`installation::apply_bundle`): staging, preserved `data.db`, migrations
//! and health checks on a copy, atomic activation, and revert on failure.
//! The difference is how the archive is authenticated: a manifest signed by
//! the pinned cloud key (see `manifest`) whose checksum the downloaded bytes
//! must match. The installation is pinned to the cloud key (trust on first
//! use, as with bundles) and records the manifest next to it.
//!
//! Layout under `<state>/data/cloud-installations/<appId>/`:
//! - `installation.json`: this device's installation id for the app and the
//!   installed document id
//! - `<documentId>/`: an ordinary installation directory (`active/`,
//!   `data.db`, `previous/`, `bundle.json`) plus `cloud.json` (the verified
//!   manifest, its signature, and the licensee's email)
use super::{check_id, err, manifest::Manifest};
use crate::bundle::{self, io_err, BundleFlags, BundleHeader, SignedBundle};
use crate::installation::{self, runtime_session};
use crate::manager::{AppError, DocumentManager, SessionState};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const CLOUD_JSON: &str = "cloud.json";
const LOCAL_JSON: &str = "installation.json";

pub fn cloud_root() -> PathBuf {
    bundle::state_dir().join("cloud-installations")
}

pub fn app_root(app_id: &str) -> Result<PathBuf, AppError> {
    check_id("application", app_id)?;
    Ok(cloud_root().join(app_id))
}

/// This device's installation of a cloud app.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocalInstallation {
    pub app_id: String,
    pub installation_id: String,
    pub device_name: String,
    #[serde(default)]
    pub document_id: Option<String>,
    #[serde(default)]
    pub app_name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub version_id: Option<String>,
}

pub fn device_name() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|h| !h.trim().is_empty())
        .unwrap_or_else(|| "desktop".into());
    format!("{host} ({})", std::env::consts::OS)
}

pub fn read_local(app_id: &str) -> Result<Option<LocalInstallation>, AppError> {
    match fs::read(app_root(app_id)?.join(LOCAL_JSON)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| err("INSTALLATION_CORRUPT", e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_err(e)),
    }
}

fn write_local(local: &LocalInstallation) -> Result<(), AppError> {
    let root = app_root(&local.app_id)?;
    crate::paths::ensure_private_dir(&root).map_err(io_err)?;
    bundle::write_atomic(
        &root.join(LOCAL_JSON),
        &serde_json::to_vec_pretty(local).map_err(io_err)?,
    )
}

/// The installation id for an app on this device, created on first use.
pub fn local_installation(app_id: &str) -> Result<LocalInstallation, AppError> {
    if let Some(local) = read_local(app_id)? {
        return Ok(local);
    }
    let local = LocalInstallation {
        app_id: app_id.into(),
        installation_id: uuid::Uuid::new_v4().to_string(),
        device_name: device_name(),
        document_id: None,
        app_name: None,
        version: None,
        version_id: None,
    };
    write_local(&local)?;
    Ok(local)
}

/// What `cloud.json` records about the installed version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CloudRecord {
    pub manifest: Manifest,
    pub signature: String,
    pub email: String,
    pub public_key_fingerprint: String,
    pub recorded_at: String,
    /// The manifest exactly as signed. Older records lack it and are verified
    /// from `manifest`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_manifest: Option<serde_json::Value>,
}

/// `cloud.json` as written, unverified. Identity and role must come from
/// [`verified_record`] instead.
pub fn read_record(dir: &Path) -> Option<CloudRecord> {
    serde_json::from_slice(&fs::read(dir.join(CLOUD_JSON)).ok()?).ok()
}

fn tampered() -> AppError {
    err(
        "INSTALLATION_TAMPERED",
        "The cloud installation record does not match its ixtable Cloud signature; reinstall the application",
    )
}

/// Re-verifies a stored record's manifest signature (not its expiry: the
/// installation opens offline) and returns the manifest as signed.
pub fn verify_record(
    record: &CloudRecord,
    key: &ed25519_dalek::VerifyingKey,
) -> Result<Manifest, AppError> {
    let candidates = match &record.signed_manifest {
        Some(signed) => vec![signed.clone()],
        // Older records: the stored manifest as written, and without null fields.
        None => {
            let value = serde_json::to_value(&record.manifest).map_err(|_| tampered())?;
            let mut stripped = value.clone();
            if let Some(map) = stripped.as_object_mut() {
                map.retain(|_, v| !v.is_null());
            }
            vec![value, stripped]
        }
    };
    candidates
        .iter()
        .find_map(|c| super::manifest::verify_signature(c, &record.signature, key).ok())
        .ok_or_else(tampered)
}

/// The installation's `cloud.json` with its manifest re-verified against the
/// pinned cloud key and bound to this device's installation of the app. Fail
/// closed: a missing, unreadable, or edited record is an error.
pub fn verified_record(dir: &Path) -> Result<CloudRecord, AppError> {
    let mut record = read_record(dir).ok_or_else(|| {
        err(
            "INSTALLATION_CORRUPT",
            "The cloud installation record is missing",
        )
    })?;
    let verified = verify_record(&record, &super::config::public_key()?)?;
    let local = read_local(&verified.app_id)?.ok_or_else(tampered)?;
    if dir.parent() != Some(app_root(&verified.app_id)?.as_path())
        || local.installation_id != verified.installation_id
    {
        return Err(tampered());
    }
    record.manifest = verified;
    Ok(record)
}

/// The runtime role a verified manifest grants: None (developer access) only
/// for the signed owner without a role; otherwise the signed role, or a role
/// allowed nothing. Mirrors `runtimeRoleFor` in src/cloud/session.ts.
pub fn manifest_role(m: &Manifest) -> Option<crate::roles::Role> {
    if m.owner && m.role_id.is_none() {
        return None;
    }
    Some(crate::roles::Role {
        id: m
            .role_id
            .clone()
            .unwrap_or_else(|| crate::authz::NO_ROLE.into()),
        name: m.role_name.clone().unwrap_or_else(|| "Runtime user".into()),
        permissions: serde_json::from_value(m.role_permissions.clone()).unwrap_or_default(),
    })
}

/// Opens the installation in `dir` with the role its verified record grants.
/// Opens an installation and applies its role: the signed manifest's role
/// for a cloud installation, developer access (unset) for a local one.
pub(crate) fn open_with_role(
    mgr: &DocumentManager,
    window: &str,
    dir: &Path,
) -> Result<SessionState, AppError> {
    if !dir.starts_with(cloud_root()) {
        return installation::open_session(mgr, window, dir);
    }
    let role = manifest_role(&verified_record(dir)?.manifest);
    let state = installation::open_session(mgr, window, dir)?;
    crate::authz::set_role(window, role)?;
    Ok(state)
}

/// The installation directory of an installed cloud app, if any.
pub fn installed_dir(app_id: &str) -> Result<Option<PathBuf>, AppError> {
    let Some(doc) = read_local(app_id)?.and_then(|l| l.document_id) else {
        return Ok(None);
    };
    check_id("document", &doc)?;
    let dir = app_root(app_id)?.join(doc);
    Ok(installation::read_installed(&dir).map(|_| dir))
}

/// Closes the window's session when it runs `dir`, so its files can be swapped.
fn release(m: &DocumentManager, window: &str, dir: &Path) -> Result<bool, AppError> {
    let Ok(state) = m.state(window) else {
        return Ok(false);
    };
    match runtime_session(&state.session_id) {
        Some(rt) if rt.dir == dir => {
            m.close(window, true)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// The bundle header a verified manifest stands for.
pub fn header_for(m: &Manifest, document_id: &str, public_key_b64: &str) -> BundleHeader {
    BundleHeader {
        bundle_id: document_id.into(),
        name: m.app_name.clone(),
        version: m.version.clone(),
        release_notes: String::new(),
        app_version: bundle::APP_VERSION.into(),
        min_runtime_version: m.min_runtime_version.clone().filter(|v| !v.is_empty()),
        created_at: m.issued_at.clone(),
        signer_public_key: public_key_b64.into(),
        flags: BundleFlags { encrypted: false },
        payload_sha256: m.archive_sha256.to_ascii_lowercase(),
        payload_size: m.archive_size,
        encryption: None,
    }
}

/// Installs or updates from a verified manifest and the archive file whose
/// checksum already matched it, then opens the installation in `window`.
/// On failure the previous working version stays installed and is reopened.
pub fn install_verified(
    window: &str,
    m: &Manifest,
    signed_manifest: &serde_json::Value,
    signature: &str,
    email: &str,
    archive: &Path,
    public_key_b64: &str,
) -> Result<SessionState, AppError> {
    let document_id = crate::archive_io::read_header(archive)?
        .metadata
        .document_id;
    // The id names the installation directory: refuse traversal before any join.
    super::check_id("document", &document_id)?;
    let header = header_for(m, &document_id, public_key_b64);
    bundle::check_runtime_compat(&header, bundle::APP_VERSION)?;
    let bytes = fs::read(archive).map_err(io_err)?;
    // Defense in depth: the file must still be the bytes that were verified.
    if bundle::sha256_hex(&bytes) != header.payload_sha256 {
        return Err(err(
            "ARCHIVE_CHECKSUM",
            "The archive changed after it was verified",
        ));
    }
    let signed = SignedBundle::trusted(header);
    let root = app_root(&m.app_id)?;
    crate::paths::ensure_private_dir(&root).map_err(io_err)?;
    let dir = root.join(&document_id);
    let mgr = crate::manager()?;
    let mut local = local_installation(&m.app_id)?;
    let previous = local
        .document_id
        .clone()
        .filter(|d| *d != document_id && crate::paths::is_safe_id(d))
        .map(|d| root.join(d))
        .filter(|d| installation::read_installed(d).is_some());
    let mut reopen = release(mgr, window, previous.as_deref().unwrap_or(&dir))?;
    // A restored copy's new document id is still this app: move the installation along.
    if let Some(previous) = previous.filter(|_| !dir.exists()) {
        fs::rename(&previous, &dir).map_err(io_err)?;
        local.document_id = Some(document_id.clone());
        write_local(&local)?;
        crate::logging::info(
            "cloud",
            &format!("{}: installation follows the new document id", m.app_id),
        );
        reopen = true;
    }
    match installation::apply_bundle(&root, &signed, &bytes) {
        Ok((dir, action)) => {
            drop(bytes);
            let record = CloudRecord {
                manifest: m.clone(),
                signature: signature.into(),
                email: email.into(),
                public_key_fingerprint: bundle::fingerprint(public_key_b64),
                recorded_at: Utc::now().to_rfc3339(),
                signed_manifest: Some(signed_manifest.clone()),
            };
            bundle::write_atomic(
                &dir.join(CLOUD_JSON),
                &serde_json::to_vec_pretty(&record).map_err(io_err)?,
            )?;
            let mut local = local_installation(&m.app_id)?;
            local.document_id = Some(document_id);
            local.app_name = Some(m.app_name.clone());
            local.version = Some(m.version.clone());
            local.version_id = Some(m.version_id.clone());
            write_local(&local)?;
            crate::logging::info(
                "cloud",
                &format!("{action:?} {} {} ({})", m.app_name, m.version, m.version_id),
            );
            open_with_role(mgr, window, &dir)
        }
        Err(e) => {
            crate::logging::warn(
                "cloud",
                &format!("install {}: {}: {}", m.app_id, e.code, e.message),
            );
            if reopen && dir.join(CLOUD_JSON).exists() {
                open_with_role(mgr, window, &dir)?;
            }
            Err(e)
        }
    }
}

/// Opens the last installed version (offline or up to date).
pub fn open_installed(window: &str, app_id: &str) -> Result<SessionState, AppError> {
    let dir = installed_dir(app_id)?.ok_or_else(|| {
        err("NOT_INSTALLED", "This application is not installed on this computer yet; connect to ixtable Cloud to install it")
    })?;
    open_with_role(crate::manager()?, window, &dir)
}

/// True for a directory under the cloud root: never a manual installation,
/// even before its cloud record is written.
pub fn is_cloud_dir(dir: &Path) -> bool {
    dir.starts_with(cloud_root())
}

/// The cloud installation directory of the window's runtime session.
pub fn session_dir(window: &str) -> Result<PathBuf, AppError> {
    let state = crate::manager()?.state(window)?;
    let rt = runtime_session(&state.session_id)
        .ok_or_else(|| err("NOT_CLOUD", "No cloud application is open in this window"))?;
    if !is_cloud_dir(&rt.dir) || !rt.dir.join(CLOUD_JSON).exists() {
        return Err(err(
            "NOT_CLOUD",
            "The open application is not a cloud installation",
        ));
    }
    Ok(rt.dir)
}
