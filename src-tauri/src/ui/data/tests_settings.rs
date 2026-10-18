//! Object browser and settings tabs owned by Data mode (ports of
//! tests/integration/object-browser.test.tsx and datasource.test.tsx). Imports are
//! explicit: `gpui_kit::*` would shadow `#[test]`.

use super::testkit::{choose, make_table};
use crate::ui::{shell::ModeId, testing::Harness};
use gpui_kit::TestAppContext;
use serde_json::json;
use std::time::Duration;

// Opens the settings tab `key` the way a user does.
fn open_tab(h: &mut Harness, key: &str) {
    h.switch_mode(ModeId::App);
    h.click(format!("tab-{key}"));
    assert!(h.exists("tab-title"), "{key} tab is shown");
}

// Settles until `id` exists (blocking backend calls finish on background threads).
fn wait_for(h: &mut Harness, id: &str) {
    for _ in 0..400 {
        if h.exists(id.to_string()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
        h.settle();
    }
    panic!("{id} never appeared");
}

// Whether the Datasource tab blocks saving and testing (the buttons are disabled).
fn blocked(h: &mut Harness) -> bool {
    let view = h.read_shell(|s, _| s.mode_view()).expect("settings view");
    let settings: gpui_kit::Entity<crate::ui::shell::settings::AppSettings> = view.downcast().expect("AppSettings");
    let tab = h.cx.read(|cx| settings.read(cx).tab_view()).expect("tab");
    let tab: gpui_kit::Entity<crate::ui::schema::datasource::DatasourceTab> = tab.downcast().expect("DatasourceTab");
    h.cx.read(|cx| tab.read(cx).blocked())
}

// Splits postgresql://user:pass@host:port/db?query into its parts.
fn parse_url(url: &str) -> (String, String, String, String, String) {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let (auth, rest) = rest.rsplit_once('@').unwrap_or(("", rest));
    let (user, password) = auth.split_once(':').unwrap_or((auth, ""));
    let (hostport, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, port) = hostport.split_once(':').unwrap_or((hostport, "5432"));
    let database = path.split('?').next().unwrap_or_default();
    (user.into(), password.into(), host.into(), port.into(), database.into())
}

#[gpui_kit::test]
fn shows_a_flat_object_list_and_expands_the_relationship_browser_until_selection(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    assert!(h.exists("new-table"), "the Tables group is listed");
    assert!(!h.exists("new-view"), "there is no Views group");
    assert!(h.exists("relationship-title"));
    assert!(!h.exists("data-pane"), "the relationship browser fills the workbench");

    make_table(
        &h,
        "People",
        json!([{ "name": "id", "declaredType": "INTEGER", "nullable": false, "primaryKeyPosition": 1 }]),
        json!([]),
    );
    h.database_changed();
    assert!(h.exists("table:People"));
    assert!(!h.exists("data-pane"));

    h.click("table:People");
    assert!(h.exists("data-pane"), "selecting a table opens the data pane");
}

#[gpui_kit::test]
fn requires_an_explicit_override_for_non_tls_postgres_and_warns_about_shared_credentials(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    open_tab(&mut h, "datasource");
    choose(&mut h, "Record store", "PostgreSQL");
    assert!(h.label("shared-credentials-note").contains("Shared credentials reduce revocation"));

    choose(&mut h, "TLS (sslmode)", "disable (no TLS)");
    assert!(h.label("plaintext-warning").contains("Severe security warning"));
    assert!(blocked(&mut h));

    h.click("accept-plaintext");
    assert!(!blocked(&mut h));

    choose(&mut h, "Credential mode", "A least-privileged credential per runtime user");
    assert!(!h.exists("shared-credentials-note"));

    h.click("capabilities-toggle");
    assert!(h.label("ddl:alter_column").contains("Requires table rebuild"));
}

#[gpui_kit::test]
fn never_releases_a_stored_password_to_a_different_server_or_over_unconfirmed_plaintext(
    cx: &mut TestAppContext,
) {
    let h = Harness::new_document(cx);
    let good = json!({
        "kind": "postgres",
        "id": format!("ds-{}", h.label),
        "host": "db.internal.example",
        "port": 5432,
        "database": "app",
        "user": "app",
        "sslmode": "require",
    });
    let datasource = |value: &serde_json::Value| -> crate::recordstore::DatasourceConfig {
        serde_json::from_value(value.clone()).expect("datasource")
    };
    let password_ref = crate::recordstore::commands::set_datasource_password(
        h.label.clone(),
        good["id"].as_str().unwrap().into(),
        "victim-secret".into(),
        Some(datasource(&good)),
    )
    .expect("store password");
    let attempt = |patch: serde_json::Value| -> String {
        let mut value = good.clone();
        for (k, v) in patch.as_object().unwrap() {
            value[k] = v.clone();
        }
        value["passwordRef"] = json!(password_ref);
        match crate::recordstore::commands::test_datasource_connection(h.label.clone(), datasource(&value), None) {
            Ok(_) => "connected".into(),
            Err(e) => e.to_string(),
        }
    };
    let mismatch = attempt(json!({ "host": "evil.example" }));
    assert!(mismatch.contains("CREDENTIAL_TARGET_MISMATCH") && mismatch.contains("Re-enter the password"), "{mismatch}");
    assert!(attempt(json!({ "user": "postgres" })).contains("CREDENTIAL_TARGET_MISMATCH"));
    assert!(attempt(json!({ "sslmode": "disable" })).contains("INSECURE_TRANSPORT"));
    crate::recordstore::commands::clear_datasource_password(h.label.clone(), password_ref).expect("clear");
}

#[gpui_kit::test]
fn sets_a_concurrency_policy_per_table_and_flags_tables_without_one(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.create_table("orders", &[("id", "INTEGER", 1)]);

    let mut config = serde_json::to_value(h.config()).expect("config json");
    config["migrations"] = json!([{
        "id": "m-legacy", "name": "Legacy", "order": 1,
        "up": "CREATE TABLE legacy (id INTEGER PRIMARY KEY)",
    }]);
    crate::update_document_config(h.label.clone(), serde_json::from_value(config).expect("config"))
        .expect("update config");
    crate::migrations::commands::apply_migrations(h.label.clone()).expect("apply migrations");
    h.database_changed();

    let issues = crate::validation::validate_document(h.label.clone()).expect("validate");
    assert!(
        issues.iter().any(|i| i.object_id == "legacy" && i.message.contains("no concurrency policy")),
        "{issues:?}"
    );

    open_tab(&mut h, "entities");
    assert_eq!(h.label("entity-status:legacy"), "No policy");
    assert_eq!(h.label("Concurrency policy for orders"), "Optimistic: reject stale edits");
    choose(&mut h, "Concurrency policy for orders", "Last write wins");
    let entities = h.config().entities;
    assert_eq!(entities.len(), 1);
    assert_eq!((entities[0].table.as_str(), entities[0].concurrency.as_str()), ("orders", "lastWriteWins"));
}

#[gpui_kit::test]
fn switches_reads_and_writes_to_postgres_without_storing_the_password_in_the_document(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("IXTABLE_TEST_POSTGRES_URL") else {
        eprintln!("skipped: IXTABLE_TEST_POSTGRES_URL is not set");
        return;
    };
    let (user, password, host, port, database) = parse_url(&url);
    let secret = if password.is_empty() { "not-needed-with-trust-auth".to_string() } else { password };
    let mut h = Harness::new_document(cx);
    open_tab(&mut h, "datasource");
    choose(&mut h, "Record store", "PostgreSQL");
    h.fill("Host", &host);
    h.fill("Port", &port);
    h.fill("Database", &database);
    h.fill("User", &user);
    h.fill("Password", &secret);
    choose(&mut h, "TLS (sslmode)", "disable (no TLS)");
    h.click("accept-plaintext");
    h.click("test-connection");
    wait_for(&mut h, "connection-report");
    assert!(h.label("connection-report").contains("WITHOUT TLS"));
    h.click("save-datasource");
    wait_for(&mut h, "datasource-status");
    let status = h.label("datasource-status");
    assert!(status.contains("Reads now use the PostgreSQL store"), "{status}");

    let config = h.config();
    let reference = config.datasource.password_ref.clone().expect("password ref");
    assert!(reference.starts_with("datasource:"));
    assert!(!serde_json::to_string(&config).unwrap().contains(&secret));
    assert!(config.datasource.insecure_transport_confirmed);
    let yaml = crate::read_document_config_yaml(h.label.clone()).expect("yaml");
    assert!(!yaml.contains(&secret));
    let capabilities = crate::recordstore::commands::store_capabilities(h.label.clone()).expect("capabilities");
    assert_eq!(capabilities.store, "postgres");
}
