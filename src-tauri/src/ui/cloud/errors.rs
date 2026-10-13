//! Cloud errors with stable codes (from the server, the client, or the Rust backend)
//! and the plain-language summaries shown with them. Port of src/cloud/errors.ts and
//! CloudErrorNotice.tsx.
//!
//! Test id of the notice: `<id>` (label "CODE: summary detail").

use crate::{manager::AppError, ui::prelude::*};
use serde_json::Value;

/// A failed cloud operation. `status` and `details` come from the server's reply.
#[derive(Debug, Clone, PartialEq)]
pub struct CloudError {
    pub code: String,
    pub message: String,
    pub status: Option<u16>,
    pub details: Option<Value>,
}

impl CloudError {
    pub fn new(code: &str, message: impl ToString) -> Self {
        Self { code: code.into(), message: message.to_string(), status: None, details: None }
    }

    /// A detail the server attached (`details.headVersionId`).
    pub fn detail(&self, key: &str) -> Option<&str> {
        self.details.as_ref()?.get(key)?.as_str()
    }
}

impl std::fmt::Display for CloudError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// Rust backend commands already carry a code and a message.
impl From<AppError> for CloudError {
    fn from(e: AppError) -> Self {
        Self { code: e.code, message: e.message, status: None, details: None }
    }
}

impl From<CloudError> for AppError {
    fn from(e: CloudError) -> Self {
        AppError { code: e.code, message: e.message }
    }
}

/// Stable code for an HTTP status without a structured body.
pub fn code_for_status(status: u16) -> &'static str {
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
        500.. => "CLOUD_UNAVAILABLE",
        _ => "CLOUD_ERROR",
    }
}

fn is_code(value: &str) -> bool {
    let bytes = value.as_bytes();
    (2..=64).contains(&bytes.len())
        && bytes[0].is_ascii_uppercase()
        && bytes.iter().all(|b| b.is_ascii_uppercase() || *b == b'_')
}

/// Maps a response body (`{error:{code,message}}` or anything else) and status.
pub fn from_body(status: u16, body: &Value) -> CloudError {
    let inner = match body.get("error") {
        Some(e) if e.is_object() => e,
        _ => body,
    };
    let code = inner
        .get("code")
        .and_then(Value::as_str)
        .filter(|c| is_code(c))
        .unwrap_or_else(|| code_for_status(status));
    let message = inner
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| body.get("msg").and_then(Value::as_str))
        .or_else(|| body.get("error").and_then(Value::as_str))
        .map_or_else(|| format!("ixtable Cloud answered with HTTP {status}"), str::to_string);
    let details = inner.get("details").filter(|d| d.is_object()).cloned();
    CloudError { code: code.into(), message, status: Some(status), details }
}

const OFFLINE: &str = "ixtable Cloud is not reachable. Check your connection.";

/// What a cloud call can fail with, before it becomes a `CloudError`.
#[derive(Debug, Clone)]
pub enum Failure {
    /// An Edge Function answered with an error status (body when it was JSON).
    Http { status: u16, body: Option<Value> },
    /// The network or the request failed.
    Network,
    /// The function relay failed.
    Relay(String),
    /// A sign-in service error.
    Auth { status: Option<u16>, message: String, retryable: bool },
    /// A PostgREST error (`code`: `42501`, `PGRST301`, ...).
    Postgrest { code: String, message: String },
    /// A Rust command error.
    Command(AppError),
    Other(String),
}

