//! A scripted stand-in for ixtable Cloud and the Rust cloud commands, for UI tests. The
//! test keeps the `State` handle: it sets what the services answer and reads what the
//! views asked.

use super::{
    errors::CloudError,
    services::{CloudServices, Result},
    types::*,
};
use crate::{
    assets::ArchiveSizeReport,
    cloud::{
        commands::{DesktopAuthPoll, DesktopAuthStart, UploadResult},
        http::Progress,
        install::LocalInstallation,
        publish::Preflight,
        runtime_commands::{CloudRuntimeInfo, GrantResult},
    },
    installation::InstalledBundle,
    manager::SessionState,
};
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

type Info = Arc<dyn Fn() -> CloudRuntimeInfo + Send + Sync>;

#[derive(Default)]
pub struct State {
    pub session: Option<Session>,
    /// What `sign_in` / `sign_up` answer when set (otherwise they sign in as the email).
    pub auth_error: Option<CloudError>,
    pub needs_confirmation: bool,
    pub resets: Vec<String>,
    pub signed_out: u32,
    pub orgs: Vec<Organization>,
    pub created_orgs: Vec<String>,
    pub app: Option<CloudApp>,
    pub versions: Vec<AppVersion>,
    pub backups: Vec<InstallationBackup>,
    pub members: Vec<MemberApp>,
    /// `list_member_apps` fails with this when set (offline).
    pub members_error: Option<CloudError>,
    /// Replies of `invoke` per function name, in order; the last one repeats.
    pub replies: HashMap<String, VecDeque<Result<Value>>>,
    /// Every `invoke`: function name and body.
    pub calls: Vec<(String, Value)>,
    /// Every other call, as `name` or `name:detail`.
    pub log: Vec<String>,
    pub release_notes: String,
    pub capacity: u32,
    pub preflight: Option<Preflight>,
    pub preflight_error: Option<CloudError>,
    pub upload: Option<(String, u64, Option<String>)>,
    pub upload_error: Option<CloudError>,
    pub size_report: Option<ArchiveSizeReport>,
    pub installed: Vec<LocalInstallation>,
    pub install_error: Option<CloudError>,
    pub runtime_info: Option<Info>,
    pub grants: VecDeque<Result<GrantResult>>,
    pub installation: Option<InstalledBundle>,
    pub credential_error: Option<CloudError>,
    pub browser: Option<DesktopAuthPoll>,
}

#[derive(Clone, Default)]
pub struct Mock(pub Arc<Mutex<State>>);

impl Mock {
    pub fn new() -> (Arc<Self>, Arc<Mutex<State>>) {
        let mock = Self::default();
        let state = mock.0.clone();
        (Arc::new(mock), state)
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn note(&self, entry: String) {
        self.state().log.push(entry);
    }
}

/// A signed-in user.
pub fn user(id: &str, email: &str) -> Session {
    Session { access_token: "jwt".into(), user_id: id.into(), email: Some(email.into()) }
}

/// Queues the replies of an Edge Function.
pub fn reply(state: &Mutex<State>, name: &str, replies: Vec<Result<Value>>) {
    state.lock().unwrap().replies.insert(name.into(), replies.into());
}

impl CloudServices for Mock {
    fn load_session(&self) -> Option<Session> {
        self.state().session.clone()
    }

    fn sign_in(&self, email: &str, _: &str) -> Result<Option<Session>> {
        let mut s = self.state();
        if let Some(e) = s.auth_error.clone() {
            return Err(e);
        }
        let session = user("u-signed-in", email);
        s.session = Some(session.clone());
        Ok(Some(session))
    }

    fn sign_up(&self, email: &str, password: &str) -> Result<Option<Session>> {
        if self.state().needs_confirmation {
            return Ok(None);
        }
        self.sign_in(email, password)
    }

    fn request_password_reset(&self, email: &str) -> Result<()> {
        self.state().resets.push(email.into());
        Ok(())
    }

    fn adopt_session(&self, _: SessionTokens) -> Result<Option<Session>> {
        let session = user("u-browser", "browser@example.test");
        self.state().session = Some(session.clone());
        Ok(Some(session))
    }

    fn sign_out(&self) {
        let mut s = self.state();
        s.session = None;
        s.signed_out += 1;
    }

    fn invoke(&self, name: &str, body: Value) -> Result<Value> {
        let mut s = self.state();
        s.calls.push((name.into(), body));
        match s.replies.get_mut(name) {
            Some(queue) if queue.len() > 1 => queue.pop_front().unwrap(),
            Some(queue) => queue.front().cloned().unwrap_or(Ok(Value::Null)),
            None => Err(CloudError::new("NOT_FOUND", format!("no scripted reply for {name}"))),
        }
    }

    fn list_organizations(&self) -> Result<Vec<Organization>> {
        Ok(self.state().orgs.clone())
    }

    fn create_organization(&self, name: &str) -> Result<Organization> {
        let mut s = self.state();
        s.created_orgs.push(name.into());
        Ok(Organization { id: "org-new".into(), name: name.into(), role: "owner".into() })
    }

    fn get_app(&self, app_id: &str) -> Result<Option<CloudApp>> {
        self.note(format!("get_app:{app_id}"));
        Ok(self.state().app.clone().filter(|a| a.id == app_id))
    }

