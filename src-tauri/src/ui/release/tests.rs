//! Release flows (ports of tests/integration/{bundle,runtime-database-login,
//! runtime-update-diagnostics}.test.tsx and open-files' bundle case). Imports are
//! explicit: `gpui_kit::*` would shadow `#[test]`.

use crate::{
    data::DataValue,
    ui::{
        shell::ModeId,
        testing::{Harness, shown},
    },
};
use gpui_kit::{ScrollDelta, TestAppContext, point, px, test::TestWindowExt as _};
use serde_json::{Value, json};
use std::path::PathBuf;

fn bundle_path(h: &Harness, name: &str) -> PathBuf {
    h.temp_path(name)
}

/// Scrolls the settings panel to its end (the export button sits below the form).
fn scroll_to_end(h: &mut Harness) {
    h.update(|w, cx| {
        w.scroll(
            "settings-panel",
            ScrollDelta::Pixels(point(px(0.), px(-5000.))),
            cx,
        )
    });
    h.settle();
}

fn open_release_tab(h: &mut Harness) {
    h.switch_mode(ModeId::App);
    h.click("tab-release");
}

fn export_bundle(h: &mut Harness, version: &str, path: &PathBuf, password: Option<&str>) {
    h.fill("release-version", version);
    if let Some(password) = password {
        if !h.exists("release-password") {
            h.click("release-protect");
        }
        h.fill("release-password", password);
        h.fill("release-confirm", password);
    }
    h.dialogs.queue_save(Some(path.clone()));
    scroll_to_end(h);
    h.click("export-bundle");
    assert_eq!(
        h.label("exported-bundle"),
        format!("Exported Untitled {version}"),
        "{:?}",
        h.try_label("release-error")
    );
}

fn close_to_start(h: &mut Harness) {
    h.dialogs.queue_confirm(true);
    h.click("close-project");
    assert!(h.exists("open-bundle"));
}

fn seed_studio_document(h: &mut Harness) {
    h.create_table("Customers", &[("id", "INTEGER", 1), ("name", "TEXT", 0)]);
    h.insert_row("Customers", &[("name", DataValue::Text("Ada".into()))]);
}

fn customer_names(h: &Harness) -> Vec<String> {
    let mut names: Vec<String> = h.read_page("Customers").rows.iter().map(|r| shown(&r[1])).collect();
    names.sort();
    names
}

fn open_bundle_from_start(h: &mut Harness, path: &PathBuf) {
    h.dialogs.queue_open(Some(path.clone()));
    h.click("open-bundle");
}

fn code<T>(result: Result<T, crate::manager::AppError>) -> String {
    result.map_or_else(|e| e.code, |_| "OK".into())
}

