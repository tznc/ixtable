// Unit tests for the cloud client: manifests, envelopes, PKCE, config,
// error mapping, publish preflight, grants, and installs through the
// runtime installation flow.
use super::*;
use crate::archive::DocumentConfig;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use chrono::{Duration, Utc};
use ed25519_dalek::{Signer, SigningKey};
use rand_core::OsRng;
use serde_json::{json, Value};

fn manifest_json(now: chrono::DateTime<Utc>) -> Value {
    json!({
        "format": manifest::FORMAT,
        "appId": "app-1",
        "appName": "CRM",
        "versionId": "ver-1",
        "version": "1.2.0",
        "archiveSha256": "ab".repeat(32),
        "archiveSize": 1234,
        "minRuntimeVersion": null,
        "userId": "user-1",
        "roleId": "role-sales",
        "rolePermissions": {"navigation": ["n1"], "objects": [], "actions": []},
        "installationId": "inst-1",
        "fingerprint": "f00d",
        "issuedAt": now.to_rfc3339(),
        "expiresAt": (now + Duration::hours(1)).to_rfc3339(),
    })
}

fn sign(key: &SigningKey, manifest: &Value) -> String {
    let bytes = manifest::canonical_json(manifest).into_bytes();
    STANDARD.encode(key.sign(&bytes).to_bytes())
}

const EXPECT: manifest::Expect<'static> = manifest::Expect {
    app_id: "app-1",
    user_id: "user-1",
    installation_id: "inst-1",
};

#[test]
fn canonical_json_sorts_keys_at_every_level_without_whitespace() {
    let v = json!({"b": 1, "a": {"z": [3, {"y": true, "x": null}], "c": "é\"\n"}, "A": 2});
    assert_eq!(
        manifest::canonical_json(&v),
        r#"{"A":2,"a":{"c":"é\"\n","z":[3,{"x":null,"y":true}]},"b":1}"#
    );
}

#[test]
fn a_signed_manifest_verifies_and_every_tamper_fails_closed() {
    let key = SigningKey::generate(&mut OsRng);
    let now = Utc::now();
    let m = manifest_json(now);
    let sig = sign(&key, &m);
    let vk = key.verifying_key();
    let ok = manifest::verify(&m, &sig, &vk, now, &EXPECT).unwrap();
    assert_eq!(ok.role_id.as_deref(), Some("role-sales"));
    assert_eq!(ok.archive_size, 1234);

    // The manifest as a string: exact bytes and canonical bytes both verify.
    let text = Value::String(manifest::canonical_json(&m));
    assert!(manifest::verify(&text, &sig, &vk, now, &EXPECT).is_ok());
    // URL-safe signature encoding is accepted too.
    let url_sig = URL_SAFE_NO_PAD.encode(STANDARD.decode(&sig).unwrap());
    assert!(manifest::verify(&m, &url_sig, &vk, now, &EXPECT).is_ok());

    for (field, value) in [
        ("roleId", json!("role-admin")),
        ("archiveSha256", json!("cd".repeat(32))),
        ("userId", json!("user-2")),
        ("expiresAt", json!((now + Duration::days(365)).to_rfc3339())),
        ("rolePermissions", json!({"navigation": ["n1", "n2"]})),
    ] {
        let mut tampered = m.clone();
        tampered[field] = value;
        let e = manifest::verify(&tampered, &sig, &vk, now, &EXPECT).unwrap_err();
        assert_eq!(e.code, "MANIFEST_SIGNATURE", "{field}");
    }
    // Another key, a garbage signature, a missing manifest.
    let other = SigningKey::generate(&mut OsRng).verifying_key();
    assert_eq!(
        manifest::verify(&m, &sig, &other, now, &EXPECT)
            .unwrap_err()
            .code,
        "MANIFEST_SIGNATURE"
    );
    assert_eq!(
        manifest::verify(&m, "!!", &vk, now, &EXPECT)
            .unwrap_err()
            .code,
        "MANIFEST_SIGNATURE"
    );
    assert_eq!(
        manifest::verify(&Value::Null, &sig, &vk, now, &EXPECT)
            .unwrap_err()
            .code,
        "MANIFEST_SIGNATURE"
    );
}

