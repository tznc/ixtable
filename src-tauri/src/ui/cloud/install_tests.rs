//! Cloud installs through the live client against a mock ixtable Cloud (port of
//! tests/integration/cloud-install.test.tsx): signed manifests, tampering, updates that
//! keep runtime records, PostgreSQL credential delivery, and publish/restore transfers.
//! The environment points the client at the mock, so these tests run one at a time.

use super::{
    client::LiveServices,
    mockhttp::{Request, Response, Server, lock_env},
    services::CloudServices,
};
use crate::{
    cloud::manifest::canonical_json,
    data::{DataValue, NamedValue},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use rand_core::OsRng;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex, MutexGuard};

const USER_ID: &str = "0190c3f4-bbbb-7bbb-8ccc-0123456789ab";
const ROLE_ID: &str = "0190c3f4-cccc-7bbb-8ccc-0123456789ab";

struct Cloud {
    app_id: String,
    archive: Vec<u8>,
    version: String,
    version_id: String,
    manifest_patch: Option<Value>,
    owner: bool,
    signer: SigningKey,
    served_bytes: Option<Vec<u8>>,
    grant: Option<Value>,
    grant_error: Option<(u16, String)>,
    envelope: Option<Value>,
    requests: Vec<(String, Value, String)>,
    uploaded: Option<(String, Vec<u8>)>,
}