#[gpui_kit::test]
fn exports_from_studio_opens_runtime_only_and_updates_keeping_runtime_records(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    seed_studio_document(&mut h);
    open_release_tab(&mut h);
    assert!(
        h.exists("release-heading"),
        "the Release tab did not open"
    );
    let v1 = bundle_path(&h, "crm-1.0.0.ixtr");
    export_bundle(&mut h, "1.0.0", &v1, None);
    assert!(h.label("bundle-sha256").len() == 64);
    assert!(h.exists("bundle-fingerprint"));
    let v2 = bundle_path(&h, "crm-1.1.0.ixtr");
    export_bundle(&mut h, "1.1.0", &v2, None);
    close_to_start(&mut h);

    open_bundle_from_start(&mut h, &v1);
    assert!(h.exists("runtime-bundle"));
    assert_eq!(h.label("runtime-version"), "Version 1.0.0");
    // Runtime-only windows hide the Studio chrome.
    for hidden in ["save-project", "save-project-as", "mode-data", "mode-app", "document-name"] {
        assert!(!h.exists(hidden), "{hidden}");
    }
    let state = h.state();
    assert!(state.runtime_only);
    assert_eq!(state.bundle_version.as_deref(), Some("1.0.0"));
    let config = crate::read_document_config(h.label.clone()).unwrap();
    assert_eq!(
        code(crate::update_document_config(h.label.clone(), config)),
        "READ_ONLY"
    );
    let rename = serde_json::from_value(json!({ "operation": "rename_table", "newName": "Clients" })).unwrap();
    assert_eq!(
        code(crate::alter_database_table(h.label.clone(), "Customers".into(), rename)),
        "READ_ONLY"
    );
    assert_eq!(code(crate::migrations::commands::apply_migrations(h.label.clone())), "READ_ONLY");
    assert_eq!(code(crate::migrations::commands::rollback_migration(h.label.clone())), "READ_ONLY");
    assert_eq!(
        code(crate::migrations::commands::dry_run_migrations(h.label.clone(), None)),
        "READ_ONLY"
    );

    h.insert_row("Customers", &[("name", DataValue::Text("Runtime Rita".into()))]);
    assert_eq!(customer_names(&h), ["Ada", "Runtime Rita"]);

    h.dialogs.queue_open(Some(v2));
    h.click("check-update");
    assert_eq!(h.label("update-confirm"), "Update Untitled to 1.1.0?");
    assert_eq!(h.label("no-migrations"), "No migrations will run on your records.");
    assert_eq!(customer_names(&h), ["Ada", "Runtime Rita"]);
    h.click("apply-update");
    assert!(h.notice().unwrap().contains("Updated to version 1.1.0. Your records were kept."));
    assert_eq!(h.label("runtime-version"), "Version 1.1.0");
    assert_eq!(customer_names(&h), ["Ada", "Runtime Rita"]);

    h.dialogs.queue_open(Some(v1));
    h.click("check-update");
    // An older version is refused before any confirm step; the installation is untouched.
    assert!(!h.exists("update-confirm"));
    let error = h.error_at("bundle-error");
    assert!(error.starts_with("BUNDLE_DOWNGRADE: A newer version"), "{error}");
    assert!(error.ends_with("Installed: 1.1.0. This file: 1.0.0."), "{error}");
    assert_eq!(h.label("runtime-version"), "Version 1.1.0");
    assert_eq!(customer_names(&h), ["Ada", "Runtime Rita"]);
}

#[gpui_kit::test]
fn asks_for_the_password_of_a_protected_bundle_and_explains_a_wrong_one(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_studio_document(&mut h);
    open_release_tab(&mut h);
    let path = bundle_path(&h, "protected.ixtr");
    export_bundle(&mut h, "2.0.0", &path, Some("correct horse"));
    assert_eq!(h.label("bundle-encrypted"), "Yes");
    close_to_start(&mut h);

    open_bundle_from_start(&mut h, &path);
    assert_eq!(h.label("bundle-password"), "Password for Untitled 2.0.0");
    h.fill("bundle-password-input", "wrong password");
    h.click("bundle-unlock");
    assert!(h.error_at("bundle-error").contains("The password is incorrect."));

    h.fill("bundle-password-input", "correct horse");
    h.click("bundle-unlock");
    assert!(h.exists("runtime-bundle"));
    assert_eq!(h.label("runtime-version"), "Version 2.0.0");
    assert_eq!(customer_names(&h), ["Ada"]);
}

#[gpui_kit::test]
fn closes_the_password_prompt_on_escape(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    open_release_tab(&mut h);
    let path = bundle_path(&h, "escape.ixtr");
    export_bundle(&mut h, "1.0.0", &path, Some("long enough"));
    close_to_start(&mut h);
    open_bundle_from_start(&mut h, &path);
    assert!(h.exists("bundle-password"));
    h.press("escape");
    assert!(!h.exists("bundle-password"));
    assert!(!h.has_shell());
}