#[test]
fn expired_future_foreign_or_malformed_manifests_are_rejected_even_when_signed() {
    let key = SigningKey::generate(&mut OsRng);
    let vk = key.verifying_key();
    let now = Utc::now();
    let check = |m: Value| {
        manifest::verify(&m, &sign(&key, &m), &vk, now, &EXPECT)
            .unwrap_err()
            .code
    };

    let mut expired = manifest_json(now - Duration::hours(2));
    expired["expiresAt"] = json!((now - Duration::seconds(1)).to_rfc3339());
    assert_eq!(check(expired), "MANIFEST_EXPIRED");
    assert_eq!(
        check(manifest_json(now + Duration::hours(1))),
        "MANIFEST_INVALID"
    );
    for (field, value) in [
        ("appId", "app-2"),
        ("userId", "user-2"),
        ("installationId", "inst-2"),
    ] {
        let mut m = manifest_json(now);
        m[field] = json!(value);
        assert_eq!(check(m), "MANIFEST_MISMATCH", "{field}");
    }
    let mut format = manifest_json(now);
    format["format"] = json!("ixtable-cloud-bundle/9");
    assert_eq!(check(format), "MANIFEST_INVALID");
    let mut sha = manifest_json(now);
    sha["archiveSha256"] = json!("not-a-digest");
    assert_eq!(check(sha), "MANIFEST_INVALID");
    let mut version = manifest_json(now);
    version["version"] = json!("latest");
    assert_eq!(check(version), "INVALID_VERSION");
    let mut big = manifest_json(now);
    big["archiveSize"] = json!(MAX_DOWNLOAD_BYTES + 1);
    assert_eq!(check(big), "TOO_LARGE");
    let mut missing = manifest_json(now);
    missing.as_object_mut().unwrap().remove("fingerprint");
    assert_eq!(check(missing), "MANIFEST_INVALID");
}

#[test]
fn archive_bytes_must_match_the_manifest_checksum_and_size() {
    let now = Utc::now();
    let m: manifest::Manifest = serde_json::from_value(manifest_json(now)).unwrap();
    assert!(manifest::check_archive(&m, &"AB".repeat(32), 1234).is_ok());
    assert_eq!(
        manifest::check_archive(&m, &"cd".repeat(32), 1234)
            .unwrap_err()
            .code,
        "ARCHIVE_CHECKSUM"
    );
    assert_eq!(
        manifest::check_archive(&m, &"ab".repeat(32), 1235)
            .unwrap_err()
            .code,
        "ARCHIVE_CHECKSUM"
    );
}

#[test]
fn credential_envelopes_round_trip_and_are_bound_to_their_aad_and_dek() {
    let aad = envelope::aad_for("app", "ds", "user", Some("u1"));
    assert_eq!(aad, "ixtable-credential/1|app|ds|user|u1");
    let s = envelope::seal(b"{\"password\":\"s3cret\"}", &aad).unwrap();
    assert!(!s.ciphertext.contains("s3cret"));
    assert_eq!(b64_any(&s.dek).unwrap().len(), 32);
    assert_eq!(b64_any(&s.nonce).unwrap().len(), 24);
    assert_eq!(
        envelope::open(&s.ciphertext, &s.nonce, &aad, &s.dek).unwrap(),
        b"{\"password\":\"s3cret\"}"
    );
    let other = envelope::seal(b"x", &aad).unwrap();
    assert_ne!(other.dek, s.dek, "a fresh DEK per envelope");
    let fail = |c: &str, n: &str, a: &str, d: &str| envelope::open(c, n, a, d).unwrap_err().code;
    assert_eq!(
        fail(
            &s.ciphertext,
            &s.nonce,
            "ixtable-credential/1|app|ds|shared|",
            &s.dek
        ),
        "CREDENTIAL_DECRYPT"
    );
    assert_eq!(
        fail(&s.ciphertext, &s.nonce, &aad, &other.dek),
        "CREDENTIAL_DECRYPT"
    );
    let mut ct = b64_any(&s.ciphertext).unwrap();
    ct[0] ^= 1;
    assert_eq!(
        fail(&STANDARD.encode(ct), &s.nonce, &aad, &s.dek),
        "CREDENTIAL_DECRYPT"
    );
    assert_eq!(
        fail(&s.ciphertext, &s.nonce, &aad, "AAAA"),
        "CREDENTIAL_DECRYPT"
    );
}