struct World {
    _env: MutexGuard<'static, ()>,
    _server: Server,
    cloud: Arc<Mutex<Cloud>>,
    window: String,
    app_id: String,
    key: SigningKey,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn manifest_for(cloud: &Cloud, body: &Value) -> Value {
    let now = chrono::Utc::now();
    let mut manifest = json!({
        "format": crate::cloud::manifest::FORMAT,
        "appId": cloud.app_id,
        "appName": "Field Notes",
        "versionId": cloud.version_id,
        "version": cloud.version,
        "archiveSha256": sha256_hex(&cloud.archive),
        "archiveSize": cloud.archive.len(),
        "minRuntimeVersion": "0.0.0",
        "userId": USER_ID,
        "roleId": ROLE_ID,
        "roleName": "Field worker",
        "rolePermissions": {
            "navigation": [],
            "objects": [
                { "kind": "table", "id": "notes", "read": true, "create": true },
                { "kind": "table", "id": "tags", "read": true },
            ],
            "actions": [],
        },
        "installationId": body["installationId"],
        "fingerprint": "a".repeat(64),
        "issuedAt": now.to_rfc3339(),
        "expiresAt": (now + chrono::Duration::hours(1)).to_rfc3339(),
    });
    if cloud.owner {
        manifest["owner"] = json!(true);
        manifest["roleId"] = Value::Null;
    }
    let signed = canonical_json(&manifest);
    let signature = STANDARD.encode(cloud.signer.sign(signed.as_bytes()).to_bytes());
    let served = match &cloud.manifest_patch {
        Some(patch) => {
            let mut patched = manifest.clone();
            for (k, v) in patch.as_object().unwrap() {
                patched[k] = v.clone();
            }
            canonical_json(&patched)
        }
        None => signed,
    };
    json!({ "manifest": served, "signature": signature, "archiveUrl": "/object/sign/app-archives/x.ixt?token=t" })
}

fn handle(cloud: &Mutex<Cloud>, request: Request) -> Response {
    let mut cloud = cloud.lock().unwrap();
    if request.method == "PUT" && request.path.starts_with("/storage/v1/object/upload/sign/") {
        cloud.uploaded = Some((request.path.clone(), request.body));
        return Response::json(200, &json!({ "Key": "app-archives/p.ixt" }));
    }
    if request.path.starts_with("/storage/v1/object/sign/") {
        let bytes = cloud.served_bytes.clone().unwrap_or_else(|| cloud.archive.clone());
        return Response::bytes(bytes);
    }
    let name = request.path.trim_start_matches("/functions/v1/").to_string();
    let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
    let auth = request.headers.get("authorization").cloned().unwrap_or_default();
    cloud.requests.push((name.clone(), body.clone(), auth));
    match name.as_str() {
        "bundle-manifest" => Response::json(200, &manifest_for(&cloud, &body)),
        "archive-upload-url" => Response::json(
            200,
            &json!({
                "uploadId": "up-1",
                "path": "apps/x/versions/p.ixt",
                "signedUrl": "http://kong:8000/storage/v1/object/upload/sign/app-archives/p.ixt",
                "token": "tok",
            }),
        ),
        "credential-envelope" => {
            cloud.envelope = Some(body);
            Response::json(200, &json!({ "envelopeId": "env-1" }))
        }
        "key-grant" => match cloud.grant_error.clone() {
            Some((status, code)) => {
                Response::json(status, &json!({ "error": { "code": code, "message": "Access revoked" } }))
            }
            None => Response::json(200, &cloud.grant.clone().unwrap_or(Value::Null)),
        },
        other => Response::json(404, &json!({ "error": { "code": "NOT_FOUND", "message": other } })),
    }
}

fn world(window: &str) -> World {
    // Each test installs its own application: the cloud signing key differs per test.
    let app_id = format!("0190c3f4-{}-7bbb-8ccc-0123456789ab", &uuid::Uuid::new_v4().simple().to_string()[..4]);
    let env = lock_env();
    let key = SigningKey::generate(&mut OsRng);
    let cloud = Arc::new(Mutex::new(Cloud {
        app_id: app_id.clone(),
        archive: vec![],
        version: "1.0.0".into(),
        version_id: "v1".into(),
        manifest_patch: None,
        owner: false,
        signer: key.clone(),
        served_bytes: None,
        grant: None,
        grant_error: None,
        envelope: None,
        requests: vec![],
        uploaded: None,
    }));
    let shared = cloud.clone();
    let server = Server::start(move |request| handle(&shared, request));
    // Serialized by ENV: no other test reads these while this one holds it.
    std::env::set_var("IXTABLE_CLOUD_URL", &server.url);
    std::env::set_var("IXTABLE_CLOUD_ANON_KEY", "test-anon");
    std::env::set_var("IXTABLE_CLOUD_PUBLIC_KEY_RAW", STANDARD.encode(key.verifying_key().as_bytes()));
    World { _env: env, _server: server, cloud, window: window.into(), app_id, key }
}

fn text_value(column: &str, text: &str) -> NamedValue {
    NamedValue { column: column.into(), value: DataValue::Text(text.into()) }
}

/// A saved archive of a one-table document (and a second table through a migration).
fn archive(window: &str, name: &str, extra: bool, datasource: Option<Value>) -> Vec<u8> {
    crate::new_document(window.into()).unwrap();
    let table = |table_name: &str| {
        let spec = json!({
            "name": table_name,
            "columns": [
                { "name": "id", "declaredType": "INTEGER", "primaryKeyPosition": 1, "nullable": true },
                { "name": "body", "declaredType": "TEXT", "primaryKeyPosition": 0, "nullable": true },
            ],
            "foreignKeys": [], "checks": [], "withoutRowid": false,
        });
        crate::create_database_table(window.into(), serde_json::from_value(spec).unwrap()).unwrap();
    };
    table("notes");
    let mut config = serde_json::to_value(crate::read_document_config(window.into()).unwrap()).unwrap();
    config["name"] = json!("Field Notes");
    config["entities"] = json!([{ "id": "e1", "table": "notes", "concurrency": "optimistic" }]);
    if let Some(datasource) = datasource {
        config["datasource"] = datasource;
    }
    config["migrations"] = if extra {
        json!([{
            "id": "0190c3f4-eeee-7bbb-8ccc-0123456789ab",
            "name": "Add tags",
            "order": 1,
            "targetStore": "sqlite",
            "up": "CREATE TABLE tags (id INTEGER PRIMARY KEY, label TEXT)",
        }])
    } else {
        json!([])
    };
    crate::update_document_config(window.into(), serde_json::from_value(config).unwrap()).unwrap();
    if extra {
        crate::migrations::commands::apply_migrations(window.into()).unwrap();
    }
    let path = crate::paths::state_dir().join(name);
    crate::save_document_as(window.into(), path.to_string_lossy().into()).unwrap();
    crate::close_document(window.into(), true).unwrap();
    std::fs::read(path).unwrap()
}

impl World {
    fn install(&self) -> Result<crate::manager::SessionState, super::errors::CloudError> {
        LiveServices.install_app(&self.window, "user-jwt", &self.app_id, USER_ID, "ada@example.test", "t1")
    }

