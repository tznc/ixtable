//! Decoders for the Edge Function replies Studio and the Runtime read
//! (docs/decisions/cloud-architecture.md "Contract"). A reply missing a field the
//! desktop needs fails with CLOUD_CONTRACT instead of surfacing later as an empty
//! default. Port of src/cloud/contract.ts.

use super::{errors::CloudError, types::*};
use serde_json::{Map, Value};

type Json = Map<String, Value>;

fn fail(name: &str, path: &str, expected: &str) -> CloudError {
    CloudError::new(
        "CLOUD_CONTRACT",
        format!(
            "ixtable Cloud answered {name} without {path} ({expected}). Update ixtable or try again later."
        ),
    )
}

fn object<'a>(name: &str, value: Option<&'a Value>, path: &str) -> Result<&'a Json, CloudError> {
    value.and_then(Value::as_object).ok_or_else(|| fail(name, path, "an object"))
}

fn string(name: &str, obj: &Json, key: &str, path: &str) -> Result<String, CloudError> {
    obj.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| fail(name, path, "a string"))
}

fn nullable_string(name: &str, obj: &Json, key: &str, path: &str) -> Result<Option<String>, CloudError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(fail(name, path, "a string or null")),
    }
}

/// An `installation_backups` row (PostgREST select or `backup-commit` reply).
pub fn backup_row(raw: &Value) -> Result<InstallationBackup, CloudError> {
    let name = "installation_backups";
    let row = object(name, Some(raw), "row")?;
    let size = row
        .get("archive_size")
        .and_then(Value::as_u64)
        .ok_or_else(|| fail(name, "archive_size", "a number"))?;
    Ok(InstallationBackup {
        id: string(name, row, "id", "id")?,
        installation_id: string(name, row, "installation_id", "installation_id")?,
        archive_sha256: string(name, row, "archive_sha256", "archive_sha256")?,
        archive_size: size,
        created_at: string(name, row, "created_at", "created_at")?,
    })
}

fn version(name: &str, value: Option<&Value>) -> Result<PublishedVersion, CloudError> {
    let row = object(name, value, "version")?;
    Ok(PublishedVersion {
        id: string(name, row, "id", "version.id")?,
        version: string(name, row, "version", "version.version")?,
        status: string(name, row, "status", "version.status")?,
    })
}

fn app(name: &str, value: Option<&Value>) -> Result<AppRef, CloudError> {
    let row = object(name, value, "app")?;
    Ok(AppRef { id: string(name, row, "id", "app.id")?, name: string(name, row, "name", "app.name")? })
}

pub fn apps_create(raw: &Value) -> Result<AppRef, CloudError> {
    let reply = object("apps-create", Some(raw), "reply")?;
    app("apps-create", reply.get("app"))
}

pub fn roles_sync(raw: &Value) -> Result<(), CloudError> {
    object("roles-sync", Some(raw), "reply").map(|_| ())
}

pub fn publish_checkpoint(raw: &Value) -> Result<PublishedVersion, CloudError> {
    let reply = object("publish-checkpoint", Some(raw), "reply")?;
    version("publish-checkpoint", reply.get("version"))
}

/// overwrite → `{version}`; fork → `{app, version}`.
pub fn versions_resolve(raw: &Value) -> Result<(Option<PublishedVersion>, Option<AppRef>), CloudError> {
    let name = "versions-resolve";
    let reply = object(name, Some(raw), "reply")?;
    let present = |key: &str| reply.get(key).filter(|v| !v.is_null());
    let version = present("version").map(|v| version(name, Some(v))).transpose()?;
    let app = present("app").map(|v| app(name, Some(v))).transpose()?;
    Ok((version, app))
}

pub fn restore_url(raw: &Value) -> Result<RestoreTarget, CloudError> {
    let name = "restore-url";
    let reply = object(name, Some(raw), "reply")?;
    let size = reply.get("size").and_then(Value::as_u64).ok_or_else(|| fail(name, "size", "a number"))?;
    let is_postgres = reply
        .get("isPostgres")
        .and_then(Value::as_bool)
        .ok_or_else(|| fail(name, "isPostgres", "a boolean"))?;
    Ok(RestoreTarget {
        signed_url: string(name, reply, "signedUrl", "signedUrl")?,
        sha256: string(name, reply, "sha256", "sha256")?,
        size,
        is_postgres,
        warning: nullable_string(name, reply, "warning", "warning")?,
    })
}

