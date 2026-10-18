//! Studio's Cloud tab and the cloud runtime bar against scripted services (UI paths of
//! tests/integration/cloud-distribution.test.tsx and tests/unit/cloud-recipient.test.tsx).

use super::{
    errors::CloudError,
    mock::{State, reply, user},
    session::{CloudRuntime, tests::info},
    tests::{local, scripted, size_report},
    types::{AppVersion, CloudApp, InstallationBackup, Organization},
};
use crate::cloud::runtime_commands::GrantResult;
use crate::{
    archive::DocumentConfig,
    cloud::{
        CloudLink,
        publish::{Preflight, SecuritySummary},
    },
    roles::Role,
    ui::{
        release::gate::set_expression_gate,
        shell::ModeId,
        testing::Harness,
    },
};
use gpui_kit::{ScrollDelta, TestAppContext, point, px, test::TestWindowExt as _};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

// ---- helpers

fn scroll_to_start(h: &mut Harness) {
    h.update(|w, cx| w.scroll("settings-panel", ScrollDelta::Pixels(point(px(0.), px(100_000.))), cx));
    h.settle();
}

/// Scrolls the settings panel until `target` is on screen.
fn reveal(h: &mut Harness, target: &str) {
    for step in [-100_000., 300.] {
        if step < 0. {
            scroll_to_start(h);
        }
        for _ in 0..40 {
            if h.try_find(target.to_string()).is_some_and(|s| s.visible() && s.bounds().bottom() <= px(880.)) {
                return;
            }
            if step < 0. {
                break;
            }
            h.update(|w, cx| w.scroll("settings-panel", ScrollDelta::Pixels(point(px(0.), px(-300.))), cx));
            h.settle();
        }
    }
}

fn tap(h: &mut Harness, target: &str) {
    reveal(h, target);
    h.click(target.to_string());
}

fn fill(h: &mut Harness, target: &str, text: &str) {
    reveal(h, target);
    h.fill(target.to_string(), text);
}

fn text(h: &mut Harness, target: &str) -> String {
    reveal(h, target);
    h.label(target.to_string())
}

/// Edits the definition through the store (as the tabs do).
fn edit(h: &mut Harness, change: impl FnOnce(&mut DocumentConfig)) {
    let store = h.shell().read_with(h.cx, |s, _| s.store.clone());
    h.cx.update(|cx| {
        store.update(cx, |s, cx| s.update("test edit", change, cx)).expect("edit definition");
    });
    h.settle();
}

fn open_cloud_tab(h: &mut Harness) {
    h.switch_mode(ModeId::App);
    h.click("tab-cloud");
}

fn app(owner: &str) -> CloudApp {
    CloudApp {
        id: "app-1".into(),
        org_id: "org-1".into(),
        owner_id: owner.into(),
        name: "CRM".into(),
        document_id: "doc".into(),
        backups_enabled: true,
        head_version_id: None,
    }
}

fn preflight(blockers: &[&str]) -> Preflight {
    Preflight {
        size: size_report(false),
        issues: vec![],
        blockers: blockers.iter().map(|b| b.to_string()).collect(),
        warnings: vec!["SEVERE: the credential is shared".into()],
        security: SecuritySummary {
            store: "sqlite".into(),
            credential_mode: None,
            tls: true,
            sslmode: None,
            insecure_override_confirmed: false,
            insecure_override_confirmed_at: None,
            shared_credential_warning: false,
            entity_policies_resolved: true,
        },
        migrations: vec![],
        unresolved_entities: vec![],
        version: "1.0.0".into(),
        release_notes: String::new(),
        min_runtime_version: None,
    }
}

fn published(id: &str, version: &str) -> Value {
    json!({ "version": { "id": id, "version": version, "status": "published" } })
}

/// A saved Studio document linked to `app-1`, signed in as its owner, Cloud tab open.
fn linked_studio(cx: &mut TestAppContext) -> (Harness<'_>, Arc<Mutex<State>>) {
    linked_studio_with(cx, |_| {})
}

/// `linked_studio`, with the services scripted further before the tab opens.
fn linked_studio_with(
    cx: &mut TestAppContext,
    setup: impl FnOnce(&mut State),
) -> (Harness<'_>, Arc<Mutex<State>>) {
    let state = scripted(cx);
    {
        let mut s = state.lock().unwrap();
        s.session = Some(user("u1", "ada@example.test"));
        s.app = Some(app("u1"));
        s.preflight = Some(preflight(&[]));
        setup(&mut s);
    }
    let mut h = Harness::new_document(cx);
    let path = h.temp_path("crm.ixt");
    crate::save_document_as(h.label.clone(), path.to_string_lossy().into()).unwrap();
    edit(&mut h, |c| {
        c.cloud = Some(CloudLink { app_id: "app-1".into(), org_id: "org-1".into(), head_version_id: None });
    });
    open_cloud_tab(&mut h);
    (h, state)
}