#[gpui_kit::test]
fn rejects_a_tampered_bundle_before_opening_it(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_studio_document(&mut h);
    open_release_tab(&mut h);
    let path = bundle_path(&h, "tampered.ixtr");
    export_bundle(&mut h, "1.0.0", &path, None);
    let mut bytes = std::fs::read(&path).unwrap();
    let at = bytes.len() - 100;
    bytes[at] ^= 0xff;
    std::fs::write(&path, bytes).unwrap();
    close_to_start(&mut h);

    open_bundle_from_start(&mut h, &path);
    let error = h.error_at("bundle-error");
    assert!(error.contains("BUNDLE_SIGNATURE"), "{error}");
    assert!(error.contains("changed after it was signed"), "{error}");
    assert!(!h.exists("runtime-bundle"));
}

#[gpui_kit::test]
fn validates_the_release_form_before_exporting(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    open_release_tab(&mut h);
    h.fill("release-version", "one");
    assert_eq!(h.label("release-version-hint"), "Enter a semantic version such as 1.2.0.");
    h.fill("release-version", "1.2.0");
    assert_eq!(
        h.label("release-version-hint"),
        "Semantic version; each update needs a higher version."
    );
    h.fill("release-min-runtime", "x");
    assert_eq!(h.label("release-min-hint"), "Enter a semantic version or leave blank.");
    h.fill("release-min-runtime", "");
    h.click("release-protect");
    h.fill("release-password", "short");
    assert_eq!(h.label("release-password-hint"), "Use at least 8 characters.");
    h.fill("release-password", "long enough");
    h.fill("release-confirm", "different");
    assert_eq!(h.label("release-password-hint"), "The passwords do not match.");
}

#[gpui_kit::test]
fn blocks_export_while_expressions_have_errors(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    open_release_tab(&mut h);
    super::gate::set_expression_gate(Some(|_| 2));
    let path = bundle_path(&h, "blocked.ixtr");
    h.dialogs.queue_save(Some(path.clone()));
    h.click("export-bundle");
    super::gate::set_expression_gate(None);
    let error = h.error_at("release-error");
    assert!(error.contains("EXPRESSION_ERRORS"), "{error}");
    assert!(error.contains("2 expression errors must be fixed first"), "{error}");
    assert!(!path.exists());
    assert_eq!(h.dialogs.count("save"), 0);
}

fn add_migration(h: &Harness) {
    let mut config: Value =
        serde_json::to_value(crate::read_document_config(h.label.clone()).unwrap()).unwrap();
    config["migrations"] = json!([{
        "id": "0190c3f4-eeee-7bbb-8ccc-0123456789ab",
        "name": "Add tags",
        "order": 1,
        "targetStore": "sqlite",
        "up": "CREATE TABLE tags (id INTEGER PRIMARY KEY, label TEXT)",
    }]);
    crate::update_document_config(h.label.clone(), serde_json::from_value(config).unwrap()).unwrap();
    crate::migrations::commands::apply_migrations(h.label.clone()).unwrap();
}

