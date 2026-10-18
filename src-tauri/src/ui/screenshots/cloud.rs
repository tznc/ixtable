//! ixtable Cloud captures (`cloud-*`) against the scripted services of `ui::cloud::mock`
//! (no network): sign-in, the cloud apps list, linking and publishing from Studio,
//! versions and backups, and the cloud runtime bar.

use crate::cloud::CloudLink;
use crate::cloud::publish::{Preflight, SecuritySummary};
use crate::cloud::runtime_commands::GrantResult;
use crate::archive::DocumentConfig;
use crate::ui::cloud::{
    errors::CloudError,
    mock::{State, reply, user},
    session::{CloudRuntime, tests::info},
    tests::{local, member, size_report},
    types::{AppVersion, CloudApp, InstallationBackup, Organization},
};
use crate::ui::screenshot::context;
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use gpui_kit::HeadlessAppContext;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const TALL: (f32, f32) = (1280.0, 1300.0);
const WINDOW: (f32, f32) = (1280.0, 1000.0);

type H<'a> = Harness<'a, HeadlessAppContext>;

fn edit(h: &mut H<'_>, change: impl FnOnce(&mut DocumentConfig)) {
    let store = h.shell().read_with(h.cx, |s, _| s.store.clone());
    h.cx.update(|cx| {
        store.update(cx, |s, cx| s.update("screenshot edit", change, cx)).expect("edit definition");
    });
    h.settle();
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

fn preflight() -> Preflight {
    Preflight {
        size: size_report(false),
        issues: vec![],
        blockers: vec![],
        warnings: vec!["Role Sales has no permissions on Orders".into()],
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

/// A saved Studio document linked to `app-1`, signed in as its owner, Cloud tab open.
fn linked_studio<'a>(cx: &'a mut HeadlessAppContext, setup: impl FnOnce(&mut State)) -> (H<'a>, Arc<Mutex<State>>) {
    let state = scripted_headless(cx);
    {
        let mut s = state.lock().unwrap();
        s.session = Some(user("u1", "ada@example.test"));
        s.app = Some(app("u1"));
        s.preflight = Some(preflight());
        setup(&mut s);
    }
    let mut h = Harness::start_sized(cx, TALL);
    h.click("new-document");
    let path = h.temp_path("crm.ixt");
    crate::save_document_as(h.label.clone(), path.to_string_lossy().into()).unwrap();
    edit(&mut h, |c| {
        c.cloud = Some(CloudLink { app_id: "app-1".into(), org_id: "org-1".into(), head_version_id: None });
    });
    h.switch_mode(ModeId::App);
    h.click("tab-cloud");
    (h, state)
}

fn scripted_headless(cx: &mut HeadlessAppContext) -> Arc<Mutex<State>> {
    let (mock, state) = crate::ui::cloud::mock::Mock::new();
    cx.update(|cx| crate::ui::cloud::services::Services::install(mock, cx));
    state
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn cloud_sign_in_and_apps() {
    let mut cx = context();
    let state = scripted_headless(&mut cx);
    let mut h = Harness::start_sized(&mut cx, WINDOW);
    h.click("cloud-apps-signin");
    assert_eq!(h.label("cloud-signin"), "Sign in to ixtable Cloud");
    h.fill("cloud-email", "ada@example.test");
    h.fill("cloud-password", "correct-horse-battery");
    h.capture(
        "cloud-01-signin",
        &[
            "The Cloud apps card on the start screen shows the Sign in to ixtable Cloud form with email and password fields.",
            "The email is filled in and the password is masked.",
        ],
    );

    {
        let mut s = state.lock().unwrap();
        s.members = vec![member("app-1", "CRM", "Sales rep"), member("app-2", "Inventory", "Warehouse")];
        s.installed = vec![local("app-1", "1.0.0")];
    }
    h.click("cloud-signin-submit");
    assert_eq!(h.label("cloud-account"), "Signed in as ada@example.test");
    assert_eq!(h.label("cloud-app:app-1"), "Open cloud app CRM");
    assert_eq!(h.label("cloud-app:app-2"), "Install cloud app Inventory");
    h.capture(
        "cloud-02-apps-list",
        &[
            "The Cloud apps card says Signed in as ada@example.test with a sign-out button.",
            "CRM is offered as Open (installed 1.0.0, role Sales rep) and Inventory as Install (not installed).",
        ],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn cloud_studio_link_and_publish() {
    let mut cx = context();
    let state = scripted_headless(&mut cx);
    {
        let mut s = state.lock().unwrap();
        s.session = Some(user("u1", "ada@example.test"));
        s.orgs = vec![
            Organization { id: "org-1".into(), name: "Acme".into(), role: "owner".into() },
            Organization { id: "org-2".into(), name: "Other".into(), role: "admin".into() },
        ];
    }
    let mut h = Harness::start_sized(&mut cx, WINDOW);
    h.click("new-document");
    h.switch_mode(ModeId::App);
    h.click("tab-cloud");
    assert!(h.exists("link-org:org-1") && h.exists("link-org:org-2"));
    h.click("link-org:org-2");
    h.fill("link-name", "Sales app");
    h.capture(
        "cloud-03-link-organization",
        &[
            "The Cloud tab offers Acme and Other as organizations, with Other selected, and a name field holding Sales app.",
            "Create a cloud application and link-to-an-existing-application controls are visible.",
        ],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn cloud_studio_publish_versions_backups() {
    let mut cx = context();
    let (mut h, state) = linked_studio(&mut cx, |s| {
        s.versions = vec![version("v2", "1.1.0", "Fixes totals."), version("v1", "1.0.0", "")];
        s.size_report = Some(size_report(false));
    });
    assert_eq!(h.label("cloud-app"), "CRM");
    assert!(h.label("preflight-size").starts_with("Archive size 2.0 KB of 476.8 MB"));
    assert!(h.label("version:0").starts_with("1.1.0 · published · "));
    h.fill("publish-notes", "First release");
    h.capture(
        "cloud-04-publish-preflight",
        &[
            "The Cloud tab names the linked application CRM with its preflight: archive size, record store and migrations, and the warning.",
            "The publish form has a version field, release notes reading First release, and a Publish button.",
            "The versions list shows 1.1.0 with its note and 1.0.0, each with a restore button.",
        ],
    );

    reply(&state, "publish-checkpoint", vec![Ok(json!({ "version": { "id": "v3", "version": "1.0.0", "status": "published" } }))]);
    h.click("publish-submit");
    assert!(h.label("publish-notice").starts_with("Published version 1.0.0."));
    h.capture(
        "cloud-05-published",
        &["A notice reads Published version 1.0.0 and says runtime users receive it on their next sync."],
    );

    let conflict = || {
        let mut e = CloudError::new("VERSION_CONFLICT", "Head moved");
        e.details = Some(json!({ "headVersionId": "v-head" }));
        Err::<Value, _>(e)
    };
    reply(&state, "publish-checkpoint", vec![conflict()]);
    h.click("publish-submit");
    assert_eq!(h.label("publish-conflict"), "The published history moved on");
    h.capture(
        "cloud-06-publish-conflict",
        &["A dialog says The published history moved on and offers Overwrite, Fork and Cancel."],
    );
    h.click("conflict-cancel");

    state.lock().unwrap().size_report = Some(size_report(true));
    h.click("backup-start");
    assert_eq!(h.label("backup-size"), "Back up to ixtable Cloud?");
    assert!(h.label("backup-size-over-limit").starts_with("Over the cloud limit"));
    h.capture(
        "cloud-07-backup-size",
        &[
            "A dialog asks Back up to ixtable Cloud? and reports the archive is over the cloud limit.",
            "The largest entry photos/site.jpg is listed and Confirm cannot send the backup.",
        ],
    );
}

fn grant(hours: i64) -> Result<GrantResult, CloudError> {
    let at = chrono::Utc::now() + chrono::Duration::hours(hours);
    Ok(GrantResult { needed: true, expires_at: Some(at.to_rfc3339()), attached: false, error: None })
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn cloud_runtime_bar() {
    let mut cx = context();
    let state = scripted_headless(&mut cx);
    {
        let mut s = state.lock().unwrap();
        s.session = Some(user("u1", "ada@example.test"));
        s.app = Some(app("owner"));
        s.runtime_info = Some(Arc::new(|| {
            let mut i = info(Some("r1"), json!({ "navigation": [], "objects": [], "actions": [] }));
            i.app_id = "app-1".into();
            i.postgres = true;
            i
        }));
        s.grants = [grant(2)].into();
        s.size_report = Some(size_report(false));
        s.backups = vec![InstallationBackup {
            id: "b1".into(),
            installation_id: "i1".into(),
            archive_sha256: "a".repeat(64),
            archive_size: 2048,
            created_at: "2026-10-03T23:14:33Z".into(),
        }];
    }
    crate::new_document("cloud-seed-shot".into()).unwrap();
    let bundle = crate::paths::state_dir().join("ui-tests").join("cloud-seed-shot.ixtr");
    std::fs::create_dir_all(bundle.parent().unwrap()).unwrap();
    let options = serde_json::from_value(json!({ "version": "1.0.0", "releaseNotes": "" })).unwrap();
    crate::bundle_export::export_runtime_bundle("cloud-seed-shot".into(), bundle.to_string_lossy().into(), options).unwrap();
    crate::close_document("cloud-seed-shot".into(), true).unwrap();
    let mut h = Harness::start_sized(&mut cx, TALL);
    h.dialogs.queue_open(Some(bundle));
    h.click("open-bundle");
    let session = h.state().session_id;
    let make = state.lock().unwrap().runtime_info.clone().unwrap();
    h.cx.update(|cx| {
        CloudRuntime::global(cx).update(cx, |r, cx| r.set(&session, Arc::new(make()), cx));
    });
    h.settle();
    assert_eq!(h.label("cloud-runtime-name"), "Orders");
    assert!(h.label("cloud-grant").starts_with("Database access authorized until "));
    assert_eq!(h.label("backup:0").is_empty(), false);
    h.capture(
        "cloud-08-runtime-bar",
        &[
            "The runtime sidebar shows the cloud block: app Orders, Version 1.0.0, Role: Sales and the license line.",
            "A database access grant line says Database access authorized until a date, and the installation backups list one backup.",
        ],
    );
}