fn calls(state: &Mutex<State>, name: &str) -> Vec<Value> {
    state.lock().unwrap().calls.iter().filter(|(n, _)| n == name).map(|(_, b)| b.clone()).collect()
}

// ---- the tab

#[gpui_kit::test]
fn asks_for_a_sign_in_before_anything_else(cx: &mut TestAppContext) {
    let _state = scripted(cx);
    let mut h = Harness::new_document(cx);
    open_cloud_tab(&mut h);
    assert_eq!(h.label("cloud-signin"), "Sign in to ixtable Cloud");
    assert_eq!(
        h.label("cloud-purpose"),
        "Sign in to publish this application to its runtime users."
    );
    h.fill("cloud-email", "ada@example.test");
    h.fill("cloud-password", "correct-horse-battery");
    h.click("cloud-signup");
    assert_eq!(h.label("cloud-account"), "Signed in as ada@example.test");
    assert_eq!(h.label("cloud-link"), "Create a cloud application");
}

#[gpui_kit::test]
fn creates_a_cloud_application_in_a_new_organization_and_links_it(cx: &mut TestAppContext) {
    let state = scripted(cx);
    state.lock().unwrap().session = Some(user("u1", "ada@example.test"));
    reply(&state, "apps-create", vec![Ok(json!({ "app": { "id": "app-9", "name": "Untitled" } }))]);
    let mut h = Harness::new_document(cx);
    open_cloud_tab(&mut h);
    assert!(!h.exists("link-org:org-1"));
    h.fill("link-org-name", "Acme");
    h.click("link-create");
    assert_eq!(state.lock().unwrap().created_orgs, ["Acme"]);
    let body = &calls(&state, "apps-create")[0];
    assert_eq!(body["orgId"], "org-new");
    assert_eq!(body["name"], "Untitled");
    assert_eq!(body["datasourceKind"], "sqlite");
    assert_eq!(body["documentId"], h.state().document_id);
    // No roles yet, so none are synced.
    assert!(calls(&state, "roles-sync").is_empty());
    let link = h.config().cloud.expect("linked");
    assert_eq!((link.app_id.as_str(), link.org_id.as_str()), ("app-9", "org-new"));
    assert!(h.exists("roles-sync"));
}

#[gpui_kit::test]
fn creating_in_an_existing_organization_syncs_the_roles(cx: &mut TestAppContext) {
    let state = scripted(cx);
    {
        let mut s = state.lock().unwrap();
        s.session = Some(user("u1", "ada@example.test"));
        s.orgs = vec![
            Organization { id: "org-1".into(), name: "Acme".into(), role: "owner".into() },
            Organization { id: "org-2".into(), name: "Other".into(), role: "admin".into() },
            Organization { id: "org-3".into(), name: "Billing only".into(), role: "billing".into() },
        ];
    }
    reply(&state, "apps-create", vec![Ok(json!({ "app": { "id": "app-9", "name": "Sales" } }))]);
    reply(&state, "roles-sync", vec![Ok(json!({}))]);
    let mut h = Harness::new_document(cx);
    edit(&mut h, |c| {
        c.roles = vec![Role { id: "role-1".into(), name: "Sales".into(), permissions: Default::default() }];
    });
    open_cloud_tab(&mut h);
    // Only organizations the user manages are offered.
    assert!(h.exists("link-org:org-1") && h.exists("link-org:org-2"));
    assert!(!h.exists("link-org:org-3"));
    h.click("link-org:org-2");
    h.fill("link-name", "Sales app");
    h.click("link-create");
    let body = &calls(&state, "apps-create")[0];
    assert_eq!((body["orgId"].as_str(), body["name"].as_str()), (Some("org-2"), Some("Sales app")));
    let synced = &calls(&state, "roles-sync")[0];
    assert_eq!(synced["appId"], "app-9");
    assert_eq!(synced["roles"][0]["id"], "role-1");
    assert_eq!(h.config().cloud.expect("linked").org_id, "org-2");
}

#[gpui_kit::test]
fn an_organization_name_is_required_when_the_user_has_none(cx: &mut TestAppContext) {
    let state = scripted(cx);
    state.lock().unwrap().session = Some(user("u1", "ada@example.test"));
    let mut h = Harness::new_document(cx);
    open_cloud_tab(&mut h);
    h.click("link-create");
    assert!(h.label("link-error-body").contains("Name the new organization."));
    assert!(calls(&state, "apps-create").is_empty());
    assert!(h.config().cloud.is_none());
}