#[gpui_kit::test]
fn previews_a_protected_updates_notes_and_migrations_then_shows_jobs_and_logs(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    seed_studio_document(&mut h);
    open_release_tab(&mut h);
    let v1 = bundle_path(&h, "diag-1.0.0.ixtr");
    export_bundle(&mut h, "1.0.0", &v1, None);
    add_migration(&h);
    let v2 = bundle_path(&h, "diag-2.0.0.ixtr");
    let options = serde_json::from_value(json!({
        "version": "2.0.0", "releaseNotes": "Adds tags.", "password": "s3cret-pass",
    }))
    .unwrap();
    crate::bundle_export::export_runtime_bundle(h.label.clone(), v2.to_string_lossy().into(), options)
        .unwrap();

    close_to_start(&mut h);
    open_bundle_from_start(&mut h, &v1);
    assert!(h.exists("runtime-bundle"));

    h.dialogs.queue_open(Some(v2));
    h.click("check-update");
    h.fill("bundle-password-input", "s3cret-pass");
    h.click("bundle-unlock");
    assert_eq!(h.label("update-confirm"), "Update Untitled to 2.0.0?");
    assert_eq!(h.label("release-notes-text"), "Adds tags.");
    assert_eq!(h.label("migration:0"), "Add tags");
    assert!(h.label("migration-sql:0").contains("CREATE TABLE tags"));
    assert_eq!(h.label("runtime-version"), "Version 1.0.0");
    h.click("apply-update");
    assert!(h.notice().unwrap().contains("Updated to version 2.0.0"));
    let objects = crate::list_database_objects(h.label.clone()).unwrap();
    assert!(objects.iter().any(|o| o.name == "tags"));

    h.dialogs.queue_open(Some(v1));
    h.click("check-update");
    assert!(!h.exists("update-confirm"));
    assert!(h.error_at("bundle-error").starts_with("BUNDLE_DOWNGRADE"));
    assert_eq!(h.label("runtime-version"), "Version 2.0.0");

    h.click("diagnostics-open");
    assert_eq!(h.label("diagnostics"), "Diagnostics");
    assert_eq!(h.label("jobs-empty"), "No jobs.");
    h.click("diagnostics-tab-logs");
    let lines: Vec<String> = (0..200).filter_map(|i| h.try_label(crate::ui::forms::eid(format!("log:{i}")))).collect();
    assert!(lines.iter().any(|l| l.contains("Untitled 2.0.0")), "{lines:?}");
    h.click("diagnostics-close");
    assert!(!h.exists("diagnostics"));
    let state = h.state();
    assert!(state.runtime_only);
    assert_eq!(state.bundle_version.as_deref(), Some("2.0.0"));
}

// ---- Database login ------------------------------------------------------------

fn files_under(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = vec![];
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(files_under(&path));
            } else {
                out.push(path);
            }
        }
    }
    out
}

