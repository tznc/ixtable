//! Everything the cloud views ask of ixtable Cloud and of the Rust backend, behind one
//! trait so tests can stand in for the server (src/cloud/client.ts and api.ts). The
//! live implementation is `client::LiveServices`; the views get the app's services
//! from the `Services` global.
//!
//! Every method blocks: views call them from `task::spawn_call`.

use super::{errors::CloudError, types::*};
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
use gpui_kit::{App, Global};
use serde_json::Value;
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, CloudError>;

pub trait CloudServices: Send + Sync + 'static {
    // ---- the cloud (supabase)

    /// The stored session, refreshed when it is about to expire; None when signed out.
    fn load_session(&self) -> Option<Session>;
    /// None: the account needs email confirmation before it has a session.
    fn sign_in(&self, email: &str, password: &str) -> Result<Option<Session>>;
    fn sign_up(&self, email: &str, password: &str) -> Result<Option<Session>>;
    fn request_password_reset(&self, email: &str) -> Result<()>;
    /// Adopts a session handed over by the browser sign-in.
    fn adopt_session(&self, tokens: SessionTokens) -> Result<Option<Session>>;
    /// Revokes the session and clears everything stored locally.
    fn sign_out(&self);
    /// Calls an Edge Function with the user's JWT; the reply is decoded by `contract`.
    fn invoke(&self, name: &str, body: Value) -> Result<Value>;
    fn list_organizations(&self) -> Result<Vec<Organization>>;
    fn create_organization(&self, name: &str) -> Result<Organization>;
    fn get_app(&self, app_id: &str) -> Result<Option<CloudApp>>;
    fn list_versions(&self, app_id: &str) -> Result<Vec<AppVersion>>;
    fn list_installation_backups(&self, app_id: &str, installation_id: &str) -> Result<Vec<InstallationBackup>>;
    /// Release notes of a published version ("" when the row is not readable).
    fn version_release_notes(&self, version_id: &str) -> Result<String>;
    /// How many runtime users the app can have: active members, or the plan's allowance.
    fn runtime_capacity(&self, app_id: &str) -> Result<u32>;
    fn list_member_apps(&self) -> Result<Vec<MemberApp>>;

    // ---- the Rust backend (`window` is the window label)

    fn desktop_auth_start(&self, window: &str, provider: &str) -> Result<DesktopAuthStart>;
    fn desktop_auth_poll(&self, window: &str, state: &str) -> Result<DesktopAuthPoll>;
    fn publish_preflight(&self, window: &str, runtime_users: u32) -> Result<Preflight>;
    fn upload_archive(
        &self,
        window: &str,
        access_token: &str,
        app_id: &str,
        kind: &str,
        transfer_id: &str,
    ) -> Result<UploadResult>;
    #[allow(clippy::too_many_arguments)]
    fn upload_credential(
        &self,
        window: &str,
        access_token: &str,
        app_id: &str,
        scope: &str,
        user_id: Option<String>,
        password: Option<String>,
        username: Option<String>,
    ) -> Result<()>;
    fn restore_copy(
        &self,
        window: &str,
        signed_url: &str,
        sha256: &str,
        size: u64,
        version_id: Option<String>,
        transfer_id: &str,
    ) -> Result<SessionState>;
    fn transfer_progress(&self, transfer_id: &str) -> Option<Progress>;
    fn installed_apps(&self, window: &str) -> Result<Vec<LocalInstallation>>;
    fn install_app(
        &self,
        window: &str,
        access_token: &str,
        app_id: &str,
        user_id: &str,
        email: &str,
        transfer_id: &str,
    ) -> Result<SessionState>;
    fn open_installed(&self, window: &str, app_id: &str) -> Result<SessionState>;
    fn runtime_info(&self, window: &str) -> Result<CloudRuntimeInfo>;
    fn key_grant(&self, window: &str, access_token: &str) -> Result<GrantResult>;
    fn release_credentials(&self, window: &str) -> Result<()>;
    fn archive_size_report(&self, window: &str) -> Result<ArchiveSizeReport>;
    fn runtime_installation_info(&self, window: &str) -> Result<InstalledBundle>;
}

/// The app's cloud services (the live client unless a test installed another).
#[derive(Clone)]
pub struct Services(pub Arc<dyn CloudServices>);
impl Global for Services {}

impl Services {
    /// The services installed for this app, creating the live ones on first use.
    pub fn get(cx: &mut App) -> Arc<dyn CloudServices> {
        if cx.try_global::<Services>().is_none() {
            cx.set_global(Services(Arc::new(super::client::LiveServices::default())));
        }
        cx.global::<Services>().0.clone()
    }

    pub fn install(services: Arc<dyn CloudServices>, cx: &mut App) {
        cx.set_global(Services(services));
    }
}