#[gpui_kit::test]
fn links_an_existing_application_only_to_its_owner(cx: &mut TestAppContext) {
    let state = scripted(cx);
    {
        let mut s = state.lock().unwrap();
        s.session = Some(user("u1", "ada@example.test"));
        s.app = Some(app("someone-else"));
    }
    let mut h = Harness::new_document(cx);
    open_cloud_tab(&mut h);
    h.fill("link-existing", "missing");
    h.click("link-existing-submit");
    assert!(h.label("link-error").contains("not found"));
    h.fill("link-existing", "app-1");
    h.click("link-existing-submit");
    assert!(h.label("link-error").contains("not allowed"));
    assert!(h.config().cloud.is_none());

    state.lock().unwrap().app = Some(app("u1"));
    h.click("link-existing-submit");
    assert_eq!(h.config().cloud.expect("linked").app_id, "app-1");
}

#[gpui_kit::test]
fn shows_the_application_and_warns_when_the_user_is_not_its_owner(cx: &mut TestAppContext) {
    let state = scripted(cx);
    {
        let mut s = state.lock().unwrap();
        s.session = Some(user("u1", "ada@example.test"));
        s.app = Some(app("someone-else"));
        s.preflight = Some(preflight(&[]));
    }
    let mut h = Harness::new_document(cx);
    edit(&mut h, |c| {
        c.cloud = Some(CloudLink { app_id: "app-1".into(), org_id: "org-1".into(), head_version_id: None });
    });
    open_cloud_tab(&mut h);
    assert_eq!(h.label("cloud-app"), "CRM");
    assert_eq!(
        h.label("cloud-app-id"),
        "Application id app-1 · you are not this application's owner"
    );
}

// ---- roles and credentials

#[gpui_kit::test]
fn syncs_the_roles_to_the_cloud(cx: &mut TestAppContext) {
    let (mut h, state) = linked_studio(cx);
    reply(&state, "roles-sync", vec![Ok(json!({}))]);
    edit(&mut h, |c| {
        c.roles = vec![Role { id: "role-1".into(), name: "Sales".into(), permissions: Default::default() }];
    });
    assert!(text(&mut h, "roles-count").starts_with("1 role defined"));
    tap(&mut h, "roles-sync");
    assert_eq!(text(&mut h, "roles-notice"), "Synced 1 role to ixtable Cloud.");
    let body = &calls(&state, "roles-sync")[0];
    assert_eq!(body["appId"], "app-1");
    assert_eq!(body["roles"][0]["name"], "Sales");
}

#[gpui_kit::test]
fn a_failed_role_sync_explains_why(cx: &mut TestAppContext) {
    let (mut h, state) = linked_studio(cx);
    reply(&state, "roles-sync", vec![Err(CloudError::new("FORBIDDEN", "Not your app"))]);
    tap(&mut h, "roles-sync");
    assert!(text(&mut h, "roles-error").contains("not allowed"));
}

#[gpui_kit::test]
fn uploads_a_shared_and_a_per_user_postgres_credential(cx: &mut TestAppContext) {
    let (mut h, state) = linked_studio(cx);
    assert!(!h.exists("cred-upload"), "credentials are PostgreSQL only");
    edit(&mut h, |c| {
        c.datasource.kind = "postgres".into();
        c.datasource.credential_mode = "shared".into();
    });
    assert!(text(&mut h, "cred-warning").contains("can extract the credential"));
    tap(&mut h, "cred-upload");
    assert_eq!(text(&mut h, "cred-notice"), "Shared credential encrypted and uploaded.");
    tap(&mut h, "cred-scope-user");
    tap(&mut h, "cred-upload");
    assert!(text(&mut h, "cred-error").contains("rejected"));
    fill(&mut h, "cred-user-id", "u-2");
    fill(&mut h, "cred-username", "alice_db");
    fill(&mut h, "cred-password", "alice-pw-77");
    tap(&mut h, "cred-upload");
    assert_eq!(text(&mut h, "cred-notice"), "Credential for that user encrypted and uploaded.");
    let log = state.lock().unwrap().log.clone();
    assert!(log.contains(&"credential:app-1:shared:::".to_string()), "{log:?}");
    assert!(log.contains(&"credential:app-1:user:u-2:alice-pw-77:alice_db".to_string()), "{log:?}");
}

// ---- publishing