pub fn sync_check(raw: &Value) -> Result<SyncCheck, CloudError> {
    let name = "sync-check";
    let reply = object(name, Some(raw), "reply")?;
    let up_to_date = reply
        .get("upToDate")
        .and_then(Value::as_bool)
        .ok_or_else(|| fail(name, "upToDate", "a boolean"))?;
    let latest = match reply.get("latest").filter(|v| !v.is_null()) {
        None => None,
        Some(latest) => {
            let latest = object(name, Some(latest), "latest")?;
            Some(LatestVersion {
                version_id: string(name, latest, "versionId", "latest.versionId")?,
                version: string(name, latest, "version", "latest.version")?,
                min_runtime_version: nullable_string(
                    name,
                    latest,
                    "minRuntimeVersion",
                    "latest.minRuntimeVersion",
                )?,
            })
        }
    };
    Ok(SyncCheck { up_to_date, latest })
}

/// The id of the committed backup.
pub fn backup_commit(raw: &Value) -> Result<String, CloudError> {
    let name = "backup-commit";
    let reply = object(name, Some(raw), "reply")?;
    let backup = object(name, reply.get("backup"), "backup")?;
    string(name, backup, "id", "backup.id")
}

pub fn invitations_accept(raw: &Value) -> Result<Membership, CloudError> {
    let name = "invitations-accept";
    let reply = object(name, Some(raw), "reply")?;
    let membership = object(name, reply.get("membership"), "membership")?;
    Ok(Membership {
        kind: string(name, membership, "kind", "membership.kind")?,
        app_id: nullable_string(name, membership, "appId", "membership.appId")?,
        org_id: string(name, membership, "orgId", "membership.orgId")?,
    })
}

/// The functions the desktop decodes (a recorded fixture exists for each).
pub const DECODED_FUNCTIONS: [&str; 8] = [
    "apps-create",
    "roles-sync",
    "publish-checkpoint",
    "versions-resolve",
    "restore-url",
    "sync-check",
    "backup-commit",
    "invitations-accept",
];