#[gpui_kit::test]
fn asks_for_a_database_login_when_a_shared_bundle_carries_none(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let stamp = uuid::Uuid::new_v4().simple().to_string();
    let datasource = json!({
        "kind": "postgres", "id": format!("pg-login-{stamp}"), "host": "127.0.0.1", "port": 1,
        "database": "orders", "user": "app_reader", "sslmode": "require",
        "credentialMode": "shared",
    });
    let mut config: Value =
        serde_json::to_value(crate::read_document_config(h.label.clone()).unwrap()).unwrap();
    config["datasource"] = datasource;
    crate::update_document_config(h.label.clone(), serde_json::from_value(config).unwrap()).unwrap();
    // The edit bypassed the UI; announce it so the shell sees the document as changed.
    h.database_changed();
    let bundle = bundle_path(&h, "pg-login.ixtr");
    let options = serde_json::from_value(json!({ "version": "1.0.0", "releaseNotes": "" })).unwrap();
    crate::bundle_export::export_runtime_bundle(h.label.clone(), bundle.to_string_lossy().into(), options)
        .unwrap();
    close_to_start_force(&mut h);

    open_bundle_from_start(&mut h, &bundle);
    let intro = h.label("database-login-intro");
    assert!(intro.contains("“orders” on 127.0.0.1"), "{intro}");
    h.fill("database-password", "typed-secret-1");
    assert_eq!(h.label("database-login-security"), "Connection security: sslmode require.");
    assert!(h.label("database-login-warning").contains("does not verify the database server"));
    // Connect stays disabled until the recipient accepts the unverified transport.
    h.click("database-connect");
    assert!(!h.exists("database-login-error"));
    let refused = crate::recordstore::runtime_login::set_runtime_datasource_login(
        h.label.clone(),
        "app_reader".into(),
        "typed-secret-1".into(),
        None,
    );
    assert!(refused.unwrap_err().code.contains("UNVERIFIED_TRANSPORT"));
    h.click("database-accept-unverified");
    h.click("database-connect");
    assert!(h.label("database-login-error").starts_with("Could not connect"));
    assert!(!h.exists("database-forget"));
    let status = serde_json::to_value(
        crate::recordstore::runtime_login::runtime_datasource_login_status(h.label.clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(status["applies"], true);
    assert_eq!(status["source"], "none");
    assert_eq!(status["needsLogin"], true);
    assert_eq!(status["reason"], "missing");
    assert_eq!(status["sslmode"], "require");
    assert_eq!(status["serverVerified"], false);
    assert!(!status.to_string().contains("typed-secret-1"));

    h.click("database-cancel");
    assert!(!h.exists("database-login"));
    h.click("database-login-open");
    assert!(h.exists("database-login"));
    crate::close_document(h.label.clone(), true).unwrap();
    for file in files_under(&crate::paths::state_dir().join("data")) {
        let bytes = std::fs::read(&file).unwrap_or_default();
        assert!(
            !bytes.windows(14).any(|w| w == b"typed-secret-1"),
            "{}",
            file.display()
        );
    }
}

fn close_to_start_force(h: &mut Harness) {
    h.dialogs.queue_confirm(true);
    h.click("close-project");
    assert!(h.exists("open-bundle"), "{:?} {:?}", h.error(), h.has_shell());
}

#[gpui_kit::test]
fn does_not_offer_a_database_login_for_a_sqlite_bundle_or_in_studio(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    let status = serde_json::to_value(
        crate::recordstore::runtime_login::runtime_datasource_login_status(h.label.clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(status["applies"], false);
    let stored = crate::recordstore::runtime_login::set_runtime_datasource_login(
        h.label.clone(),
        "x".into(),
        "y".into(),
        None,
    );
    assert!(stored.unwrap_err().code.contains("NOT_RUNTIME"));
    let _ = &mut h;
}

#[gpui_kit::test]
fn routes_a_forwarded_runtime_bundle_to_the_bundle_open_flow(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    open_release_tab(&mut h);
    let bundle = bundle_path(&h, "forwarded-1.0.0.ixtr");
    export_bundle(&mut h, "1.0.0", &bundle, None);
    let path = crate::paths::state_dir().join(format!("{}.ixt", uuid::Uuid::new_v4().simple()));
    h.dialogs.queue_save(Some(path));
    h.click("save-project");

    // A bundle forwarded while a document is open closes it and runs the bundle flow.
    let root = h.root.clone();
    let forwarded = bundle.to_string_lossy().into_owned();
    h.cx.update(|cx| root.update(cx, |view, cx| view.open_request(forwarded, cx)));
    h.settle();
    assert!(h.exists("runtime-bundle"), "{:?}", h.error());
    assert_eq!(h.label("runtime-version"), "Version 1.0.0");
    assert_eq!(h.dialogs.count("confirm"), 0);
}

/// `(user, password, host, port, database)` of a `postgresql://` URL.
fn parse_postgres_url(url: &str) -> (String, String, String, u16, String) {
    fn decode(text: &str) -> String {
        let bytes = text.as_bytes();
        let mut out = vec![];
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                if let Ok(v) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let (auth, rest) = rest.rsplit_once('@').expect("credentials in the URL");
    let (user, password) = auth.split_once(':').unwrap_or((auth, ""));
    let (hostport, path) = rest.split_once('/').expect("a database in the URL");
    let (host, port) = hostport.split_once(':').unwrap_or((hostport, "5432"));
    let database = path.split('?').next().unwrap_or("");
    (decode(user), decode(password), host.into(), port.parse().unwrap_or(5432), database.into())
}

#[gpui_kit::test]
#[ignore = "needs IXTABLE_TEST_POSTGRES_URL"]
fn verifies_stores_per_installation_and_forgets_a_recipients_postgres_login(
    cx: &mut TestAppContext,
) {
    let Some(url) = crate::test_env::postgres_url() else {
        return;
    };
    let (login, secret, host, port, database) = parse_postgres_url(&url);
    let mut h = Harness::new_document(cx);
    let stamp = uuid::Uuid::new_v4().simple().to_string();
    let table = format!("ixt_login_{stamp}");
    let datasource = json!({
        "kind": "postgres", "id": format!("pg-real-{stamp}"), "host": host, "port": port,
        "database": database, "user": login, "sslmode": "disable",
        "insecureTransportConfirmed": true,
        "insecureTransportConfirmedAt": chrono::Utc::now().to_rfc3339(),
        "credentialMode": "shared",
    });
    let password_ref = crate::recordstore::commands::set_datasource_password(
        h.label.clone(),
        format!("pg-real-{stamp}"),
        secret.clone(),
        Some(serde_json::from_value(datasource.clone()).unwrap()),
    )
    .unwrap();
    let mut config: Value =
        serde_json::to_value(crate::read_document_config(h.label.clone()).unwrap()).unwrap();
    let mut with_ref = datasource;
    with_ref["passwordRef"] = json!(password_ref);
    config["datasource"] = with_ref;
    crate::update_document_config(h.label.clone(), serde_json::from_value(config).unwrap()).unwrap();
    let spec = json!({
        "name": table,
        "columns": [{ "name": "id", "logicalType": "integer", "nullable": false, "primaryKeyPosition": 1 }],
    });
    crate::create_database_table(h.label.clone(), serde_json::from_value(spec).unwrap()).unwrap();
    crate::recordstore::commands::insert_row(h.label.clone(), table.clone(), vec![], None).unwrap();
    let bundle = bundle_path(&h, "pg-real.ixtr");
    let options = serde_json::from_value(json!({ "version": "1.0.0", "releaseNotes": "" })).unwrap();
    crate::bundle_export::export_runtime_bundle(h.label.clone(), bundle.to_string_lossy().into(), options)
        .unwrap();
    crate::recordstore::commands::clear_datasource_password(h.label.clone(), password_ref).unwrap();
    h.database_changed();
    close_to_start_force(&mut h);
    open_bundle_from_start(&mut h, &bundle);

    assert_eq!(h.label("database-login-security"), "Connection security: sslmode disable.");
    h.click("database-accept-unverified");
    h.fill("database-password", "wrong-password-9");
    h.click("database-connect");
    assert!(h
        .label("database-login-error")
        .contains("The database refused this login"));
    assert!(!h.label("database-login-error").contains("wrong-password-9"));

    h.fill("database-password", &secret);
    h.click("database-connect");
    assert_eq!(
        h.notice().unwrap(),
        format!("Connected to the database as {login}. The login is saved on this computer.")
    );
    assert!(!h.exists("database-login"));
    assert_eq!(h.read_page(&table).rows.len(), 1);
    let status = |h: &Harness| {
        serde_json::to_value(
            crate::recordstore::runtime_login::runtime_datasource_login_status(h.label.clone())
                .unwrap(),
        )
        .unwrap()
    };
    let after = status(&h);
    assert_eq!(after["attached"], true);
    assert_eq!(after["source"], "installation");
    assert_eq!(after["user"], login.as_str());
    assert_eq!(after["needsLogin"], false);
    let needle = format!("password={secret}");
    for file in files_under(&crate::paths::state_dir().join("data")) {
        let bytes = std::fs::read(&file).unwrap_or_default();
        assert!(
            !bytes.windows(needle.len()).any(|w| w == needle.as_bytes()),
            "{}",
            file.display()
        );
    }

    h.click("database-login-open");
    assert_eq!(h.label("database-login-connected"), format!("Connected as {login}."));
    h.click("database-forget");
    assert_eq!(
        h.notice().unwrap(),
        "The saved database login was removed from this computer."
    );
    let forgotten = status(&h);
    assert_eq!(forgotten["attached"], false);
    assert_eq!(forgotten["source"], "none");
    assert_eq!(forgotten["needsLogin"], true);
    assert!(h.exists("database-login"));
}
