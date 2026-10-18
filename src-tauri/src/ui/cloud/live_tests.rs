//! Cloud flows against a real local Supabase stack (ports of the live parts of
//! `tests/integration/cloud-auth.test.tsx` and `cloud-distribution.test.tsx`, with the helpers
//! of `cloud-helpers.ts`). Every test is ignored by default; run them with
//! `cargo test --lib -- --ignored cloud_live --test-threads=1` (they share one state directory) while the stack is up (`supabase start`, the
//! Edge Functions served) and `SUPABASE_URL` (default `http://127.0.0.1:54321`),
//! `SUPABASE_ANON_KEY` (default: the CLI's demo key) and `IXTABLE_CLOUD_PUBLIC_KEY_RAW` (the
//! signing key from `scripts/cloud/dev-secrets.mjs`) are set. `SUPABASE_SERVICE_ROLE_KEY`
//! defaults to the CLI's demo key. Only local stacks are accepted.

use super::services::Services;
use crate::ui::{shell::ModeId, testing::{Harness, UiContext}};
use gpui_kit::TestAppContext;
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::time::Duration;

const DEMO_ANON: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZS1kZW1vIiwicm9sZSI6ImFub24iLCJleHAiOjE5ODM4MTI5OTZ9.CRXP1A7WOeoJeXxjNni43kdQwgnWNReilDMblYTn_I0";
const DEMO_SERVICE: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZS1kZW1vIiwicm9sZSI6InNlcnZpY2Vfcm9sZSIsImV4cCI6MTk4MzgxMjk5Nn0.EGIM96RAZx35lJzdJsyH-qQwv8Hdp7fsn3W0YpN81IU";
const PASSWORD: &str = "correct-horse-battery";
const SALES_REP: &str = "01a100fd-deca-7a05-8f33-87d2d2ba675b";

/// The local stack and the helpers `cloud-helpers.ts` offered.
struct Stack {
    url: String,
    anon: String,
    service: String,
    http: Client,
}

impl Stack {
    /// Points the app at the stack named by the environment; panics when it is not local or up.
    fn connect() -> Self {
        let url = std::env::var("SUPABASE_URL").unwrap_or_else(|_| "http://127.0.0.1:54321".into()).trim_end_matches('/').to_string();
        assert!(
            url.contains("127.0.0.1") || url.contains("localhost"),
            "refusing non-local SUPABASE_URL {url}"
        );
        let anon = std::env::var("SUPABASE_ANON_KEY").unwrap_or_else(|_| DEMO_ANON.into());
        let service = std::env::var("SUPABASE_SERVICE_ROLE_KEY").unwrap_or_else(|_| DEMO_SERVICE.into());
        let key = std::env::var("IXTABLE_CLOUD_PUBLIC_KEY_RAW")
            .expect("IXTABLE_CLOUD_PUBLIC_KEY_RAW is not set (run scripts/cloud/dev-secrets.mjs)");
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = Client::builder().timeout(Duration::from_secs(60)).build().expect("http client");
        // The sealed session outlives a test (one state dir per process), so every test starts signed out.
        crate::cloud::commands::cloud_auth_storage_remove("main".into(), "sb-ixtable-auth".into()).expect("clear the stored session");
        // Installed apps persist too and are listed for any user, so start with none.
        let _ = std::fs::remove_dir_all(crate::cloud::install::cloud_root());
        let stack = Self { url, anon, service, http };
        let health = stack.http.get(format!("{}/functions/v1/health", stack.url)).send();
        assert!(
            health.as_ref().is_ok_and(|r| r.status().is_success()),
            "the local stack at {} is not reachable or healthy: {:?}",
            stack.url,
            health.map(|r| r.status())
        );
        std::env::set_var("IXTABLE_CLOUD_URL", &stack.url);
        std::env::set_var("IXTABLE_CLOUD_ANON_KEY", &stack.anon);
        std::env::set_var("IXTABLE_CLOUD_PUBLIC_KEY_RAW", &key);
        std::env::set_var("IXTABLE_CLOUD_PUBLIC_KEY", &key);
        std::env::set_var("IXTABLE_CLOUD_NO_BROWSER", "1");
        stack
    }

    fn unique_email(prefix: &str) -> String {
        format!("{prefix}-{}@example.test", uuid::Uuid::new_v4().simple())
    }

