//! Typed replies of the Edge Functions the Rust side calls
//! (docs/decisions/cloud-architecture.md "Contract"). A reply without a
//! field the desktop needs fails with `CLOUD_CONTRACT` instead of an empty
//! default. The tests decode responses recorded from the local stack
//! (web/e2e/service-qa/fixtures/contract, written by specs/contract.spec.ts).
use super::err;
use crate::manager::AppError;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

/// `archive-upload-url` → {uploadId, path, signedUrl, token, expiresAt}.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadUrlReply {
    pub upload_id: String,
    pub path: String,
    #[serde(alias = "signedURL")]
    pub signed_url: String,
    #[serde(default)]
    pub token: Option<String>,
}

/// `bundle-manifest` → {manifest, signature, archiveUrl, archiveUrlExpiresAt}.
/// The manifest stays a JSON value: its signature covers the canonical JSON.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleReply {
    pub manifest: Value,
    pub signature: String,
    pub archive_url: String,
}

/// `key-grant` envelope.
#[derive(Debug, Deserialize)]
pub struct GrantEnvelope {
    pub ciphertext: String,
    pub nonce: String,
    pub aad: String,
}

/// `key-grant` → {grantId, datasourceId, dek, envelope, issuedAt, expiresAt, renewed}.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantReply {
    pub dek: String,
    pub envelope: GrantEnvelope,
    #[serde(default)]
    pub expires_at: Option<String>,
}

/// `desktop-auth-exchange` → {session:{access_token, refresh_token, expires_at, …, user}}.
/// The session goes to supabase-js unchanged (`setSession`).
#[derive(Debug, Deserialize)]
pub struct ExchangeReply {
    pub session: Value,
}

/// Decodes a function reply into `T`, or fails with `CLOUD_CONTRACT`.
pub fn decode<T: DeserializeOwned>(function: &str, reply: Value) -> Result<T, AppError> {
    serde_json::from_value(reply).map_err(|e| {
        err(
            "CLOUD_CONTRACT",
            format!("ixtable Cloud answered {function} in an unexpected shape ({e})"),
        )
    })
}

/// Checks that an exchanged session carries the tokens supabase-js needs.
pub fn session(reply: Value) -> Result<Value, AppError> {
    let ExchangeReply { session } = decode("desktop-auth-exchange", reply)?;
    for key in ["access_token", "refresh_token"] {
        if !session[key].is_string() {
            return Err(err(
                "CLOUD_CONTRACT",
                format!("ixtable Cloud answered desktop-auth-exchange without session.{key}"),
            ));
        }
    }
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::manifest::Manifest;

    /// The recorded response of a contract fixture.
    fn recorded(name: &str) -> Value {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../web/e2e/service-qa/fixtures/contract");
        let text = std::fs::read_to_string(dir.join(format!("{name}.json")))
            .unwrap_or_else(|e| panic!("fixture {name}: {e}"));
        let fixture: Value = serde_json::from_str(&text).unwrap();
        fixture["response"].clone()
    }

    #[test]
    fn upload_url_reply_decodes() {
        let reply: UploadUrlReply =
            decode("archive-upload-url", recorded("archive-upload-url")).unwrap();
        assert!(reply.signed_url.contains("/storage/v1/object/upload/sign/"));
        assert!(reply.path.ends_with(&format!("{}.ixt", reply.upload_id)));
        assert!(reply.token.is_some());
    }

    #[test]
    fn bundle_reply_and_manifest_decode() {
        let reply: BundleReply = decode("bundle-manifest", recorded("bundle-manifest")).unwrap();
        assert!(reply.archive_url.contains("/storage/v1/object/sign/"));
        let manifest: Manifest = serde_json::from_value(reply.manifest).unwrap();
        assert_eq!(manifest.format, crate::cloud::manifest::FORMAT);
        assert_eq!(manifest.role_name.as_deref(), Some("Sales"));
        assert!(manifest.role_id.is_some());
        assert_eq!(manifest.role_permissions["navigation"][0], "forms");
        assert_eq!(manifest.min_runtime_version.as_deref(), Some("0.1.0"));
        assert!(!manifest.owner);
    }

    #[test]
    fn owner_manifest_with_null_role_decodes() {
        let mut manifest = recorded("bundle-manifest")["manifest"].clone();
        for key in ["roleId", "roleName", "rolePermissions"] {
            manifest[key] = Value::Null;
        }
        manifest["owner"] = Value::Bool(true);
        let manifest: Manifest = serde_json::from_value(manifest).unwrap();
        assert!(manifest.owner);
        assert_eq!(manifest.role_id, None);
        assert_eq!(manifest.role_name, None);
        assert!(manifest.role_permissions.is_null());
    }

    #[test]
    fn grant_reply_decodes() {
        let reply: GrantReply = decode("key-grant", recorded("key-grant")).unwrap();
        assert!(!reply.dek.is_empty() && !reply.envelope.ciphertext.is_empty());
        assert!(!reply.envelope.nonce.is_empty() && !reply.envelope.aad.is_empty());
        let expires = reply.expires_at.unwrap();
        assert!(chrono::DateTime::parse_from_rfc3339(&expires).is_ok());
    }

    #[test]
    fn exchange_reply_yields_the_session() {
        let session = session(recorded("desktop-auth-exchange")).unwrap();
        assert!(session["expires_at"].is_number());
        assert!(session["user"]["id"].is_string());
        let missing = super::session(serde_json::json!({ "session": { "access_token": "a" } }));
        assert_eq!(missing.unwrap_err().code, "CLOUD_CONTRACT");
    }

    #[test]
    fn a_reply_without_a_needed_field_is_a_contract_error() {
        let mut reply = recorded("archive-upload-url");
        reply.as_object_mut().unwrap().remove("uploadId");
        let e = decode::<UploadUrlReply>("archive-upload-url", reply).unwrap_err();
        assert_eq!(e.code, "CLOUD_CONTRACT");
        assert!(e.message.contains("archive-upload-url"));
    }
}