    fn notes(&self) -> Vec<String> {
        let page = crate::read_table_page(self.window.clone(), "notes".into(), 0, 100, vec![], vec![], None)
            .unwrap();
        page.rows.iter().map(|r| crate::ui::testing::shown(&r[1])).collect()
    }
}

#[test]
fn installs_a_signed_cloud_bundle_rejects_tampering_and_updates_keeping_records() {
    let w = world("cloud-install-main");
    let v1 = archive("cloud-install-seed-1", "field-v1.ixt", false, None);
    let v2 = archive("cloud-install-seed-2", "field-v2.ixt", true, None);
    w.cloud.lock().unwrap().archive = v1.clone();

    let state = w.install().unwrap();
    assert!(state.runtime_only);
    {
        let cloud = w.cloud.lock().unwrap();
        let (name, _, auth) = cloud.requests.last().unwrap();
        assert_eq!((name.as_str(), auth.as_str()), ("bundle-manifest", "Bearer user-jwt"));
    }
    let info = LiveServices.runtime_info(&w.window).unwrap();
    assert_eq!(
        (info.app_id.as_str(), info.version.as_str(), info.role_id.as_deref(), info.role_name.as_deref()),
        (w.app_id.as_str(), "1.0.0", Some(ROLE_ID), Some("Field worker"))
    );
    assert_eq!((info.email.as_str(), info.user_id.as_str()), ("ada@example.test", USER_ID));
    let progress = LiveServices.transfer_progress("t1").unwrap();
    assert_eq!((progress.phase.as_str(), progress.done), ("downloaded", v1.len() as u64));

    crate::recordstore::commands::insert_row(
        w.window.clone(),
        "notes".into(),
        vec![text_value("body", "written at runtime")],
        None,
    )
    .unwrap();
    // The role may not run ad hoc SQL or delete rows it was only given create on.
    assert_eq!(
        crate::execute_read_query(w.window.clone(), "SELECT 1".into()).err().map(|e| e.code),
        Some("FORBIDDEN".into())
    );
    assert_eq!(
        crate::recordstore::commands::delete_row(
            w.window.clone(),
            "notes".into(),
            vec![DataValue::Integer(1)],
            Some(vec![text_value("body", "written at runtime")]),
            None,
        )
        .err()
        .map(|e| e.code),
        Some("FORBIDDEN".into())
    );

    {
        let mut cloud = w.cloud.lock().unwrap();
        cloud.archive = v2.clone();
        cloud.version = "1.1.0".into();
        cloud.version_id = "v2".into();
        // A signed manifest changed in transit.
        cloud.manifest_patch = Some(json!({ "roleId": "0190c3f4-dddd-7bbb-8ccc-0123456789ab" }));
    }
    assert_eq!(w.install().err().map(|e| e.code), Some("MANIFEST_SIGNATURE".into()));
    {
        let mut cloud = w.cloud.lock().unwrap();
        cloud.manifest_patch = None;
        cloud.signer = SigningKey::generate(&mut OsRng);
    }
    assert_eq!(w.install().err().map(|e| e.code), Some("MANIFEST_SIGNATURE".into()));
    {
        let mut cloud = w.cloud.lock().unwrap();
        cloud.signer = w.key.clone();
        let mut flipped = v2.clone();
        *flipped.last_mut().unwrap() ^= 1;
        cloud.served_bytes = Some(flipped);
    }
    assert_eq!(w.install().err().map(|e| e.code), Some("ARCHIVE_CHECKSUM".into()));
    {
        let mut cloud = w.cloud.lock().unwrap();
        cloud.served_bytes = None;
        let past = (chrono::Utc::now() - chrono::Duration::seconds(1)).to_rfc3339();
        cloud.manifest_patch = Some(json!({ "expiresAt": past }));
    }
    let expired = w.install().err().map(|e| e.code);
    assert!(matches!(expired.as_deref(), Some("MANIFEST_SIGNATURE" | "MANIFEST_EXPIRED")), "{expired:?}");
    w.cloud.lock().unwrap().manifest_patch = None;
    assert_eq!(LiveServices.runtime_info(&w.window).unwrap().version, "1.0.0");
    assert_eq!(w.notes(), ["written at runtime"]);

    let updated = w.install().unwrap();
    assert_eq!(updated.bundle_version.as_deref(), Some("1.1.0"));
    assert_eq!(w.notes(), ["written at runtime"]);
    let mut tables: Vec<String> =
        crate::list_database_objects(w.window.clone()).unwrap().into_iter().map(|o| o.name).collect();
    tables.sort();
    assert_eq!(tables, ["notes", "tags"]);
    assert_eq!(
        crate::installation_commands::reset_runtime_installation_data(w.window.clone(), true)
            .err()
            .map(|e| e.code),
        Some("FORBIDDEN".into())
    );
    let installed = LiveServices.runtime_installation_info(&w.window).unwrap();
    assert_eq!(installed.version, "1.1.0");
    assert_eq!(installed.applied_migrations, ["Add tags"]);
    // Other tests install into the same directory: find this test's application.
    let apps: Vec<_> =
        LiveServices.installed_apps(&w.window).unwrap().into_iter().filter(|a| a.app_id == w.app_id).collect();
    assert_eq!(
        (apps[0].app_id.as_str(), apps[0].version.as_deref(), apps[0].version_id.as_deref()),
        (w.app_id.as_str(), Some("1.1.0"), Some("v2"))
    );

    // Offline, the installed version opens with its records.
    crate::close_document(w.window.clone(), true).unwrap();
    let offline = LiveServices.open_installed(&w.window, &w.app_id).unwrap();
    assert_eq!(offline.bundle_version.as_deref(), Some("1.1.0"));
    assert_eq!(w.notes(), ["written at runtime"]);
    crate::close_document(w.window.clone(), true).unwrap();

    // Granting the owner role in the stored record is caught by its signature.
    let app_dir = crate::cloud::install::cloud_root().join(&w.app_id);
    let doc_dir = std::fs::read_dir(&app_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|d| d.path())
        .find(|p| p.join("cloud.json").exists())
        .unwrap();
    let record_path = doc_dir.join("cloud.json");
    let mut record: Value = serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
    let mut signed: Value = serde_json::from_str(record["signedManifest"].as_str().unwrap()).unwrap();
    signed["owner"] = json!(true);
    signed["roleId"] = Value::Null;
    record["signedManifest"] = json!(canonical_json(&signed));
    record["manifest"]["owner"] = json!(true);
    record["manifest"]["roleId"] = Value::Null;
    std::fs::write(&record_path, record.to_string()).unwrap();
    assert_eq!(
        LiveServices.open_installed(&w.window, &w.app_id).err().map(|e| e.code),
        Some("INSTALLATION_TAMPERED".into())
    );
}

#[test]
fn keeps_the_owners_access_after_resetting_installation_data() {
    let w = world("cloud-install-owner");
    w.cloud.lock().unwrap().archive = archive("cloud-install-owner-seed", "owner.ixt", false, None);
    {
        let mut cloud = w.cloud.lock().unwrap();
        cloud.version = "1.5.0".into();
        cloud.version_id = "v-owner".into();
        cloud.owner = true;
    }
    w.install().unwrap();
    crate::recordstore::commands::insert_row(
        w.window.clone(),
        "notes".into(),
        vec![text_value("body", "before reset")],
        None,
    )
    .unwrap();
    crate::installation_commands::reset_runtime_installation_data(w.window.clone(), true).unwrap();
    assert!(w.notes().is_empty());
    assert!(crate::execute_read_query(w.window.clone(), "SELECT 1".into()).is_ok());
    let _ = crate::close_document(w.window.clone(), true);
}

#[test]
fn delivers_a_postgres_credential_by_envelope_and_key_grant_memory_only() {
    let w = world("cloud-install-pg");
    let datasource = json!({
        "kind": "postgres", "id": "pg-main", "host": "127.0.0.1", "port": 1, "database": "app",
        "user": "runtime", "sslmode": "require", "credentialMode": "shared",
    });
    let bytes = archive("cloud-install-pg-seed", "pg.ixt", false, Some(datasource.clone()));
    crate::open_document(
        w.window.clone(),
        crate::paths::state_dir().join("pg.ixt").to_string_lossy().into(),
    )
    .unwrap();
    let password_ref = crate::recordstore::commands::set_datasource_password(
        w.window.clone(),
        "pg-main".into(),
        "pg-secret-123".into(),
        serde_json::from_value(datasource.clone()).ok(),
    )
    .unwrap();
    let mut config = serde_json::to_value(crate::read_document_config(w.window.clone()).unwrap()).unwrap();
    let mut with_ref = datasource.clone();
    with_ref["passwordRef"] = json!(password_ref);
    config["datasource"] = with_ref;
    crate::update_document_config(w.window.clone(), serde_json::from_value(config).unwrap()).unwrap();

    LiveServices
        .upload_credential(&w.window, "dev-jwt", &w.app_id, "shared", None, None, None)
        .unwrap();
    let envelope = w.cloud.lock().unwrap().envelope.clone().unwrap();
    assert_eq!(
        (envelope["appId"].as_str(), envelope["datasourceId"].as_str(), envelope["scope"].as_str()),
        (Some(w.app_id.as_str()), Some("pg-main"), Some("shared"))
    );
    assert!(!envelope.to_string().contains("pg-secret-123"));

    let upload = |scope: &str, user: Option<&str>, password: Option<&str>, username: Option<&str>| {
        LiveServices.upload_credential(
            &w.window,
            "dev-jwt",
            &w.app_id,
            scope,
            user.map(str::to_string),
            password.map(str::to_string),
            username.map(str::to_string),
        )
    };
    let refused = upload("shared", None, Some("x"), Some("alice")).unwrap_err();
    assert!(refused.code == "VALIDATION" && refused.message.contains("per-user"), "{refused}");
    let refused = upload("user", Some("u-2"), None, Some("alice")).unwrap_err();
    assert!(refused.code == "VALIDATION" && refused.message.contains("password"), "{refused}");
    upload("user", Some("u-2"), Some("alice-pw-77"), Some("alice_db")).unwrap();
    let per_user = w.cloud.lock().unwrap().envelope.clone().unwrap();
    assert_eq!((per_user["scope"].as_str(), per_user["userId"].as_str()), (Some("user"), Some("u-2")));
    assert!(!per_user.to_string().contains("alice-pw-77") && !per_user.to_string().contains("alice_db"));
    w.cloud.lock().unwrap().envelope = Some(envelope.clone());
    crate::close_document(w.window.clone(), true).unwrap();

    {
        let mut cloud = w.cloud.lock().unwrap();
        cloud.archive = bytes;
        cloud.version = "2.0.0".into();
        cloud.version_id = "pg-v1".into();
    }
    w.install().unwrap();
    let expires = (chrono::Utc::now() + chrono::Duration::hours(24)).to_rfc3339();
    w.cloud.lock().unwrap().grant = Some(json!({
        "grantId": "g1",
        "dek": envelope["dek"],
        "envelope": { "ciphertext": envelope["ciphertext"], "nonce": envelope["nonce"], "aad": envelope["aad"] },
        "expiresAt": expires,
    }));
    let grant = LiveServices.key_grant(&w.window, "user-jwt").unwrap();
    assert!(grant.needed);
    assert!(grant.expires_at.is_some());
    assert!(!grant.attached, "nothing listens on the datasource port");
    assert!(LiveServices.runtime_info(&w.window).unwrap().grant_expires_at.is_some());

    // A grant whose envelope was altered cannot be decrypted.
    {
        let mut cloud = w.cloud.lock().unwrap();
        let mut tampered = cloud.grant.clone().unwrap();
        tampered["envelope"]["aad"] = json!("x");
        cloud.grant = Some(tampered);
    }
    assert_eq!(
        LiveServices.key_grant(&w.window, "user-jwt").err().map(|e| e.code),
        Some("CREDENTIAL_DECRYPT".into())
    );

    // A revoked user is refused and holds no credential afterwards.
    w.cloud.lock().unwrap().grant_error = Some((403, "REVOKED".into()));
    assert_eq!(
        LiveServices.key_grant(&w.window, "user-jwt").err().map(|e| e.code),
        Some("REVOKED".into())
    );
    assert!(LiveServices.runtime_info(&w.window).unwrap().grant_expires_at.is_none());

    // The plaintext secret is never stored with the installation.
    fn files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                files(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut all = Vec::new();
    files(&crate::cloud::install::cloud_root(), &mut all);
    for file in all {
        let bytes = std::fs::read(&file).unwrap_or_default();
        assert!(
            !bytes.windows(13).any(|w| w == b"pg-secret-123"),
            "{} holds the plaintext password",
            file.display()
        );
    }
    let _ = crate::close_document(w.window.clone(), true);
}

#[test]
fn uploads_the_saved_archive_for_publishing_and_restores_a_version_as_a_new_local_copy() {
    let w = world("cloud-install-publish");
    archive("cloud-install-publish-seed", "publish.ixt", false, None);
    crate::open_document(
        w.window.clone(),
        crate::paths::state_dir().join("publish.ixt").to_string_lossy().into(),
    )
    .unwrap();
    let preflight = LiveServices.publish_preflight(&w.window, 3).unwrap();
    assert!(preflight.blockers.is_empty(), "{:?}", preflight.blockers);
    assert_eq!(preflight.security.store, "sqlite");
    assert!(preflight.security.entity_policies_resolved);
    assert!(!preflight.size.over_cloud_limit);
    assert_eq!(preflight.size.cloud_limit_bytes, 500_000_000);

    let upload = LiveServices.upload_archive(&w.window, "dev-jwt", &w.app_id, "version", "up").unwrap();
    let saved = std::fs::read(crate::paths::state_dir().join("publish.ixt")).unwrap();
    let sha = sha256_hex(&saved);
    assert_eq!((upload.upload_id.as_str(), upload.sha256.as_str(), upload.size), ("up-1", sha.as_str(), saved.len() as u64));
    {
        let cloud = w.cloud.lock().unwrap();
        let (path, bytes) = cloud.uploaded.clone().unwrap();
        assert_eq!(path, "/storage/v1/object/upload/sign/app-archives/p.ixt?token=tok");
        assert_eq!(sha256_hex(&bytes), sha);
        let (_, body, _) = cloud.requests.last().unwrap();
        assert_eq!(
            (body["appId"].as_str(), body["kind"].as_str(), body["sha256"].as_str()),
            (Some(w.app_id.as_str()), Some("version"), Some(sha.as_str()))
        );
    }

    // An unsaved change must be saved before it can be published.
    let mut config = serde_json::to_value(crate::read_document_config(w.window.clone()).unwrap()).unwrap();
    config["name"] = json!("Changed");
    crate::update_document_config(w.window.clone(), serde_json::from_value(config).unwrap()).unwrap();
    assert_eq!(
        LiveServices.upload_archive(&w.window, "t", &w.app_id, "version", "").err().map(|e| e.code),
        Some("SAVE_REQUIRED".into())
    );

    let original = crate::document_state(w.window.clone()).unwrap();
    w.cloud.lock().unwrap().served_bytes = Some(saved.clone());
    let url = "/object/sign/app-archives/x.ixt?token=t";
    assert_eq!(
        LiveServices
            .restore_copy(&w.window, url, &"0".repeat(64), saved.len() as u64, Some("v9".into()), "")
            .err()
            .map(|e| e.code),
        Some("ARCHIVE_CHECKSUM".into())
    );
    let restored = LiveServices
        .restore_copy(&w.window, url, &sha, saved.len() as u64, Some("v9".into()), "")
        .unwrap();
    assert!(restored.path.is_none());
    assert_ne!(restored.document_id, original.document_id);
    assert!(w.notes().is_empty());
    let _ = crate::close_document(w.window.clone(), true);
}