#[gpui_kit::test]
fn publishes_a_checkpoint_saves_and_records_the_published_version(cx: &mut TestAppContext) {
    let (mut h, state) = linked_studio(cx);
    reply(&state, "publish-checkpoint", vec![Ok(published("v1", "1.0.0"))]);
    assert!(text(&mut h, "preflight-size").starts_with("Archive size 2.0 KB of 476.8 MB"));
    assert_eq!(text(&mut h, "security-store"), "Record store: Embedded SQLite");
    assert_eq!(text(&mut h, "security-migrations"), "Migrations: 0");
    fill(&mut h, "publish-notes", "First release");
    tap(&mut h, "publish-submit");
    assert_eq!(
        text(&mut h, "publish-notice"),
        "Published version 1.0.0. Runtime users receive it on their next sync."
    );
    let body = &calls(&state, "publish-checkpoint")[0];
    assert_eq!(body["appId"], "app-1");
    assert_eq!(body["uploadId"], "up-1");
    assert_eq!(body["version"], "1.0.0");
    assert_eq!(body["releaseNotes"], "First release");
    assert_eq!(body["minRuntimeVersion"], "0.0.0");
    assert_eq!(body["expectedHeadVersionId"], Value::Null);
    assert_eq!(body["security"]["store"], "sqlite");
    assert_eq!(body["security"]["concurrencyPoliciesResolved"], true);
    // The document records its publish base and is saved with it.
    let config = h.config();
    assert_eq!(config.cloud.expect("linked").head_version_id.as_deref(), Some("v1"));
    assert_eq!(config.release.notes, "First release");
    assert!(!h.state().dirty);
    assert!(state.lock().unwrap().log.contains(&"upload:version:app-1".to_string()));

    // The next publish builds on the version just published.
    reply(&state, "publish-checkpoint", vec![Ok(published("v2", "1.1.0"))]);
    fill(&mut h, "publish-version", "1.1.0");
    tap(&mut h, "publish-submit");
    assert!(text(&mut h, "publish-notice").starts_with("Published version 1.1.0."));
    assert_eq!(calls(&state, "publish-checkpoint")[1]["expectedHeadVersionId"], "v1");
}

#[gpui_kit::test]
fn refuses_a_version_that_is_not_semantic(cx: &mut TestAppContext) {
    let (mut h, state) = linked_studio(cx);
    fill(&mut h, "publish-version", "1.2");
    assert_eq!(text(&mut h, "publish-version-error"), "Enter a semantic version such as 1.2.0.");
    tap(&mut h, "publish-submit");
    assert!(calls(&state, "publish-checkpoint").is_empty());
    fill(&mut h, "publish-version", "1.2.0");
    assert!(!h.exists("publish-version-error"));
    fill(&mut h, "publish-min-runtime", "soon");
    assert_eq!(text(&mut h, "publish-min-error"), "Enter a semantic version or leave blank.");
    tap(&mut h, "publish-submit");
    assert!(calls(&state, "publish-checkpoint").is_empty());
}

#[gpui_kit::test]
fn blockers_from_the_preflight_stop_publishing(cx: &mut TestAppContext) {
    let (mut h, state) = linked_studio(cx);
    state.lock().unwrap().preflight = Some(preflight(&["The archive is over the cloud limit"]));
    tap(&mut h, "publish-recheck");
    assert_eq!(text(&mut h, "preflight-blocker:0"), "The archive is over the cloud limit");
    assert_eq!(text(&mut h, "preflight-blockers"), "Publishing is blocked");
    assert_eq!(text(&mut h, "preflight-warning:0"), "SEVERE: the credential is shared");
    assert_eq!(text(&mut h, "preflight-largest-heading"), "Largest files");
    tap(&mut h, "publish-submit");
    assert!(calls(&state, "publish-checkpoint").is_empty());
    // Fixing it and checking again lifts the block.
    state.lock().unwrap().preflight = Some(preflight(&[]));
    reply(&state, "publish-checkpoint", vec![Ok(published("v1", "1.0.0"))]);
    tap(&mut h, "publish-recheck");
    assert!(!h.exists("preflight-blockers"));
    tap(&mut h, "publish-submit");
    assert_eq!(calls(&state, "publish-checkpoint").len(), 1);
}

#[gpui_kit::test]
fn expression_errors_block_publishing_like_any_preflight_blocker(cx: &mut TestAppContext) {
    let (mut h, state) = linked_studio(cx);
    set_expression_gate(Some(|_| 2));
    h.database_changed();
    assert_eq!(
        text(&mut h, "preflight-blocker:0"),
        "2 expression errors must be fixed first (see Settings › Problems)"
    );
    tap(&mut h, "publish-submit");
    assert!(calls(&state, "publish-checkpoint").is_empty());
    set_expression_gate(None);
}

#[gpui_kit::test]
fn a_shared_postgres_credential_must_be_acknowledged_before_publishing(cx: &mut TestAppContext) {
    let (mut h, state) = linked_studio(cx);
    {
        let mut pre = preflight(&[]);
        pre.security.store = "postgres".into();
        pre.security.credential_mode = Some("shared".into());
        pre.security.tls = false;
        pre.security.sslmode = Some("prefer".into());
        pre.security.shared_credential_warning = true;
        pre.security.insecure_override_confirmed = true;
        state.lock().unwrap().preflight = Some(pre);
    }
    reply(&state, "publish-checkpoint", vec![Ok(published("v1", "1.0.0"))]);
    tap(&mut h, "publish-recheck");
    assert_eq!(text(&mut h, "security-credentials"), "Credentials: Shared");
    assert_eq!(text(&mut h, "security-tls"), "TLS: Not required (prefer), override confirmed");
    tap(&mut h, "publish-submit");
    assert!(calls(&state, "publish-checkpoint").is_empty());
    tap(&mut h, "publish-ack-shared");
    tap(&mut h, "publish-submit");
    assert!(calls(&state, "publish-checkpoint").is_empty(), "the TLS risk still needs accepting");
    tap(&mut h, "publish-ack-insecure");
    tap(&mut h, "publish-submit");
    let body = &calls(&state, "publish-checkpoint")[0];
    assert_eq!(body["security"]["sharedCredentialAcknowledged"], true);
    assert_eq!(body["security"]["insecureTransportConfirmed"], true);
}