/// Normalizes anything a cloud call can throw.
pub fn to_cloud_error(failure: Failure) -> CloudError {
    match failure {
        Failure::Http { status, body } => from_body(status, &body.unwrap_or(Value::Null)),
        Failure::Network => CloudError::new("CLOUD_OFFLINE", OFFLINE),
        Failure::Relay(message) => CloudError::new("CLOUD_UNAVAILABLE", message),
        Failure::Auth { retryable: true, .. } => CloudError::new("CLOUD_OFFLINE", OFFLINE),
        Failure::Auth { status, message, .. } => CloudError {
            code: if status == Some(429) { "RATE_LIMITED" } else { "UNAUTHENTICATED" }.into(),
            message: if message.is_empty() { "Sign-in failed".into() } else { message },
            status,
            details: None,
        },
        Failure::Postgrest { code, message } => {
            let mapped = if code == "42501" || code == "PGRST301" { "FORBIDDEN" } else { "CLOUD_ERROR" };
            CloudError::new(mapped, if message.is_empty() { "The cloud query failed" } else { &message })
        }
        Failure::Command(e) => e.into(),
        Failure::Other(message) => CloudError::new("CLOUD_ERROR", message),
    }
}

/// Plain-language summary for a cloud error code.
pub fn cloud_error_summary(code: &str) -> &'static str {
    match code {
        "UNAUTHENTICATED" => "Your cloud session ended. Sign in again.",
        "FORBIDDEN" => "Your account is not allowed to do this.",
        "REVOKED" => "Your access to this application was revoked by its developer.",
        "NOT_FOUND" => "The cloud application or version was not found.",
        "VERSION_CONFLICT" => "Someone published a newer checkpoint since this document last synced.",
        "TOO_LARGE" => "The archive is larger than the 500 MB cloud limit.",
        "ENTITLEMENT_REQUIRED" => {
            "This application's plan does not allow this. The developer must upgrade."
        }
        "RATE_LIMITED" => "Too many requests. Wait a moment and try again.",
        "VALIDATION" => "ixtable Cloud rejected the request.",
        "CLOUD_OFFLINE" => "ixtable Cloud is not reachable. Check your connection.",
        "CLOUD_UNAVAILABLE" => "ixtable Cloud is temporarily unavailable.",
        "CLOUD_NOT_CONFIGURED" => "ixtable Cloud is not configured in this build.",
        "CLOUD_CONTRACT" => {
            "ixtable Cloud sent a reply this version of ixtable does not understand."
        }
        "MANIFEST_SIGNATURE" => {
            "The application bundle is not signed by ixtable Cloud. It was not installed."
        }
        "MANIFEST_EXPIRED" => "The download authorization expired. Sync again.",
        "MANIFEST_MISMATCH" => "The bundle was issued for someone else. It was not installed.",
        "ARCHIVE_CHECKSUM" => "The download was corrupted or modified. It was discarded.",
        "CLOUD_KEY_MISSING" => "This build cannot verify cloud bundles (no signing key).",
        "SAVE_REQUIRED" => "Save the document first.",
        "EXPRESSION_ERRORS" => "Some expressions have errors. Fix them in Settings › Problems first.",
        "NOT_INSTALLED" => "Connect to ixtable Cloud once to install this application.",
        "UPDATE_FAILED" => {
            "The update failed and was rolled back. The previous version is still in use."
        }
        _ => "The cloud operation failed.",
    }
}

/// An alert for a failed cloud operation: plain-language summary plus the server detail.
pub fn cloud_error_notice(id: impl Into<ElementId>, error: &CloudError, cx: &App) -> Div {
    let summary = cloud_error_summary(&error.code);
    let detail = if error.message.is_empty() || error.message == summary {
        String::new()
    } else {
        format!(" {}", error.message)
    };
    let theme = cx.theme();
    div()
        .w_full()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(theme.danger)
        .text_color(theme.danger)
        .child(widgets::text(id, format!("{}: {summary}{detail}", error.code)))
}

#[cfg(test)]
mod tests {
    use super::{cloud_error_summary, code_for_status, from_body, to_cloud_error, Failure};
    use crate::manager::AppError;
    use serde_json::{json, Value};