/// The raw token of an invitation, from the emailed link (`…/invite?token=…`, or the
/// older `/invitations/accept?token=…`) or the token pasted on its own.
pub fn invitation_token(input: &str) -> String {
    let text = input.trim();
    if text.is_empty() {
        return String::new();
    }
    let token_of = |url: &reqwest::Url| {
        url.query_pairs()
            .find(|(k, _)| k == "token")
            .map(|(_, v)| v.trim().to_string())
            .unwrap_or_default()
    };
    let has_scheme = text.split_once("://").is_some_and(|(scheme, _)| {
        scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
    });
    if has_scheme {
        return reqwest::Url::parse(text).map(|u| token_of(&u)).unwrap_or_default();
    }
    if text.contains("token=") {
        let query = text.split_once('?').map_or(text, |(_, q)| q);
        return reqwest::Url::parse(&format!("http://x/?{query}"))
            .map(|u| token_of(&u))
            .unwrap_or_default();
    }
    let bare = (16..=256).contains(&text.len())
        && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare { text.to_string() } else { String::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A recorded fixture: function, status, request, response.
    fn fixture(name: &str) -> Value {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../web/e2e/service-qa/fixtures/contract");
        let text = std::fs::read_to_string(dir.join(format!("{name}.json")))
            .unwrap_or_else(|e| panic!("fixture {name}: {e}"));
        serde_json::from_str(&text).unwrap()
    }

    fn response(name: &str) -> Value {
        fixture(name)["response"].clone()
    }

    #[test]
    fn has_a_fixture_for_every_function_the_desktop_decodes() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../web/e2e/service-qa/fixtures/contract");
        let recorded: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|f| f.file_name().to_string_lossy().split('.').next().unwrap().to_string())
            .collect();
        for name in DECODED_FUNCTIONS {
            assert!(recorded.iter().any(|r| r == name), "{name}");
        }
    }

    #[test]
    fn apps_create_publish_checkpoint_and_versions_resolve_return_version_and_app_rows() {
        let created = apps_create(&response("apps-create")).unwrap();
        assert_eq!(created.name, "Contract CRM");
        assert!(!created.id.is_empty());

        let published = publish_checkpoint(&response("publish-checkpoint")).unwrap();
        assert_eq!((published.version.as_str(), published.status.as_str()), ("1.0.0", "published"));

        let (version, app) = versions_resolve(&response("versions-resolve.overwrite")).unwrap();
        assert!(app.is_none());
        let version = version.unwrap();
        assert_eq!((version.version.as_str(), version.status.as_str()), ("1.1.0", "published"));

        let (version, app) = versions_resolve(&response("versions-resolve.fork")).unwrap();
        assert_eq!(app.unwrap().name, "Contract CRM (fork)");
        assert!(!version.unwrap().id.is_empty());
    }

    #[test]
    fn restore_url_sync_check_and_backup_commit_carry_what_restore_update_and_backup_read() {
        for name in ["restore-url.version", "restore-url.backup"] {
            let target = restore_url(&response(name)).unwrap();
            assert!(target.signed_url.contains("/storage/v1/object/sign/"));
            assert!(target.sha256.len() == 64 && target.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(!target.is_postgres);
            assert_eq!(target.warning, None);
        }
        let sync = sync_check(&response("sync-check")).unwrap();
        assert!(sync.up_to_date);
        let latest = sync.latest.unwrap();
        assert_eq!(latest.version, "1.0.0");
        assert_eq!(latest.min_runtime_version.as_deref(), Some("0.1.0"));
        assert!(!latest.version_id.is_empty());
        assert!(sync_check(&json!({ "upToDate": false, "latest": null })).unwrap().latest.is_none());
        assert!(!backup_commit(&response("backup-commit")).unwrap().is_empty());
    }

    #[test]
    fn a_reply_missing_a_needed_field_fails_with_cloud_contract() {
        let mut reply = response("publish-checkpoint");
        reply["version"].as_object_mut().unwrap().remove("id");
        assert_eq!(publish_checkpoint(&reply).unwrap_err().code, "CLOUD_CONTRACT");
        let error = sync_check(&json!({ "upToDate": "yes" })).unwrap_err();
        assert_eq!(error.code, "CLOUD_CONTRACT");
        assert!(error.message.contains("upToDate"));
    }

    #[test]
    fn recorded_error_bodies_keep_their_code_and_the_details_the_ui_reads() {
        let conflict = fixture("publish-checkpoint.409");
        let e409 = super::super::errors::from_body(conflict["status"].as_u64().unwrap() as u16, &conflict["response"]);
        assert_eq!(e409.code, "VERSION_CONFLICT");
        assert!(e409.detail("headVersionId").is_some());

        let pending = fixture("desktop-auth-exchange.428");
        let e428 = super::super::errors::from_body(pending["status"].as_u64().unwrap() as u16, &pending["response"]);
        assert_eq!(e428.code, "PENDING");

        let withdraw = fixture("versions-resolve.withdraw.422");
        let e422 = super::super::errors::from_body(withdraw["status"].as_u64().unwrap() as u16, &withdraw["response"]);
        assert_eq!(e422.details, Some(json!({ "requiresConfirm": true, "installations": 1 })));
    }

    #[test]
    fn an_installation_backups_row_decodes_from_the_backup_commit_reply() {
        let backup = backup_row(&response("backup-commit")["backup"]).unwrap();
        assert_eq!(backup.id, "4d0213df-4045-487f-b0f5-9dd9f4e80c3a");
        assert_eq!(backup.installation_id, "6c0015d3-6817-4894-aac5-1a10e44f7b30");
        assert_eq!(backup.archive_sha256.len(), 64);
        assert_eq!(backup.archive_size, 2048);
        assert!(!backup.created_at.is_empty());
        let error = backup_row(&json!({ "id": "x" })).unwrap_err();
        assert!(error.message.contains("answered installation_backups without"));
    }

    #[test]
    fn invitations_accept_decodes_the_membership() {
        let membership = invitations_accept(&response("invitations-accept")).unwrap();
        assert_eq!(membership.kind, "app");
        assert_eq!(membership.app_id.as_deref(), Some("d5c5216b-2a16-48b4-86c4-488a9074a42d"));
        assert_eq!(membership.org_id, "6baa48c5-3687-468d-952c-34e41d117128");
    }

    #[test]
    fn an_invitation_token_comes_from_the_emailed_link_or_the_bare_token() {
        let token = "Zm9vYmFyYmF6cXV4cXV1eGNvcmdlZ3JhdWx0";
        assert_eq!(invitation_token(&format!("https://ixtable.app/invite?token={token}")), token);
        assert_eq!(
            invitation_token(&format!("https://ixtable.app/invitations/accept?token={token}&x=1")),
            token
        );
        assert_eq!(invitation_token(&format!("/invite?token={token}")), token);
        assert_eq!(invitation_token(&format!("  {token}  ")), token);
        assert_eq!(invitation_token("https://ixtable.app/invite"), "");
        assert_eq!(invitation_token("not a token"), "");
    }
}