#[gpui_kit::test]
fn an_unsaved_document_must_be_saved_before_publishing(cx: &mut TestAppContext) {
    let state = scripted(cx);
    {
        let mut s = state.lock().unwrap();
        s.session = Some(user("u1", "ada@example.test"));
        s.app = Some(app("u1"));
        s.preflight = Some(preflight(&[]));
    }
    let mut h = Harness::new_document(cx);
    edit(&mut h, |c| {
        c.cloud = Some(CloudLink { app_id: "app-1".into(), org_id: "org-1".into(), head_version_id: None });
    });
    open_cloud_tab(&mut h);
    tap(&mut h, "publish-submit");
    assert!(text(&mut h, "publish-error").starts_with("Save the document first"));
    assert!(!state.lock().unwrap().log.iter().any(|l| l.starts_with("upload:")));
}

#[gpui_kit::test]
fn a_moved_history_offers_overwrite_or_fork_and_cancel_changes_nothing(cx: &mut TestAppContext) {
    let (mut h, state) = linked_studio(cx);
    let conflict = || {
        let mut e = CloudError::new("VERSION_CONFLICT", "Head moved");
        e.details = Some(json!({ "headVersionId": "v-head" }));
        Err(e)
    };
    reply(&state, "publish-checkpoint", vec![conflict()]);
    tap(&mut h, "publish-submit");
    assert_eq!(text(&mut h, "publish-conflict"), "The published history moved on");
    tap(&mut h, "conflict-cancel");
    assert!(!h.exists("publish-conflict"));
    assert!(calls(&state, "versions-resolve").is_empty());
    assert_eq!(text(&mut h, "publish-error"), "Someone published a newer version. Reload to see it.");
    assert!(!h.exists("publish-error-details"), "the technical detail starts collapsed");
    tap(&mut h, "publish-error-details-toggle");
    assert_eq!(text(&mut h, "publish-error-details"), "Error code VERSION_CONFLICT\nHead moved");

    // Overwrite publishes this document as the newest version anyway.
    tap(&mut h, "publish-submit");
    reply(&state, "versions-resolve", vec![Ok(published("v3", "1.0.0"))]);
    tap(&mut h, "conflict-overwrite");
    let resolve = &calls(&state, "versions-resolve")[0];
    assert_eq!((resolve["action"].as_str(), resolve["fromVersionId"].as_str()), (Some("overwrite"), Some("v-head")));
    assert_eq!(resolve["version"], "1.0.0");
    assert!(text(&mut h, "publish-notice").starts_with("Published version 1.0.0."));
    assert_eq!(h.config().cloud.expect("linked").head_version_id.as_deref(), Some("v3"));
    assert!(!h.exists("publish-conflict"));
}

#[gpui_kit::test]
fn forking_links_the_document_to_a_new_application(cx: &mut TestAppContext) {
    let (mut h, state) = linked_studio(cx);
    let mut e = CloudError::new("VERSION_CONFLICT", "Head moved");
    e.details = Some(json!({ "headVersionId": "v-head" }));
    reply(&state, "publish-checkpoint", vec![Err(e)]);
    tap(&mut h, "publish-submit");
    reply(
        &state,
        "versions-resolve",
        vec![Ok(json!({
            "app": { "id": "app-fork", "name": "CRM (fork)" },
            "version": { "id": "v-fork", "version": "1.0.0", "status": "published" },
        }))],
    );
    tap(&mut h, "conflict-fork");
    assert_eq!(
        text(&mut h, "publish-notice"),
        "Forked into a new cloud application (CRM (fork))."
    );
    let link = h.config().cloud.expect("linked");
    assert_eq!(
        (link.app_id.as_str(), link.org_id.as_str(), link.head_version_id.as_deref()),
        ("app-fork", "org-1", Some("v-fork"))
    );
    assert_eq!(calls(&state, "versions-resolve")[0]["action"], "fork");
}