    #[test]
    fn maps_the_contracts_error_bodies() {
        let e = from_body(
            409,
            &json!({ "error": { "code": "VERSION_CONFLICT", "message": "Head moved",
                                "details": { "headVersionId": "v2" } } }),
        );
        assert_eq!((e.code.as_str(), e.message.as_str(), e.status), ("VERSION_CONFLICT", "Head moved", Some(409)));
        assert_eq!(e.details, Some(json!({ "headVersionId": "v2" })));
        assert_eq!(e.detail("headVersionId"), Some("v2"));
        let revoked = from_body(403, &json!({ "error": { "code": "REVOKED", "message": "Access revoked" } }));
        assert_eq!(revoked.code, "REVOKED");
    }

    #[test]
    fn falls_back_to_the_http_status_for_unknown_or_hostile_bodies() {
        assert_eq!(from_body(401, &Value::Null).code, "UNAUTHENTICATED");
        assert_eq!(from_body(402, &json!({ "error": { "code": "drop table; --" } })).code, "ENTITLEMENT_REQUIRED");
        assert_eq!(from_body(413, &json!("<html>")).code, "TOO_LARGE");
        assert_eq!(from_body(503, &json!({})).code, "CLOUD_UNAVAILABLE");
        assert_eq!(from_body(400, &json!({ "msg": "bad" })).message, "bad");
        assert_eq!(from_body(400, &json!({ "error": "plain" })).message, "plain");
        let codes: Vec<&str> =
            [401, 402, 403, 404, 409, 413, 422, 428, 429, 418].map(code_for_status).to_vec();
        assert_eq!(
            codes,
            [
                "UNAUTHENTICATED",
                "ENTITLEMENT_REQUIRED",
                "FORBIDDEN",
                "NOT_FOUND",
                "VERSION_CONFLICT",
                "TOO_LARGE",
                "VALIDATION",
                "PENDING",
                "RATE_LIMITED",
                "CLOUD_ERROR",
            ]
        );
    }

    #[test]
    fn normalizes_functions_auth_postgrest_rust_and_network_errors() {
        let http = to_cloud_error(Failure::Http {
            status: 429,
            body: Some(json!({ "error": { "code": "RATE_LIMITED", "message": "Slow down" } })),
        });
        assert_eq!((http.code.as_str(), http.message.as_str()), ("RATE_LIMITED", "Slow down"));
        assert_eq!(to_cloud_error(Failure::Http { status: 502, body: None }).code, "CLOUD_UNAVAILABLE");
        assert_eq!(to_cloud_error(Failure::Network).code, "CLOUD_OFFLINE");
        assert_eq!(to_cloud_error(Failure::Relay("relay".into())).code, "CLOUD_UNAVAILABLE");
        let auth = to_cloud_error(Failure::Auth {
            status: Some(400),
            message: "Invalid login credentials".into(),
            retryable: false,
        });
        assert_eq!((auth.code.as_str(), auth.message.as_str()), ("UNAUTHENTICATED", "Invalid login credentials"));
        let retry = Failure::Auth { status: None, message: "x".into(), retryable: true };
        assert_eq!(to_cloud_error(retry).code, "CLOUD_OFFLINE");
        let pg = |code: &str, message: &str| {
            to_cloud_error(Failure::Postgrest { code: code.into(), message: message.into() }).code
        };
        assert_eq!(pg("42501", "permission denied"), "FORBIDDEN");
        assert_eq!(pg("PGRST116", "no rows"), "CLOUD_ERROR");
        let command = to_cloud_error(Failure::Command(AppError::new("MANIFEST_SIGNATURE", "bad sig")));
        assert_eq!((command.code.as_str(), command.message.as_str()), ("MANIFEST_SIGNATURE", "bad sig"));
        assert_eq!(to_cloud_error(Failure::Other("boom".into())).message, "boom");
    }

    #[test]
    fn explains_codes_in_plain_language() {
        assert!(cloud_error_summary("REVOKED").contains("revoked"));
        assert!(cloud_error_summary("ARCHIVE_CHECKSUM").contains("discarded"));
        assert_eq!(cloud_error_summary("SOMETHING_NEW"), "The cloud operation failed.");
    }
}