    fn list_versions(&self, _: &str) -> Result<Vec<AppVersion>> {
        Ok(self.state().versions.clone())
    }

    fn list_installation_backups(&self, _: &str, _: &str) -> Result<Vec<InstallationBackup>> {
        self.note("list_installation_backups".into());
        Ok(self.state().backups.clone())
    }

    fn version_release_notes(&self, version_id: &str) -> Result<String> {
        self.note(format!("release_notes:{version_id}"));
        Ok(self.state().release_notes.clone())
    }

    fn runtime_capacity(&self, _: &str) -> Result<u32> {
        Ok(self.state().capacity)
    }

    fn list_member_apps(&self) -> Result<Vec<MemberApp>> {
        self.note("list_member_apps".into());
        let s = self.state();
        match &s.members_error {
            Some(e) => Err(e.clone()),
            None => Ok(s.members.clone()),
        }
    }

    fn desktop_auth_start(&self, _: &str, provider: &str) -> Result<DesktopAuthStart> {
        self.note(format!("desktop_auth_start:{provider}"));
        Ok(DesktopAuthStart {
            state: "st".into(),
            url: "https://ixtable.example/desktop-auth?code_challenge=abc".into(),
            opened: false,
        })
    }

    fn desktop_auth_poll(&self, _: &str, _: &str) -> Result<DesktopAuthPoll> {
        match self.state().browser.take() {
            Some(poll) => Ok(poll),
            None => Ok(DesktopAuthPoll { status: "pending".into(), session: None }),
        }
    }

    fn publish_preflight(&self, _: &str, runtime_users: u32) -> Result<Preflight> {
        self.note(format!("preflight:{runtime_users}"));
        let s = self.state();
        if let Some(e) = &s.preflight_error {
            return Err(e.clone());
        }
        s.preflight.clone().ok_or_else(|| CloudError::new("NOT_FOUND", "no scripted preflight"))
    }

    fn upload_archive(&self, _: &str, _: &str, app_id: &str, kind: &str, _: &str) -> Result<UploadResult> {
        self.note(format!("upload:{kind}:{app_id}"));
        let s = self.state();
        if let Some(e) = &s.upload_error {
            return Err(e.clone());
        }
        let (id, size, installation) =
            s.upload.clone().unwrap_or_else(|| ("up-1".into(), 2048, None));
        Ok(UploadResult {
            upload_id: id,
            path: "apps/x/p.ixt".into(),
            sha256: "a".repeat(64),
            size,
            installation_id: installation,
        })
    }

    fn upload_credential(
        &self,
        _: &str,
        _: &str,
        app_id: &str,
        scope: &str,
        user_id: Option<String>,
        password: Option<String>,
        username: Option<String>,
    ) -> Result<()> {
        self.note(format!(
            "credential:{app_id}:{scope}:{}:{}:{}",
            user_id.unwrap_or_default(),
            password.unwrap_or_default(),
            username.unwrap_or_default()
        ));
        match self.state().credential_error.clone() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn restore_copy(
        &self,
        window: &str,
        signed_url: &str,
        _: &str,
        _: u64,
        version_id: Option<String>,
        _: &str,
    ) -> Result<SessionState> {
        self.note(format!("restore:{signed_url}:{}", version_id.unwrap_or_default()));
        crate::new_document(window.to_string()).map_err(Into::into)
    }

    fn transfer_progress(&self, _: &str) -> Option<Progress> {
        None
    }

    fn installed_apps(&self, _: &str) -> Result<Vec<LocalInstallation>> {
        Ok(self.state().installed.clone())
    }

    fn install_app(&self, window: &str, _: &str, app_id: &str, _: &str, _: &str, _: &str) -> Result<SessionState> {
        self.note(format!("install:{app_id}"));
        if let Some(e) = self.state().install_error.clone() {
            return Err(e);
        }
        crate::new_document(window.to_string()).map_err(Into::into)
    }

    fn open_installed(&self, window: &str, app_id: &str) -> Result<SessionState> {
        self.note(format!("open_installed:{app_id}"));
        crate::new_document(window.to_string()).map_err(Into::into)
    }

    fn runtime_info(&self, _: &str) -> Result<CloudRuntimeInfo> {
        let make = self.state().runtime_info.clone();
        make.map(|f| f()).ok_or_else(|| CloudError::new("NOT_FOUND", "no scripted runtime info"))
    }

    fn key_grant(&self, _: &str, _: &str) -> Result<GrantResult> {
        self.note("key_grant".into());
        self.state().grants.pop_front().unwrap_or_else(|| {
            Ok(GrantResult { needed: true, expires_at: None, attached: false, error: None })
        })
    }

    fn release_credentials(&self, _: &str) -> Result<()> {
        self.note("release_credentials".into());
        Ok(())
    }

    fn archive_size_report(&self, _: &str) -> Result<ArchiveSizeReport> {
        self.state()
            .size_report
            .clone()
            .ok_or_else(|| CloudError::new("NOT_FOUND", "no scripted size report"))
    }

    fn runtime_installation_info(&self, _: &str) -> Result<InstalledBundle> {
        self.state()
            .installation
            .clone()
            .ok_or_else(|| CloudError::new("NOT_FOUND", "no installation record"))
    }
}