#[gpui_kit::test]
fn escape_closes_the_conflict_dialog(cx: &mut TestAppContext) {
    let (mut h, state) = linked_studio(cx);
    let mut e = CloudError::new("VERSION_CONFLICT", "Head moved");
    e.details = Some(json!({ "headVersionId": "v-head" }));
    reply(&state, "publish-checkpoint", vec![Err(e)]);
    tap(&mut h, "publish-submit");
    assert!(h.exists("publish-conflict"));
    h.press("escape");
    assert!(!h.exists("publish-conflict"));
}

// ---- versions and backups

fn version(id: &str, version: &str, notes: &str) -> AppVersion {
    AppVersion {
        id: id.into(),
        version: version.into(),
        created_at: "2026-10-03T23:14:33Z".into(),
        archive_sha256: "a".repeat(64),
        archive_size: 2048,
        release_notes: notes.into(),
        status: "published".into(),
        min_runtime_version: "0.0.0".into(),
        resolution: None,
    }
}

#[gpui_kit::test]
fn lists_published_versions_and_restores_one_to_a_new_local_copy(cx: &mut TestAppContext) {
    let (mut h, state) = linked_studio_with(cx, |s| {
        s.versions = vec![version("v2", "1.1.0", "Fixes totals."), version("v1", "1.0.0", "")];
    });
    reply(
        &state,
        "restore-url",
        vec![Ok(json!({ "signedUrl": "/object/sign/x", "sha256": "a".repeat(64), "size": 2048, "isPostgres": false }))],
    );
    let line = text(&mut h, "version:0");
    assert!(line.starts_with("1.1.0 · published · "), "{line}");
    assert!(line.ends_with("2.0 KB — Fixes totals."), "{line}");
    let before = h.state().document_id;
    tap(&mut h, "restore-version:1");
    let restore = &calls(&state, "restore-url")[0];
    assert_eq!((restore["appId"].as_str(), restore["versionId"].as_str()), (Some("app-1"), Some("v1")));
    assert!(state.lock().unwrap().log.contains(&"restore:/object/sign/x:v1".to_string()));
    // The restored copy opens in this window as a different document.
    assert_ne!(h.state().document_id, before);
}

#[gpui_kit::test]
fn restoring_a_postgres_app_warns_that_records_are_not_restored(cx: &mut TestAppContext) {
    let (mut h, state) = linked_studio_with(cx, |s| s.versions = vec![version("v1", "1.0.0", "")]);
    reply(
        &state,
        "restore-url",
        vec![Ok(json!({
            "signedUrl": "/object/sign/x", "sha256": "a".repeat(64), "size": 2048,
            "isPostgres": true, "warning": "External records are not restored.",
        }))],
    );
    tap(&mut h, "restore-version:0");
    assert_eq!(text(&mut h, "restore-warning"), "PostgreSQL records are not restored");
    assert_eq!(text(&mut h, "restore-warning-text"), "External records are not restored.");
    assert!(!state.lock().unwrap().log.iter().any(|l| l.starts_with("restore:")));
    tap(&mut h, "restore-warning-cancel");
    assert!(!h.exists("restore-warning"));
    assert!(!state.lock().unwrap().log.iter().any(|l| l.starts_with("restore:")));
    tap(&mut h, "restore-version:0");
    tap(&mut h, "restore-warning-confirm");
    assert!(state.lock().unwrap().log.contains(&"restore:/object/sign/x:v1".to_string()));
}

#[gpui_kit::test]
fn shows_the_backup_size_first_and_blocks_an_over_limit_archive(cx: &mut TestAppContext) {
    let (mut h, state) = linked_studio(cx);
    state.lock().unwrap().size_report = Some(size_report(true));
    tap(&mut h, "backup-start");
    assert_eq!(text(&mut h, "backup-size"), "Back up to ixtable Cloud?");
    assert!(text(&mut h, "backup-size-over-limit").starts_with("Over the cloud limit"));
    assert_eq!(text(&mut h, "backup-size-largest-heading"), "Largest files");
    assert_eq!(text(&mut h, "backup-size-largest:0"), "photos/site.jpg (assets) 512 B");
    // The listed files do not make up the total, so the rest is named.
    assert!(text(&mut h, "backup-size-largest-other").starts_with("Everything else"));
    tap(&mut h, "backup-confirm");
    assert!(!state.lock().unwrap().log.iter().any(|l| l.starts_with("upload:")));
    tap(&mut h, "backup-size-cancel");
    assert!(!h.exists("backup-size"));
}

#[gpui_kit::test]
fn uploads_and_commits_a_within_limit_developer_backup(cx: &mut TestAppContext) {
    let (mut h, state) = linked_studio(cx);
    state.lock().unwrap().size_report = Some(size_report(false));
    reply(&state, "backup-commit", vec![Ok(json!({ "backup": { "id": "b2" } }))]);
    tap(&mut h, "backup-start");
    assert!(text(&mut h, "backup-size-limit").starts_with("Within the"));
    tap(&mut h, "backup-confirm");
    assert_eq!(text(&mut h, "versions-notice"), "Backed up 2.0 KB to ixtable Cloud.");
    assert_eq!(calls(&state, "backup-commit")[0]["uploadId"], "up-1");
    assert!(state.lock().unwrap().log.contains(&"upload:backup:app-1".to_string()));
}

