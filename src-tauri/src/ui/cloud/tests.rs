//! Cloud flows against scripted services (ports of the mocked parts of
//! tests/unit/cloud-recipient.test.tsx and the UI paths of cloud-distribution.test.tsx).
//! Imports are explicit: `gpui_kit::*` would shadow `#[test]`.

use super::{
    mock::{Mock, State, reply, user},
    open::{open_cloud_app, update_notice},
    services::Services,
    session::{CloudRuntime, tests::info},
    types::MemberApp,
};
use crate::{
    assets::ArchiveSizeReport,
    cloud::install::LocalInstallation,
    installation::InstalledBundle,
    ui::testing::Harness,
};
use gpui_kit::TestAppContext;
use serde_json::json;
use std::sync::{Arc, Mutex};

pub(super) fn scripted(cx: &mut TestAppContext) -> Arc<Mutex<State>> {
    let (mock, state) = Mock::new();
    cx.update(|cx| Services::install(mock, cx));
    state
}

pub(super) fn member(app_id: &str, name: &str, role: &str) -> MemberApp {
    MemberApp { app_id: app_id.into(), name: name.into(), role_id: "r1".into(), role_name: role.into() }
}

pub(super) fn local(app_id: &str, version: &str) -> LocalInstallation {
    LocalInstallation {
        app_id: app_id.into(),
        installation_id: "i1".into(),
        device_name: "laptop".into(),
        document_id: None,
        app_name: Some("CRM".into()),
        version: Some(version.into()),
        version_id: Some("v1".into()),
    }
}

pub(super) fn runtime_info(app_id: &str) -> Arc<dyn Fn() -> crate::cloud::runtime_commands::CloudRuntimeInfo + Send + Sync> {
    let app_id = app_id.to_string();
    Arc::new(move || {
        let mut i = info(Some("r1"), json!({ "navigation": [], "objects": [], "actions": [] }));
        i.app_id = app_id.clone();
        i
    })
}

pub(super) fn size_report(over: bool) -> ArchiveSizeReport {
    ArchiveSizeReport {
        total_bytes: if over { 600_000_000 } else { 2048 },
        records_bytes: 1024,
        config_bytes: 512,
        assets_bytes: 512,
        other_bytes: 0,
        asset_share: 0.25,
        largest: vec![crate::archive_io::SizeEntry {
            id: "a1".into(),
            name: "photos/site.jpg".into(),
            section: "assets".into(),
            bytes: 512,
        }],
        cloud_limit_bytes: 500_000_000,
        over_cloud_limit: over,
        measured: "snapshot".into(),
    }
}

// ---- sign-in and invitations (cloud-recipient "desktop auth")

#[gpui_kit::test]
fn forgot_password_sends_the_recovery_email_for_the_typed_address(cx: &mut TestAppContext) {
    let state = scripted(cx);
    let mut h = Harness::start(cx);
    h.click("cloud-apps-signin");
    h.click("cloud-forgot");
    assert!(h.label("cloud-signin-error").contains("Enter your email address"));
    assert!(state.lock().unwrap().resets.is_empty());
    h.fill("cloud-email", "ada@example.test");
    h.click("cloud-forgot");
    assert!(h.label("cloud-notice").contains("password reset email is on its way"));
    assert_eq!(state.lock().unwrap().resets, ["ada@example.test"]);
}