    /// A confirmed user created through the admin API; returns the user id.
    fn create_confirmed_user(&self, email: &str, password: &str) -> String {
        let reply: Value = self
            .http
            .post(format!("{}/auth/v1/admin/users", self.url))
            .header("apikey", &self.service)
            .bearer_auth(&self.service)
            .json(&json!({ "email": email, "password": password, "email_confirm": true }))
            .send()
            .expect("create user")
            .json()
            .expect("create user reply");
        reply["id"].as_str().unwrap_or_else(|| panic!("no user id in {reply}")).to_string()
    }

    /// An access token of a separate signed-in client (another person, outside the app).
    fn sign_in(&self, email: &str, password: &str) -> String {
        let reply: Value = self
            .http
            .post(format!("{}/auth/v1/token?grant_type=password", self.url))
            .header("apikey", &self.anon)
            .json(&json!({ "email": email, "password": password }))
            .send()
            .expect("sign in")
            .json()
            .expect("sign in reply");
        reply["access_token"].as_str().unwrap_or_else(|| panic!("no token in {reply}")).to_string()
    }

    /// Calls an Edge Function as the person holding `token`; Err carries the error code.
    fn call(&self, token: &str, function: &str, body: Value) -> Result<Value, String> {
        let reply = self
            .http
            .post(format!("{}/functions/v1/{function}", self.url))
            .header("apikey", &self.anon)
            .bearer_auth(token)
            .json(&body)
            .send()
            .expect("function call");
        let ok = reply.status().is_success();
        let body: Value = reply.json().unwrap_or(Value::Null);
        if ok {
            Ok(body)
        } else {
            Err(body["error"]["code"].as_str().unwrap_or("UNKNOWN").to_string())
        }
    }

