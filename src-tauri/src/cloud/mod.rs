//! ixtable Cloud desktop client (PRD §4.2, §21–§23).
//!
//! The control plane is Supabase: the frontend uses supabase-js for sign-in,
//! PostgREST reads, and JSON-only Edge Functions. This module does the work
//! that must not live in the webview or that handles bytes and secrets:
//!
//! - `config`: cloud URL, anon key, site URL, and the pinned Ed25519 public key.
//! - `contract`: typed Edge Function replies (`CLOUD_CONTRACT` on drift).
//! - `http`: blocking HTTPS client (rustls), Edge Function calls with stable
//!   error codes, streaming archive upload/download with pollable progress.
//! - `manifest`: personalized bundle manifests, canonical JSON, and
//!   fail-closed verification (signature, expiry, identity, checksum).
//! - `install`: cloud installations through the existing runtime installation
//!   flow (`installation::apply_bundle`: preserved data.db, migrations,
//!   health checks, atomic activation, revert).
//! - `envelope`: XChaCha20-Poly1305 credential envelopes (random DEK).
//! - `grants`: datasource credentials released by key grants, memory only.
//! - `pkce`: browser hand-off sign-in (Google/Microsoft) with PKCE S256.
//! - `publish`: publish preflight (size, validation, security summary) and
//!   the staged archive copy that is uploaded.
//! - `restore`: restore a cloud archive as a new untitled local copy.
//!
//! Tokens, DEKs, and credentials are never written to logs or archives.
pub mod commands;
pub mod config;
pub mod contract;
pub mod envelope;
pub mod grants;
pub mod http;
pub mod install;
pub mod manifest;
pub mod pkce;
pub mod publish;
pub mod restore;
pub mod runtime_commands;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod chaos_tests;

use crate::manager::AppError;
use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
    Engine,
};
use serde::{Deserialize, Serialize};

/// The cloud application a Studio document publishes to (`DocumentConfig.cloud`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CloudLink {
    pub app_id: String,
    pub org_id: String,
    /// Published version this document is based on (`expectedHeadVersionId`, PRD §22.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_version_id: Option<String>,
}

/// Largest archive ixtable Cloud stores (PRD §7.4).
pub const MAX_ARCHIVE_BYTES: u64 = crate::assets::CLOUD_LIMIT_BYTES;
/// Download cap: the archive limit plus headroom for format overhead.
pub const MAX_DOWNLOAD_BYTES: u64 = 600 * 1024 * 1024;

pub(crate) fn err(code: &str, message: impl ToString) -> AppError {
    AppError::new(code, message)
}

/// Decodes standard or URL-safe base64, padded or not (servers differ).
pub fn b64_any(value: &str) -> Result<Vec<u8>, AppError> {
    let v = value.trim();
    STANDARD
        .decode(v)
        .or_else(|_| STANDARD_NO_PAD.decode(v))
        .or_else(|_| URL_SAFE.decode(v))
        .or_else(|_| URL_SAFE_NO_PAD.decode(v))
        .map_err(|_| {
            err(
                "INVALID_ENCODING",
                "A value from ixtable Cloud is not valid base64",
            )
        })
}

/// Cloud ids name directories: only uuid-like ids are accepted.
pub fn check_id(kind: &str, id: &str) -> Result<(), AppError> {
    if crate::paths::is_safe_id(id) {
        Ok(())
    } else {
        Err(err("VALIDATION", format!("Invalid {kind} id")))
    }
}