#[gpui_kit::test]
fn accepts_an_invitation_link_through_invitations_accept(cx: &mut TestAppContext) {
    let state = scripted(cx);
    state.lock().unwrap().session = Some(user("u1", "ada@example.test"));
    reply(
        &state,
        "invitations-accept",
        vec![Ok(json!({ "membership": { "kind": "app", "appId": "a", "orgId": "o" } }))],
    );
    let mut h = Harness::start(cx);
    assert_eq!(h.label("cloud-account"), "Signed in as ada@example.test");
    h.click("invite-open");
    h.fill("invite-link", "https://x.test/invite");
    h.click("invite-accept");
    assert!(h.label("invite-error").contains("Paste the invitation link"));
    assert!(state.lock().unwrap().calls.is_empty());
    h.fill("invite-link", "https://x.test/invite?token=abcdefghijklmnopqrstuvwxyz");
    h.click("invite-accept");
    assert!(h.label("invite-notice").contains("Invitation accepted"));
    let s = state.lock().unwrap();
    assert_eq!(s.calls, [("invitations-accept".to_string(), json!({ "token": "abcdefghijklmnopqrstuvwxyz" }))]);
    // Accepting reloads the application list.
    assert_eq!(s.log.iter().filter(|l| *l == "list_member_apps").count(), 2);
}

#[gpui_kit::test]
fn signs_in_and_out_and_lists_the_empty_state(cx: &mut TestAppContext) {
    let state = scripted(cx);
    let mut h = Harness::start(cx);
    h.click("cloud-apps-signin");
    h.fill("cloud-email", "ada@example.test");
    h.fill("cloud-password", "correct-horse-battery");
    h.click("cloud-signin-submit");
    assert_eq!(h.label("cloud-account"), "Signed in as ada@example.test");
    assert_eq!(h.label("cloud-apps-empty"), "No applications have been shared with you yet.");
    h.click("cloud-signout");
    assert!(h.exists("cloud-apps-signin"));
    assert_eq!(state.lock().unwrap().signed_out, 1);
    assert!(state.lock().unwrap().session.is_none());
}

#[gpui_kit::test]
fn a_short_password_is_refused_before_any_request(cx: &mut TestAppContext) {
    let state = scripted(cx);
    let mut h = Harness::start(cx);
    h.click("cloud-apps-signin");
    h.fill("cloud-email", "ada@example.test");
    h.fill("cloud-password", "short");
    h.click("cloud-signin-submit");
    assert!(h.label("cloud-signin-error").contains("at least 8 characters"));
    assert!(state.lock().unwrap().session.is_none());
}

#[gpui_kit::test]
fn sign_up_that_needs_email_confirmation_says_so(cx: &mut TestAppContext) {
    let state = scripted(cx);
    state.lock().unwrap().needs_confirmation = true;
    let mut h = Harness::start(cx);
    h.click("cloud-apps-signin");
    h.fill("cloud-email", "ada@example.test");
    h.fill("cloud-password", "correct-horse-battery");
    h.click("cloud-signup");
    assert!(h.label("cloud-notice").contains("Check your email to confirm"));
    assert!(!h.exists("cloud-account"));
}

#[gpui_kit::test]
fn browser_sign_in_shows_the_address_and_adopts_the_approved_session(cx: &mut TestAppContext) {
    let state = scripted(cx);
    let mut h = Harness::start(cx);
    h.click("cloud-apps-signin");
    h.click("cloud-google");
    assert!(h.label("cloud-handoff-url").contains("code_challenge="));
    assert!(state.lock().unwrap().log.contains(&"desktop_auth_start:google".to_string()));
    state.lock().unwrap().browser = Some(crate::cloud::commands::DesktopAuthPoll {
        status: "approved".into(),
        session: Some(json!({ "access_token": "a", "refresh_token": "r" })),
    });
    h.advance(std::time::Duration::from_secs(3));
    assert_eq!(h.label("cloud-account"), "Signed in as browser@example.test");
}

#[gpui_kit::test]
fn cancelling_the_browser_sign_in_stops_waiting(cx: &mut TestAppContext) {
    let _state = scripted(cx);
    let mut h = Harness::start(cx);
    h.click("cloud-apps-signin");
    h.click("cloud-microsoft");
    assert!(h.exists("cloud-handoff"));
    h.click("cloud-handoff-cancel");
    assert!(!h.exists("cloud-handoff"));
    assert!(!h.exists("cloud-busy"));
}