    /// Entitlement for the app (fake billing provider).
    fn grant_subscription(&self, app_id: &str) {
        let period_end = (chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339();
        let reply = self
            .http
            .post(format!("{}/rest/v1/subscriptions?on_conflict=app_id", self.url))
            .header("apikey", &self.service)
            .bearer_auth(&self.service)
            .header("Prefer", "resolution=merge-duplicates")
            .json(&json!({ "app_id": app_id, "plan_id": "team", "provider": "fake", "status": "active", "current_period_end": period_end }))
            .send()
            .expect("grant subscription");
        assert!(reply.status().is_success(), "grant subscription: {}", reply.status());
    }
}

fn wait_for<C: UiContext>(h: &mut Harness<C>, what: &str, mut done: impl FnMut(&mut Harness<C>) -> bool) {
    for _ in 0..600 {
        h.settle();
        if done(h) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
        h.advance(Duration::from_millis(250));
    }
    panic!("timed out waiting for {what}");
}

fn live(cx: &mut TestAppContext) {
    cx.update(|cx| Services::install(std::sync::Arc::new(super::client::LiveServices::default()), cx));
}

fn open_cloud_tab<C: UiContext>(h: &mut Harness<C>) {
    h.switch_mode(ModeId::App);
    h.click("tab-cloud");
}

fn account<C: UiContext>(h: &mut Harness<C>) -> Option<String> {
    h.try_label("cloud-account")
}

fn sign_in_ui<C: UiContext>(h: &mut Harness<C>, email: &str) {
    h.fill("cloud-email", email);
    h.fill("cloud-password", PASSWORD);
    h.click("cloud-signin-submit");
    let want = format!("Signed in as {email}");
    wait_for(h, "the account line", |h| account(h).as_deref() == Some(want.as_str()));
}

fn close_to_start<C: UiContext>(h: &mut Harness<C>) {
    h.dialogs.queue_confirm(true);
    h.click("close-project");
    // The card is only a scope; its heading `cloud-apps` is the observed element.
    wait_for(h, "the start screen", |h| h.exists("cloud-apps"));
}

fn switch_account<C: UiContext>(h: &mut Harness<C>, email: &str) {
    if h.exists("cloud-signout") {
        h.click("cloud-signout");
    }
    wait_for(h, "the sign-in button", |h| h.exists("cloud-apps-signin"));
    h.click("cloud-apps-signin");
    sign_in_ui(h, email);
}

fn publish<C: UiContext>(h: &mut Harness<C>, version: &str) {
    wait_for(h, "the publish card", |h| h.exists("cloud-publish"));
    h.fill("publish-version", version);
    h.click("publish-submit");
    let want = format!("Published version {version}");
    wait_for(h, "the publish notice", |h| h.try_label("publish-notice").is_some_and(|t| t.contains(&want)));
}

#[gpui_kit::test]
#[ignore = "needs a local Supabase stack"]
fn cloud_live_signs_up_in_the_cloud_tab_and_keeps_the_session_only_in_the_secret_store(cx: &mut TestAppContext) {
    let stack = Stack::connect();
    let email = Stack::unique_email("auth");
    live(cx);
    let mut h = Harness::new_document(cx);
    open_cloud_tab(&mut h);
    h.fill("cloud-email", &email);
    h.fill("cloud-password", PASSWORD);
    h.click("cloud-signup");
    let want = format!("Signed in as {email}");
    wait_for(&mut h, "the account line", |h| account(h).as_deref() == Some(want.as_str()));
    wait_for(&mut h, "the create-application card", |h| h.exists("cloud-link"));

    let stored = crate::cloud::commands::cloud_auth_storage_get("main".into(), "sb-ixtable-auth".into()).expect("storage");
    assert!(stored.is_some_and(|s| s.contains("refresh_token")));
    let refused = crate::cloud::commands::cloud_auth_storage_get("main".into(), "datasource:x".into());
    assert!(refused.is_err_and(|e| e.code.contains("VALIDATION")));

    close_to_start(&mut h);
    wait_for(&mut h, "the account on the start screen", |h| account(h).as_deref() == Some(want.as_str()));
    wait_for(&mut h, "the empty application list", |h| h.exists("cloud-apps-empty"));
    assert_eq!(h.label("cloud-apps-empty"), "No applications have been shared with you yet.");
    h.click("cloud-signout");
    wait_for(&mut h, "the sign-in button", |h| h.exists("cloud-apps-signin"));
    let gone = crate::cloud::commands::cloud_auth_storage_get("main".into(), "sb-ixtable-auth".into()).expect("storage");
    assert!(gone.is_none());
    drop(stack);
}

#[gpui_kit::test]
#[ignore = "needs a local Supabase stack"]
fn cloud_live_signs_in_through_the_browser_hand_off_with_pkce(cx: &mut TestAppContext) {
    let stack = Stack::connect();
    let email = Stack::unique_email("browser");
    stack.create_confirmed_user(&email, PASSWORD);
    let website = stack.sign_in(&email, PASSWORD);
    live(cx);
    let mut h = Harness::start(cx);
    h.click("cloud-apps-signin");
    h.click("cloud-google");
    wait_for(&mut h, "the hand-off address", |h| h.exists("cloud-handoff-url"));
    let address = h.label("cloud-handoff-url");
    let query = address.split_once('?').map(|(_, q)| q.to_string()).unwrap_or_default();
    let param = |name: &str| {
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
            .map(str::to_string)
    };
    assert_eq!(param("code_challenge_method").as_deref(), Some("S256"));
    assert_eq!(param("provider").as_deref(), Some("google"));
    stack
        .call(&website, "desktop-auth-approve", json!({ "codeChallenge": param("code_challenge"), "state": param("state") }))
        .expect("approve the sign-in");
    let want = format!("Signed in as {email}");
    wait_for(&mut h, "the approved session", |h| account(h).as_deref() == Some(want.as_str()));
    let stored = crate::cloud::commands::cloud_auth_storage_get("main".into(), "sb-ixtable-auth".into()).expect("storage");
    assert!(stored.is_some_and(|s| s.contains("refresh_token")));
    h.click("cloud-signout");
    wait_for(&mut h, "the sign-in button", |h| h.exists("cloud-apps-signin"));
}

#[gpui_kit::test]
#[ignore = "needs a local Supabase stack"]
fn cloud_live_publishes_installs_with_the_assigned_role_updates_in_place_and_revokes(cx: &mut TestAppContext) {
    let stack = Stack::connect();
    let dev_email = Stack::unique_email("developer");
    let runtime_email = Stack::unique_email("runtime");
    let runtime_id = stack.create_confirmed_user(&runtime_email, PASSWORD);
    live(cx);
    // Tall enough that the whole cloud panel (publish card included) is on screen.
    let mut h = Harness::start_sized(cx, (1280.0, 2800.0));
    h.click("template:crm");
    assert!(h.has_shell());
    open_cloud_tab(&mut h);
    h.fill("cloud-email", &dev_email);
    h.fill("cloud-password", PASSWORD);
    h.click("cloud-signup");
    let want = format!("Signed in as {dev_email}");
    wait_for(&mut h, "the developer account", |h| account(h).as_deref() == Some(want.as_str()));
    wait_for(&mut h, "the create-application card", |h| h.exists("link-org-name"));
    h.fill("link-org-name", "Acme");
    h.click("link-create");
    wait_for(&mut h, "the publish card", |h| h.exists("cloud-publish"));
    let app_id = h.config().cloud.expect("linked").app_id;
    stack.grant_subscription(&app_id);

    let archive = h.temp_path("crm-cloud.ixt");
    // Publishing needs the file on disk, so save the untitled document first.
    h.dialogs.queue_save(Some(archive));
    h.click("save-project");
    wait_for(&mut h, "the saved document", |h| h.try_label("document-status").as_deref() == Some("Saved"));
    publish(&mut h, "1.0.0");
    wait_for(&mut h, "the version history", |h| h.try_label("version:0").is_some_and(|t| t.contains("1.0.0")));

    let developer = stack.sign_in(&dev_email, PASSWORD);
    let invite = stack
        .call(&developer, "invitations-create", json!({ "kind": "app", "appId": app_id, "email": runtime_email, "roleId": SALES_REP }))
        .expect("invite");
    let accept_url = invite["acceptUrl"].as_str().expect("acceptUrl");
    let token = accept_url.split("token=").nth(1).expect("token").split('&').next().unwrap().to_string();
    let runtime = stack.sign_in(&runtime_email, PASSWORD);
    stack.call(&runtime, "invitations-accept", json!({ "token": token })).expect("accept");

    close_to_start(&mut h);
    switch_account(&mut h, &runtime_email);
    let app = format!("cloud-app:{app_id}");
    wait_for(&mut h, "the shared app", |h| h.try_label(app.clone()).as_deref() == Some("Install cloud app CRM"));
    h.click(app.clone());
    wait_for(&mut h, "the installed app", |h| h.has_shell() && h.exists("cloud-license"));
    assert!(h.label("cloud-license").contains(&format!("Licensed to {runtime_email}")));
    let config = h.config();
    let nav_has = |label: &str| config.design.navigation.iter().any(|n| n.label == label);
    assert!(nav_has("Deals"));
    assert!(!h.exists("run-as"), "no role preview in a cloud runtime");
    assert!(h.label("cloud-runtime-role").contains("Role: Sales rep"));
    assert!(!h.exists("undo") && !h.exists("save-project"));
    let name = |v: &str| crate::data::NamedValue { column: "name".into(), value: crate::data::DataValue::Text(v.into()) };
    crate::recordstore::commands::insert_row(h.label.clone(), "companies".into(), vec![name("Runtime-only Ltd")], None).expect("write allowed");
    let refused = crate::recordstore::commands::insert_row(h.label.clone(), "deal_stages".into(), vec![name("Hacked")], None);
    assert_eq!(refused.map(|_| ()).unwrap_err().code, "FORBIDDEN");
    let sql = crate::execute_read_query(h.label.clone(), "SELECT * FROM deal_stages".into());
    assert_eq!(sql.map(|_| ()).unwrap_err().code, "FORBIDDEN");
    let names = |h: &Harness| -> Vec<String> { h.read_page("companies").rows.iter().map(|r| crate::ui::testing::shown(&r[1])).collect() };
    assert!(names(&h).contains(&"Runtime-only Ltd".to_string()));

    close_to_start(&mut h);
    switch_account(&mut h, &dev_email);
    h.click("recent:crm-cloud.ixt");
    wait_for(&mut h, "the reopened document", |h| h.has_shell());
    open_cloud_tab(&mut h);
    publish(&mut h, "1.1.0");

    close_to_start(&mut h);
    switch_account(&mut h, &runtime_email);
    wait_for(&mut h, "the open button", |h| h.try_label(app.clone()).as_deref() == Some("Open cloud app CRM"));
    h.click(app.clone());
    wait_for(&mut h, "version 1.1.0", |h| h.try_label("cloud-runtime-version").as_deref() == Some("Version 1.1.0"));
    assert!(names(&h).contains(&"Runtime-only Ltd".to_string()));

    let installed = crate::cloud::runtime_commands::cloud_installed_apps(h.label.clone()).expect("installed apps");
    let installation_id = installed.first().expect("an installation").installation_id.clone();
    stack
        .call(&developer, "members-update", json!({ "appId": app_id, "userId": runtime_id, "status": "revoked" }))
        .expect("revoke");
    let refused = stack.call(&runtime, "key-grant", json!({ "appId": app_id, "installationId": installation_id, "datasourceId": "primary" }));
    assert!(matches!(refused.as_ref().err().map(String::as_str), Some("REVOKED" | "FORBIDDEN")), "{refused:?}");
    close_to_start(&mut h);
    h.click(app);
    wait_for(&mut h, "the revoked notice", |h| {
        h.try_label("cloud-apps-error").is_some_and(|t| t.contains("REVOKED") || t.contains("FORBIDDEN") || t.to_lowercase().contains("revoked"))
    });
}