#[gpui_kit::test]
fn the_backup_button_is_offered_when_backups_are_enabled(cx: &mut TestAppContext) {
    let (mut h, _state) = linked_studio(cx);
    assert!(h.exists("backup-start"));
}

#[gpui_kit::test]
fn there_is_no_backup_button_when_the_application_has_backups_off(cx: &mut TestAppContext) {
    let (mut h, _state) = linked_studio_with(cx, |s| {
        s.app = Some(CloudApp { backups_enabled: false, ..app("u1") });
    });
    assert!(h.exists("cloud-versions"));
    assert!(!h.exists("backup-start"));
}

// ---- the runtime bar

/// A runtime-only window for the exported document, registered as a cloud installation.
/// `setup` scripts the services before the window opens (the bar asks for its key grant
/// as soon as it shows).
fn cloud_runtime_window(
    cx: &mut TestAppContext,
    postgres: bool,
    setup: impl FnOnce(&mut State),
) -> (Harness<'_>, Arc<Mutex<State>>) {
    let state = scripted(cx);
    {
        let mut s = state.lock().unwrap();
        s.session = Some(user("u1", "ada@example.test"));
        s.app = Some(app("owner"));
        s.runtime_info = Some(Arc::new(move || {
            let mut i = info(Some("r1"), json!({ "navigation": [], "objects": [], "actions": [] }));
            i.app_id = "app-1".into();
            i.postgres = postgres;
            i
        }));
        setup(&mut s);
    }
    // A bundle exported from a scratch document, opened from the start screen.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
    let seed = format!("cloud-seed-{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
    crate::new_document(seed.clone()).unwrap();
    let bundle = crate::paths::state_dir().join("ui-tests").join(format!("{seed}.ixtr"));
    std::fs::create_dir_all(bundle.parent().unwrap()).unwrap();
    let options = serde_json::from_value(json!({ "version": "1.0.0", "releaseNotes": "" })).unwrap();
    crate::bundle_export::export_runtime_bundle(seed.clone(), bundle.to_string_lossy().into(), options)
        .unwrap();
    crate::close_document(seed, true).unwrap();
    let mut h = Harness::start(cx);
    h.dialogs.queue_open(Some(bundle));
    h.click("open-bundle");
    // The cloud installation is registered for the session the bundle opened.
    let session = h.state().session_id;
    let make = state.lock().unwrap().runtime_info.clone().unwrap();
    h.cx.update(|cx| {
        CloudRuntime::global(cx).update(cx, |r, cx| r.set(&session, Arc::new(make()), cx));
    });
    h.settle();
    (h, state)
}

fn grant(expires_in_hours: i64) -> Result<GrantResult, CloudError> {
    let at = chrono::Utc::now() + chrono::Duration::hours(expires_in_hours);
    Ok(GrantResult { needed: true, expires_at: Some(at.to_rfc3339()), attached: false, error: None })
}

fn count(state: &Mutex<State>, entry: &str) -> usize {
    state.lock().unwrap().log.iter().filter(|l| *l == entry).count()
}

#[gpui_kit::test]
fn a_cloud_session_shows_the_cloud_block_instead_of_the_bundle_block(cx: &mut TestAppContext) {
    let (mut h, _state) = cloud_runtime_window(cx, false, |_| {});
    assert!(!h.exists("runtime-bundle"));
    assert_eq!(h.label("cloud-runtime-name"), "Orders");
    assert_eq!(h.label("cloud-runtime-version"), "Version 1.0.0");
    assert_eq!(h.label("cloud-runtime-role"), "Role: Sales");
    assert_eq!(h.label("cloud-license"), "Licensed to ada@example.com");
    assert!(!h.exists("cloud-fingerprint"));
    h.click("cloud-license-details-toggle");
    assert_eq!(h.label("cloud-fingerprint"), "Fingerprint f");
    // The bundle block would offer a manual update.
    assert!(!h.exists("check-update"));
    assert!(!h.exists("cloud-grant"), "SQLite installations have no key grant");
}

#[gpui_kit::test]
fn sync_now_reports_when_the_newest_version_is_already_installed(cx: &mut TestAppContext) {
    let (mut h, state) = cloud_runtime_window(cx, false, |s| s.installed = vec![local("app-1", "1.0.0")]);
    reply(&state, "sync-check", vec![Ok(json!({ "upToDate": true }))]);
    h.click("cloud-sync");
    // The synced installation opens as a new session; the window says what happened.
    assert_eq!(h.notice().as_deref(), Some("You have the newest published version."));
    assert!(state.lock().unwrap().log.contains(&"open_installed:app-1".to_string()));
}

#[gpui_kit::test]
fn a_postgres_installation_requests_a_key_grant_and_renews_it_before_it_expires(cx: &mut TestAppContext) {
    let (mut h, state) = cloud_runtime_window(cx, true, |s| s.grants = [grant(2), grant(25)].into());
    assert!(h.label("cloud-grant").starts_with("Database access authorized until "));
    assert_eq!(count(&state, "key_grant"), 1);
    // The grant is renewed an hour before it expires.
    h.advance(std::time::Duration::from_secs(30 * 60));
    assert_eq!(count(&state, "key_grant"), 1);
    h.advance(std::time::Duration::from_secs(31 * 60));
    assert_eq!(count(&state, "key_grant"), 2);
    assert!(h.label("cloud-grant").starts_with("Database access authorized until "));
}

#[gpui_kit::test]
fn a_revoked_user_loses_the_database_credential_and_is_told_so(cx: &mut TestAppContext) {
    let (mut h, state) = cloud_runtime_window(cx, true, |s| {
        s.grants = [grant(2), Err(CloudError::new("REVOKED", "Access revoked"))].into();
    });
    assert!(!h.exists("cloud-revoked"));
    h.advance(std::time::Duration::from_secs(61 * 60));
    assert_eq!(h.label("cloud-grant"), "Access revoked: the database credential was removed.");
    assert!(h.label("cloud-revoked").starts_with("Your access to this application was revoked"));
    assert_eq!(count(&state, "release_credentials"), 1);
    assert!(!h.exists("cloud-runtime-error"), "the revoked notice replaces the generic error");
}

fn backup() -> InstallationBackup {
    InstallationBackup {
        id: "b1".into(),
        installation_id: "i1".into(),
        archive_sha256: "a".repeat(64),
        archive_size: 2048,
        created_at: "2026-10-03T23:14:33Z".into(),
    }
}

#[gpui_kit::test]
fn the_installation_backup_shows_the_size_report_first_and_blocks_an_over_limit_backup(
    cx: &mut TestAppContext,
) {
    let (mut h, state) = cloud_runtime_window(cx, false, |s| s.size_report = Some(size_report(true)));
    h.click("backup-installation");
    assert_eq!(h.label("backup-size"), "Back up to ixtable Cloud?");
    assert!(h.label("backup-size-over-limit").starts_with("Over the cloud limit"));
    assert_eq!(h.label("backup-size-largest:0"), "photos/site.jpg (assets) 512 B");
    h.click("backup-confirm");
    assert!(!state.lock().unwrap().log.iter().any(|l| l.starts_with("upload:")));
}

#[gpui_kit::test]
fn uploads_and_commits_a_within_limit_installation_backup(cx: &mut TestAppContext) {
    let (mut h, state) = cloud_runtime_window(cx, false, |s| {
        s.size_report = Some(size_report(false));
        s.upload = Some(("u1".into(), 2048, Some("i1".into())));
    });
    reply(&state, "backup-commit", vec![Ok(json!({ "backup": { "id": "b2" } }))]);
    h.click("backup-installation");
    assert!(h.label("backup-size-limit").starts_with("Within the"));
    h.click("backup-confirm");
    assert_eq!(h.label("backups-notice"), "Backed up 2.0 KB of this installation.");
    assert_eq!(
        calls(&state, "backup-commit"),
        [json!({ "appId": "app-1", "uploadId": "u1", "installationId": "i1" })]
    );
    // The backup list is read again after the upload.
    assert_eq!(count(&state, "list_installation_backups"), 2);
}

#[gpui_kit::test]
fn restores_an_installation_backup_to_a_new_local_copy_after_the_postgres_warning(
    cx: &mut TestAppContext,
) {
    let (mut h, state) = cloud_runtime_window(cx, false, |s| s.backups = vec![backup()]);
    reply(
        &state,
        "restore-url",
        vec![Ok(json!({
            "signedUrl": "/object/sign/x", "sha256": "a".repeat(64), "size": 2048,
            "isPostgres": true, "warning": "External records are not restored.",
        }))],
    );
    let line = h.label("backup:0");
    assert!(line.ends_with(" · 2.0 KB"), "{line}");
    h.click("restore-backup:0");
    assert_eq!(calls(&state, "restore-url"), [json!({ "appId": "app-1", "backupId": "b1" })]);
    assert_eq!(
        h.label("restore-backup-note"),
        "PostgreSQL records are not restored. External records are not restored."
    );
    assert!(!state.lock().unwrap().log.iter().any(|l| l.starts_with("restore:")));
    let before = h.state().document_id;
    h.click("restore-backup-confirm");
    assert!(state.lock().unwrap().log.contains(&"restore:/object/sign/x:".to_string()));
    assert_ne!(h.state().document_id, before, "the copy opened as a new document");
}