// ---- cloud apps on the start screen

#[gpui_kit::test]
fn installs_a_shared_app_and_opens_it(cx: &mut TestAppContext) {
    let state = scripted(cx);
    {
        let mut s = state.lock().unwrap();
        s.session = Some(user("u1", "ada@example.test"));
        s.members = vec![member("app-1", "CRM", "Sales rep")];
        s.runtime_info = Some(runtime_info("app-1"));
    }
    let mut h = Harness::start(cx);
    assert_eq!(h.label("cloud-app:app-1"), "Install cloud app CRM");
    assert_eq!(h.label("cloud-app-detail:app-1"), "Role: Sales rep · not installed");
    h.click("cloud-app:app-1");
    assert!(h.has_shell());
    assert!(state.lock().unwrap().log.contains(&"install:app-1".to_string()));
    assert_eq!(h.notice().as_deref(), Some("Installed from ixtable Cloud."));
    let active = h.cx.update(|cx| CloudRuntime::global(cx).read(cx).active().map(|a| a.info.app_id.clone()));
    assert_eq!(active.as_deref(), Some("app-1"));
}

#[gpui_kit::test]
fn an_installed_app_opens_offline_and_says_so(cx: &mut TestAppContext) {
    let state = scripted(cx);
    {
        let mut s = state.lock().unwrap();
        s.session = Some(user("u1", "ada@example.test"));
        s.members_error = Some(super::errors::CloudError::new("CLOUD_OFFLINE", "offline"));
        s.installed = vec![local("app-1", "1.0.0")];
        s.runtime_info = Some(runtime_info("app-1"));
    }
    reply(&state, "sync-check", vec![Err(super::errors::CloudError::new("CLOUD_OFFLINE", "offline"))]);
    let mut h = Harness::start(cx);
    assert!(h.exists("cloud-apps-offline"));
    assert_eq!(h.label("cloud-app:app-1"), "Open cloud app CRM");
    assert_eq!(h.label("cloud-app-detail:app-1"), "Runtime user · installed 1.0.0");
    h.click("cloud-app:app-1");
    assert!(h.has_shell());
    assert_eq!(h.notice().as_deref(), Some("Offline: running the installed version 1.0.0."));
    assert!(state.lock().unwrap().log.contains(&"open_installed:app-1".to_string()));
}

#[gpui_kit::test]
fn a_revoked_user_is_told_so_instead_of_opening(cx: &mut TestAppContext) {
    let state = scripted(cx);
    {
        let mut s = state.lock().unwrap();
        s.session = Some(user("u1", "ada@example.test"));
        s.members = vec![member("app-1", "CRM", "Sales rep")];
        s.installed = vec![local("app-1", "1.0.0")];
    }
    reply(
        &state,
        "sync-check",
        vec![Err(super::errors::CloudError::new("REVOKED", "Access revoked"))],
    );
    let mut h = Harness::start(cx);
    h.click("cloud-app:app-1");
    assert!(!h.has_shell());
    assert!(h.label("cloud-apps-error").starts_with("REVOKED"));
}

#[gpui_kit::test]
fn the_cloud_apps_section_starts_collapsed_after_a_sign_out(cx: &mut TestAppContext) {
    let state = scripted(cx);
    state.lock().unwrap().session = Some(user("u1", "ada@example.test"));
    let mut h = Harness::start(cx);
    h.click("cloud-signout");
    assert!(h.exists("cloud-apps-signin"));
    assert!(!h.exists("cloud-signin"));
}

// ---- opening an app (open.ts)

fn installed_bundle(last_action: &str, migrations: &[&str]) -> InstalledBundle {
    InstalledBundle {
        bundle_id: "b".into(),
        name: "CRM".into(),
        version: "2.0.0".into(),
        release_notes: String::new(),
        signer_public_key: String::new(),
        signer_fingerprint: String::new(),
        payload_sha256: String::new(),
        installed_at: String::new(),
        updated_at: String::new(),
        previous_version: None,
        applied_migrations: migrations.iter().map(|m| m.to_string()).collect(),
        last_action: Some(last_action.into()),
    }
}