#[test]
fn pkce_matches_rfc7636_and_tracks_pending_sign_ins() {
    // RFC 7636 appendix B.
    assert_eq!(
        pkce::challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    let p = pkce::generate();
    assert_eq!(p.verifier.len(), 43);
    assert_eq!(pkce::challenge_for(&p.verifier), p.challenge);
    assert_ne!(pkce::generate().verifier, p.verifier);
    let started = pkce::begin();
    assert_eq!(
        pkce::verifier(&started.state).as_deref(),
        Some(started.verifier.as_str())
    );
    let url = pkce::approve_url("https://ixtable.app/", &started, Some("google"));
    assert!(url.starts_with("https://ixtable.app/desktop-auth?code_challenge="));
    assert!(url.contains(&format!("state={}", started.state)) && url.ends_with("&provider=google"));
    assert!(
        !url.contains(&started.verifier),
        "the verifier never leaves the app"
    );
    assert!(!pkce::approve_url("s", &started, Some("evil")).contains("provider"));
    pkce::finish(&started.state);
    assert_eq!(pkce::verifier(&started.state), None);
}

#[test]
fn config_prefers_env_then_preferences_then_build_defaults() {
    let env = |k: &str| (k == "IXTABLE_CLOUD_URL").then(|| "https://env.example/".to_string());
    let pref = |k: &str| (k == "cloud.anonKey").then(|| "pref-key".to_string());
    let c = config::resolve_with(env, pref, true);
    assert_eq!(c.url, "https://env.example");
    assert_eq!(c.anon_key, "pref-key");
    assert_eq!(c.site_url, config::DEV_SITE_URL);
    assert!(c.configured);
    let dev = config::resolve_with(|_| None, |_| None, true);
    assert_eq!(
        (dev.url.as_str(), dev.anon_key.as_str()),
        (config::DEV_URL, config::DEV_ANON_KEY)
    );
    let release = config::resolve_with(|_| None, |_| None, false);
    assert_eq!(
        release.configured,
        option_env!("IXTABLE_CLOUD_BUILD_URL").is_some()
            && option_env!("IXTABLE_CLOUD_BUILD_ANON_KEY").is_some()
    );
}

#[test]
fn the_public_key_parses_from_common_encodings_and_only_debug_builds_take_overrides() {
    let key = SigningKey::generate(&mut OsRng).verifying_key();
    let raw = key.as_bytes();
    let mut spki = vec![
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ];
    spki.extend_from_slice(raw);
    let hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let pem = format!(
        "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
        STANDARD.encode(&spki)
    );
    let jwk = json!({"kty": "OKP", "crv": "Ed25519", "x": URL_SAFE_NO_PAD.encode(raw)}).to_string();
    for text in [
        STANDARD.encode(raw),
        URL_SAFE_NO_PAD.encode(raw),
        hex,
        STANDARD.encode(&spki),
        pem,
        jwk,
    ] {
        assert_eq!(config::parse_public_key(&text).unwrap(), key, "{text}");
    }
    for bad in ["", "abc", &STANDARD.encode([0u8; 31]), r#"{"kty":"RSA"}"#] {
        assert_eq!(
            config::parse_public_key(bad).unwrap_err().code,
            "CLOUD_KEY_INVALID",
            "{bad}"
        );
    }
    let b64 = STANDARD.encode(raw);
    let env = move |k: &str| (k == "IXTABLE_CLOUD_PUBLIC_KEY").then(|| b64.clone());
    let debug = config::resolve_with(&env, |_| None, true);
    assert_eq!(
        debug.public_key_fingerprint,
        Some(crate::bundle::fingerprint(&config::base64_key(&key)))
    );
    let release = config::resolve_with(&env, |_| None, false);
    assert_ne!(
        release.public_key_fingerprint, debug.public_key_fingerprint,
        "release ignores the runtime override"
    );
}

#[test]
fn cloud_errors_map_to_stable_codes_without_leaking_urls() {
    let e = http::error_from(
        409,
        r#"{"error":{"code":"VERSION_CONFLICT","message":"Head moved"}}"#,
    );
    assert_eq!(
        (e.code.as_str(), e.message.as_str()),
        ("VERSION_CONFLICT", "Head moved")
    );
    assert_eq!(
        http::error_from(403, r#"{"error":{"code":"REVOKED","message":"x"}}"#).code,
        "REVOKED"
    );
    assert_eq!(http::error_from(401, "<html>").code, "UNAUTHENTICATED");
    assert_eq!(http::error_from(413, "").code, "TOO_LARGE");
    assert_eq!(
        http::error_from(428, r#"{"error":{"code":"PENDING"}}"#).code,
        "PENDING"
    );
    assert_eq!(
        http::error_from(502, "bad gateway").code,
        "CLOUD_UNAVAILABLE"
    );
    // Unknown shapes or hostile codes fall back to the status mapping.
    assert_eq!(
        http::error_from(402, r#"{"error":{"code":"bad code; drop"}}"#).code,
        "ENTITLEMENT_REQUIRED"
    );
    assert_eq!(
        http::error_from(400, r#"{"code":"VALIDATION","message":"m"}"#).code,
        "VALIDATION"
    );
    assert_eq!(http::error_from(400, r#"{"msg":"m"}"#).message, "m");
}

#[test]
fn signed_storage_urls_are_rebased_onto_the_configured_host() {
    let base = "http://127.0.0.1:54321";
    assert_eq!(
        http::rebase(
            base,
            "http://kong:8000/storage/v1/object/upload/sign/app-archives/a.ixt?token=t"
        ),
        "http://127.0.0.1:54321/storage/v1/object/upload/sign/app-archives/a.ixt?token=t"
    );
    assert_eq!(
        http::rebase(base, "/object/sign/app-archives/a.ixt?token=t"),
        "http://127.0.0.1:54321/storage/v1/object/sign/app-archives/a.ixt?token=t"
    );
    assert_eq!(
        http::rebase(base, "https://s3.example/x?y"),
        "https://s3.example/x?y"
    );
}

fn size(over: bool) -> crate::assets::ArchiveSizeReport {
    crate::assets::ArchiveSizeReport {
        total_bytes: if over { 600_000_000 } else { 1000 },
        records_bytes: 0,
        config_bytes: 0,
        assets_bytes: 0,
        other_bytes: 0,
        asset_share: 0.0,
        largest: vec![],
        cloud_limit_bytes: 500_000_000,
        over_cloud_limit: over,
        measured: "saved".into(),
    }
}

#[test]
fn publish_preflight_blocks_unsafe_or_invalid_checkpoints() {
    let mut config = DocumentConfig::default();
    let tables = vec!["Customers".to_string(), "_ixtable_migrations".to_string()];
    let p = publish::assess(&config, &tables, size(false), vec![], 1);
    assert!(p.blockers.is_empty(), "{:?}", p.blockers);
    assert_eq!(p.unresolved_entities, vec!["Customers"]);
    assert_eq!(p.warnings.len(), 1, "a single runtime user only warns");
    assert_eq!(p.security.store, "sqlite");
    assert!(!p.security.entity_policies_resolved);

    assert_eq!(
        publish::assess(&config, &tables, size(false), vec![], 2)
            .blockers
            .len(),
        1
    );
    assert_eq!(
        publish::assess(&config, &tables, size(true), vec![], 0)
            .blockers
            .len(),
        1
    );
    let errors = vec![crate::validation::Issue::error("form", "f", "broken")];
    assert!(
        publish::assess(&config, &[], size(false), errors, 0).blockers[0]
            .contains("validation error")
    );

    config.entities.push(crate::recordstore::EntitySettings {
        id: "e".into(),
        table: "Customers".into(),
        ..Default::default()
    });
    config.datasource.kind = "postgres".into();
    config.datasource.sslmode = "disable".into();
    let p = publish::assess(&config, &tables, size(false), vec![], 0);
    assert!(p.security.entity_policies_resolved);
    assert_eq!(p.blockers.len(), 1, "non-TLS without the override blocks");
    assert!(p.security.shared_credential_warning && !p.security.tls);
    config.datasource.insecure_transport_confirmed = true;
    config.datasource.credential_mode = "perUser".into();
    let p = publish::assess(&config, &tables, size(false), vec![], 0);
    assert!(p.blockers.is_empty());
    assert!(p.warnings[0].starts_with("SEVERE"));
    assert!(p.security.insecure_override_confirmed && !p.security.shared_credential_warning);
    // PostgreSQL shares records between users: an unresolved entity always blocks.
    config.entities.clear();
    assert_eq!(
        publish::assess(&config, &tables, size(false), vec![], 0)
            .blockers
            .len(),
        1
    );
}

#[test]
fn granted_credentials_are_released_only_to_their_target_until_expiry() {
    let mut ds = crate::recordstore::DatasourceConfig {
        kind: "postgres".into(),
        id: "grant-ds".into(),
        host: "db.example".into(),
        database: "app".into(),
        user: "u".into(),
        grant_scope: Some("cloud-w".into()),
        ..Default::default()
    };
    let target = crate::recordstore::secrets::datasource_target(&ds);
    grants::put(
        "cloud-w",
        &target,
        None,
        "pw".into(),
        Utc::now() + Duration::hours(1),
    );
    assert_eq!(grants::granted_password(&ds).as_deref(), Some("pw"));
    assert_eq!(
        crate::recordstore::secrets::datasource_credential(&ds)
            .unwrap()
            .as_deref(),
        Some("pw"),
        "the record store sees the grant before the local store"
    );
    let mut other = ds.clone();
    other.host = "evil.example".into();
    assert_eq!(grants::granted_password(&other), None);
    grants::clear("cloud-w", &target);
    assert_eq!(grants::granted_password(&ds), None);
    grants::put(
        "cloud-w",
        &target,
        None,
        "old".into(),
        Utc::now() - Duration::seconds(1),
    );
    assert_eq!(
        grants::granted_password(&ds),
        None,
        "expired grants are dropped"
    );
    ds.id = "grant-ds-2".into();
    assert_eq!(grants::expires_at(&ds), None);
}

#[test]
fn a_grant_held_for_one_cloud_window_changes_no_other_session() {
    use crate::recordstore::secrets::{datasource_login, datasource_target, LoginSource};
    let cloud = crate::recordstore::DatasourceConfig {
        kind: "postgres".into(),
        id: format!("scoped-{}", uuid::Uuid::new_v4()),
        host: "db.example".into(),
        database: "app".into(),
        user: "app_owner".into(),
        grant_scope: Some("cloud-a".into()),
        ..Default::default()
    };
    let target = datasource_target(&cloud);
    grants::put(
        "cloud-a",
        &target,
        Some("alice".into()),
        "alice-pw".into(),
        Utc::now() + Duration::hours(1),
    );
    let login = datasource_login(&cloud).unwrap();
    assert_eq!(login.source, LoginSource::Grant);
    assert_eq!(login.user, "alice");
    assert!(grants::expires_at(&cloud).is_some());
    // Another cloud window, Studio, and a manual install keep their identity.
    let other_window = crate::recordstore::DatasourceConfig {
        grant_scope: Some("cloud-b".into()),
        ..cloud.clone()
    };
    let studio = crate::recordstore::DatasourceConfig {
        grant_scope: None,
        ..cloud.clone()
    };
    let manual = crate::recordstore::DatasourceConfig {
        grant_scope: None,
        installation: Some("bundle".into()),
        ..cloud.clone()
    };
    for ds in [&other_window, &studio, &manual] {
        let login = datasource_login(ds).unwrap();
        assert_eq!(login.source, LoginSource::None, "{:?}", ds.grant_scope);
        assert_eq!(login.user, "app_owner");
        assert_eq!(login.password, None);
        assert_eq!(grants::expires_at(ds), None);
    }
    // Closing the cloud session drops its grant.
    grants::release(&cloud);
    assert_eq!(grants::granted_password(&cloud), None);
}

#[test]
fn a_v2_grant_connects_as_its_per_user_database_user() {
    use crate::cloud::contract::{GrantEnvelope, GrantReply};
    let ds = crate::recordstore::DatasourceConfig {
        kind: "postgres".into(),
        id: "ds-v2".into(),
        host: "db.example".into(),
        database: "app".into(),
        user: "app_owner".into(),
        ..Default::default()
    };
    let target = crate::recordstore::secrets::datasource_target(&ds);
    let reply = |user: Option<&str>, target: &str| {
        let cred = envelope::Credential {
            v: if user.is_some() { 2 } else { 1 },
            kind: "postgres".into(),
            user: user.map(str::to_string),
            password: "pw-v2".into(),
            target: target.into(),
        };
        let aad = envelope::aad_for("app", "ds-v2", "user", Some("u1"));
        let s = envelope::seal(&serde_json::to_vec(&cred).unwrap(), &aad).unwrap();
        GrantReply {
            dek: s.dek,
            envelope: GrantEnvelope {
                ciphertext: s.ciphertext,
                nonce: s.nonce,
                aad,
            },
            expires_at: None,
        }
    };
    let open = |r: &GrantReply| super::runtime_commands::open_credential("app", &ds, r);
    assert_eq!(
        open(&reply(Some("alice"), &target)).unwrap(),
        (Some("alice".into()), "pw-v2".into())
    );
    assert_eq!(open(&reply(None, &target)).unwrap(), (None, "pw-v2".into()));
    assert_eq!(open(&reply(Some(""), &target)).unwrap().0, None);
    assert_eq!(
        open(&reply(Some("bad\nuser"), &target)).unwrap_err().code,
        "CREDENTIAL_DECRYPT"
    );
    assert_eq!(
        open(&reply(Some("alice"), "other-target"))
            .unwrap_err()
            .code,
        "CREDENTIAL_TARGET_MISMATCH"
    );
    assert_eq!(
        super::runtime_commands::open_credential("other-app", &ds, &reply(Some("alice"), &target))
            .unwrap_err()
            .code,
        "CREDENTIAL_DECRYPT"
    );
}

#[test]
fn cloud_installs_reuse_the_runtime_installation_flow_and_keep_records() {
    use crate::bundle::{archive_bytes, sha256_hex, SignedBundle};
    let root = std::env::temp_dir().join(format!("ixtable-cloud-install-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let doc = crate::archive::create_document("CRM").unwrap();
    let bytes = archive_bytes(&doc, &root.join(".tmp")).unwrap();
    let cloud_key = SigningKey::generate(&mut OsRng).verifying_key();
    let key_b64 = config::base64_key(&cloud_key);
    let now = Utc::now();
    let mut m: manifest::Manifest = serde_json::from_value(manifest_json(now)).unwrap();
    m.archive_sha256 = sha256_hex(&bytes);
    m.archive_size = bytes.len() as u64;
    m.version = "1.0.0".into();
    let id = &doc.metadata.document_id;
    let header = install::header_for(&m, id, &key_b64);
    assert_eq!(header.payload_sha256, m.archive_sha256);
    let (dir, action) =
        crate::installation::apply_bundle(&root, &SignedBundle::trusted(header), &bytes)
            .unwrap();
    assert_eq!(action, crate::installation::Action::Install);
    let conn = rusqlite::Connection::open(dir.join("data.db")).unwrap();
    conn.execute_batch("CREATE TABLE Notes(id INTEGER PRIMARY KEY, body TEXT); INSERT INTO Notes(body) VALUES('runtime record')").unwrap();
    drop(conn);
    m.version = "1.1.0".into();
    let (_, action) = crate::installation::apply_bundle(
        &root,
        &SignedBundle::trusted(install::header_for(&m, id, &key_b64)),
        &bytes,
    )
    .unwrap();
    assert_eq!(action, crate::installation::Action::Update);
    let conn = rusqlite::Connection::open(dir.join("data.db")).unwrap();
    let kept: String = conn
        .query_row("SELECT body FROM Notes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, "runtime record");
    // A different signing key cannot take over the installation.
    let other = config::base64_key(&SigningKey::generate(&mut OsRng).verifying_key());
    m.version = "1.2.0".into();
    let err = crate::installation::apply_bundle(
        &root,
        &SignedBundle::trusted(install::header_for(&m, id, &other)),
        &bytes,
    )
    .unwrap_err();
    assert_eq!(err.code, "BUNDLE_SIGNER_MISMATCH");
    // Older published versions never install.
    m.version = "0.9.0".into();
    let err = crate::installation::apply_bundle(
        &root,
        &SignedBundle::trusted(install::header_for(&m, id, &key_b64)),
        &bytes,
    )
    .unwrap_err();
    assert_eq!(err.code, "BUNDLE_DOWNGRADE");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cloud_ids_and_auth_storage_keys_are_restricted() {
    assert!(check_id("app", "0190c3f4-aaaa-7bbb-8ccc-0123456789ab").is_ok());
    for bad in ["", "../x", "a/b", "a b"] {
        assert_eq!(check_id("app", bad).unwrap_err().code, "VALIDATION");
    }
    assert!(install::app_root("../../etc").is_err());
}

#[test]
fn a_cloud_archive_with_a_traversal_document_id_is_refused_before_any_path_is_built() {
    use crate::bundle::{archive_bytes, sha256_hex};
    let root =
        std::env::temp_dir().join(format!("ixtable-cloud-traversal-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let doc = crate::archive::create_document("CRM").unwrap();
    let archive = root.join("hostile.ixt");
    std::fs::write(&archive, archive_bytes(&doc, &root.join(".tmp")).unwrap()).unwrap();
    // A hostile archive: its header names a directory outside the app root.
    let conn = rusqlite::Connection::open(&archive).unwrap();
    conn.execute(
        "UPDATE archive_metadata SET document_id='../../escaped'",
        [],
    )
    .unwrap();
    drop(conn);
    let bytes = std::fs::read(&archive).unwrap();
    let key_b64 = config::base64_key(&SigningKey::generate(&mut OsRng).verifying_key());
    let mut m: manifest::Manifest = serde_json::from_value(manifest_json(Utc::now())).unwrap();
    m.archive_sha256 = sha256_hex(&bytes);
    m.archive_size = bytes.len() as u64;
    let err = install::install_verified(
        "main",
        &m,
        &Value::Null,
        "sig",
        "a@example.com",
        &archive,
        &key_b64,
    )
    .unwrap_err();
    assert!(
        ["VALIDATION", "INVALID_ARCHIVE"].contains(&err.code.as_str())
            || err.message.contains("document id"),
        "{err:?}"
    );
    assert!(!root.join("escaped").exists() && !std::env::temp_dir().join("escaped").exists());
    let _ = std::fs::remove_dir_all(root);
}

fn record_for(m: &Value, sig: &str, signed: bool) -> install::CloudRecord {
    install::CloudRecord {
        manifest: serde_json::from_value(m.clone()).unwrap(),
        signature: sig.into(),
        email: "rep@example.com".into(),
        public_key_fingerprint: "fp".into(),
        recorded_at: Utc::now().to_rfc3339(),
        signed_manifest: signed.then(|| m.clone()),
    }
}

#[test]
fn a_tampered_cloud_record_never_grants_a_role_or_owner_access() {
    let key = SigningKey::generate(&mut OsRng);
    let vk = key.verifying_key();
    let mut m = manifest_json(Utc::now() - Duration::days(30));
    // Every field present, as bundle-manifest issues it (older records are re-serialized).
    m["roleName"] = json!("Sales rep");
    m["owner"] = json!(false);
    let sig = sign(&key, &m);
    // Verified even when expired (offline use), with and without the signed copy.
    for signed in [true, false] {
        let verified = install::verify_record(&record_for(&m, &sig, signed), &vk).unwrap();
        let role = install::manifest_role(&verified).unwrap();
        assert_eq!(role.id, "role-sales");
        assert_eq!(role.permissions.navigation, vec!["n1".to_string()]);
    }
    // Editing the parsed copy is ignored: only the signed manifest counts.
    let mut record = record_for(&m, &sig, true);
    record.manifest.owner = true;
    record.manifest.role_id = None;
    let verified = install::verify_record(&record, &vk).unwrap();
    assert_eq!(verified.role_id.as_deref(), Some("role-sales"));
    assert!(!verified.owner);
    // Editing what is verified (owner flag, role, permissions) fails closed.
    for (field, value) in [
        ("owner", json!(true)),
        ("roleId", json!(null)),
        (
            "rolePermissions",
            json!({"objects": [{"kind": "table", "id": "t", "read": true}]}),
        ),
    ] {
        let mut edited = m.clone();
        edited[field] = value;
        for signed in [true, false] {
            let e = install::verify_record(&record_for(&edited, &sig, signed), &vk).unwrap_err();
            assert_eq!(e.code, "INSTALLATION_TAMPERED", "{field} signed={signed}");
        }
    }
    let other = SigningKey::generate(&mut OsRng).verifying_key();
    assert!(install::verify_record(&record_for(&m, &sig, true), &other).is_err());
}

#[test]
fn manifest_roles_map_like_the_frontend() {
    let mut m: manifest::Manifest = serde_json::from_value(manifest_json(Utc::now())).unwrap();
    m.owner = true;
    m.role_id = None;
    assert_eq!(install::manifest_role(&m), None);
    m.owner = false;
    assert_eq!(
        install::manifest_role(&m).unwrap().id,
        crate::authz::NO_ROLE
    );
    m.role_permissions = json!("garbage");
    assert_eq!(
        install::manifest_role(&m).unwrap().permissions,
        crate::roles::Permissions::default()
    );
    // An owner with an assigned role is held to that role.
    m.owner = true;
    m.role_id = Some("role-sales".into());
    assert_eq!(install::manifest_role(&m).unwrap().id, "role-sales");
}

// Serves one canned status per connection and records each request body.
type Bodies = std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>;
fn mock_storage(statuses: Vec<u16>) -> (String, Bodies) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let bodies: Bodies = Default::default();
    let seen = bodies.clone();
    std::thread::spawn(move || {
        for status in statuses {
            let Ok((mut s, _)) = listener.accept() else {
                return;
            };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            let (head_end, len) = loop {
                let n = s.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    return;
                }
                buf.extend_from_slice(&chunk[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                    let len = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .map(|v| v.trim().parse::<usize>().unwrap())
                        .unwrap_or(0);
                    break (i + 4, len);
                }
            };
            while buf.len() < head_end + len {
                let n = s.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            seen.lock().unwrap().push(buf[head_end..].to_vec());
            let _ = write!(
                s,
                "HTTP/1.1 {status} X\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{{}}"
            );
        }
    });
    (url, bodies)
}

fn upload_with(statuses: Vec<u16>) -> (Result<(), crate::manager::AppError>, usize) {
    let (url, bodies) = mock_storage(statuses);
    let file = std::env::temp_dir().join(format!("ixt-upload-{}.ixt", uuid::Uuid::new_v4()));
    std::fs::write(&file, b"archive-bytes").unwrap();
    let cfg = config::CloudConfig {
        url: url.clone(),
        anon_key: "anon".into(),
        site_url: String::new(),
        configured: true,
        public_key_fingerprint: None,
    };
    let result = http::upload_file_with(
        &cfg,
        &format!("{url}/storage/v1/object/upload/sign/x"),
        &file,
        "",
        4,
        std::time::Duration::from_millis(1),
    );
    let _ = std::fs::remove_file(&file);
    let bodies = bodies.lock().unwrap();
    assert!(bodies.iter().all(|b| b == b"archive-bytes"));
    (result, bodies.len())
}

#[test]
fn uploads_retry_transient_failures_with_the_full_body_each_time() {
    let (result, attempts) = upload_with(vec![503, 429, 200]);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(attempts, 3);
}

#[test]
fn uploads_never_retry_auth_or_precondition_failures() {
    for status in [401, 403, 409, 412, 413] {
        let (result, attempts) = upload_with(vec![status, 200]);
        assert!(result.is_err(), "{status}");
        assert_eq!(attempts, 1, "{status} must not be retried");
    }
}

#[test]
fn uploads_give_up_after_the_bounded_number_of_attempts() {
    let (result, attempts) = upload_with(vec![500, 502, 503, 504, 200]);
    assert_eq!(result.unwrap_err().code, "CLOUD_UNAVAILABLE");
    assert_eq!(attempts, 4);
}

#[test]
fn retry_backs_off_exponentially_and_retries_connect_errors() {
    let mut calls = 0;
    let start = std::time::Instant::now();
    let out = http::with_retry(3, std::time::Duration::from_millis(10), |_| {
        calls += 1;
        Err::<(), _>(err("CLOUD_OFFLINE", "down"))
    });
    assert_eq!(out.unwrap_err().code, "CLOUD_OFFLINE");
    assert_eq!(calls, 3);
    // 10 ms + 20 ms between the three attempts.
    assert!(start.elapsed() >= std::time::Duration::from_millis(30));
}

#[test]
fn per_user_envelopes_carry_their_own_database_user() {
    let v1: envelope::Credential =
        serde_json::from_str(r#"{"v":1,"kind":"postgres","password":"pw","target":"t"}"#).unwrap();
    assert_eq!(v1.user, None, "v1 envelopes keep the datasource user");
    let v2 = envelope::Credential {
        v: 2,
        kind: "postgres".into(),
        user: Some("alice".into()),
        password: "alice-pw".into(),
        target: "t".into(),
    };
    let aad = envelope::aad_for("app", "ds", "user", Some("u1"));
    let sealed = envelope::seal(&serde_json::to_vec(&v2).unwrap(), &aad).unwrap();
    assert!(!sealed.ciphertext.contains("alice"));
    let plain = envelope::open(&sealed.ciphertext, &sealed.nonce, &aad, &sealed.dek).unwrap();
    let back: envelope::Credential = serde_json::from_slice(&plain).unwrap();
    assert_eq!(back.user.as_deref(), Some("alice"));
    assert_eq!(back.password, "alice-pw");
}