#[test]
fn the_update_notice_names_the_version_the_migrations_and_the_release_notes() {
    let (mock, state) = Mock::new();
    {
        let mut s = state.lock().unwrap();
        s.release_notes = "Adds due dates.\nFixes totals.".into();
        s.installation = Some(installed_bundle("update", &["Add due dates"]));
    }
    assert_eq!(
        update_notice(mock.as_ref(), "w", Some("v2"), "2.0.0"),
        "Updated to version 2.0.0. Your records were kept. Migrations applied: Add due dates.\nRelease notes:\nAdds due dates.\nFixes totals."
    );
    assert!(state.lock().unwrap().log.contains(&"release_notes:v2".to_string()));
}

#[test]
fn the_update_notice_does_not_repeat_an_earlier_update() {
    let (mock, state) = Mock::new();
    {
        let mut s = state.lock().unwrap();
        s.release_notes = "Old notes.".into();
        s.installation = Some(installed_bundle("open", &[]));
    }
    assert_eq!(update_notice(mock.as_ref(), "w", Some("v2"), "2.0.0"), "Running version 2.0.0.");
}

#[test]
fn the_update_notice_still_reports_when_notes_and_migrations_cannot_be_read() {
    let (mock, _) = Mock::new();
    assert_eq!(
        update_notice(mock.as_ref(), "w", Some("v2"), "2.0.0"),
        "Updated to version 2.0.0. Your records were kept. No migrations were needed."
    );
}

#[test]
fn opening_an_app_updates_when_a_newer_version_is_published_and_falls_back_when_that_fails() {
    let (mock, state) = Mock::new();
    {
        let mut s = state.lock().unwrap();
        s.session = Some(user("u1", "ada@example.test"));
        s.installed = vec![local("app-1", "1.0.0")];
        s.runtime_info = Some(runtime_info("app-1"));
        s.installation = Some(installed_bundle("update", &[]));
    }
    reply(
        &state,
        "sync-check",
        vec![Ok(json!({ "upToDate": false, "latest": { "versionId": "v2", "version": "2.0.0", "minRuntimeVersion": null } }))],
    );
    let opened = open_cloud_app(mock.as_ref(), "update-window", "app-1", "t").unwrap();
    assert!(opened.notice.starts_with("Updated to version 2.0.0."), "{}", opened.notice);
    assert!(state.lock().unwrap().log.contains(&"install:app-1".to_string()));

    // A failed update runs the installed version and says why.
    state.lock().unwrap().install_error = Some(super::errors::CloudError::new("ARCHIVE_CHECKSUM", "bad bytes"));
    let fallback = open_cloud_app(mock.as_ref(), "update-window-2", "app-1", "t").unwrap();
    assert_eq!(
        fallback.notice,
        "The update could not be installed (bad bytes). Running version 1.0.0."
    );
    // A revoked user is not given the old version.
    state.lock().unwrap().install_error = Some(super::errors::CloudError::new("REVOKED", "no"));
    let refused = open_cloud_app(mock.as_ref(), "update-window-3", "app-1", "t");
    assert_eq!(refused.err().map(|e| e.code), Some("REVOKED".to_string()));
}

#[test]
fn opening_the_wrong_application_is_refused() {
    let (mock, state) = Mock::new();
    {
        let mut s = state.lock().unwrap();
        s.session = Some(user("u1", "ada@example.test"));
        s.runtime_info = Some(runtime_info("someone-else"));
    }
    let refused = open_cloud_app(mock.as_ref(), "wrong-window", "app-1", "t");
    assert_eq!(refused.err().map(|e| e.code), Some("INSTALLATION_CORRUPT".to_string()));
}

// ---- values the tests above build

