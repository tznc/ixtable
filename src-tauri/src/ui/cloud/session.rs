//! The cloud installation open in this window, if any. Its signed manifest's role
//! becomes the only runtime role (no "preview as role" switch). Port of
//! src/cloud/session.ts.
//!
//! The Runtime registers what it does with the role through `set_role_hook`; this
//! module only decides which role the manifest grants.

use crate::cloud::runtime_commands::CloudRuntimeInfo;
use gpui_kit::{App, AppContext, Context, Entity, EventEmitter, Global};
use serde_json::Value;
use std::sync::{Arc, OnceLock};

const NO_ROLE: &str = "cloud:no-role";

/// A runtime role granted by a signed manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct AssignedRole {
    pub id: String,
    pub name: String,
    pub navigation: Vec<String>,
    pub objects: Vec<Value>,
    pub actions: Vec<String>,
    pub user: RoleUser,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoleUser {
    pub name: String,
    pub email: String,
    pub id: String,
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// The runtime role a signed manifest grants. Only the manifest's signed `owner` flag
/// (the app's Developer/Owner) gives developer access (None: no assigned role). Any
/// other manifest without a role maps to a role that is allowed nothing (fail closed).
pub fn runtime_role_for(info: &CloudRuntimeInfo) -> Option<AssignedRole> {
    if info.owner && info.role_id.is_none() {
        return None;
    }
    let permissions = &info.role_permissions;
    Some(AssignedRole {
        id: info.role_id.clone().unwrap_or_else(|| NO_ROLE.to_string()),
        name: info.role_name.clone().unwrap_or_else(|| "Runtime user".to_string()),
        navigation: strings(permissions, "navigation"),
        objects: permissions.get("objects").and_then(Value::as_array).cloned().unwrap_or_default(),
        actions: strings(permissions, "actions"),
        user: RoleUser { name: info.email.clone(), email: info.email.clone(), id: info.user_id.clone() },
    })
}

/// Called whenever the cloud role of the window changes (None: no cloud role).
pub type RoleHook = fn(Option<AssignedRole>, &mut App);

static ROLE_HOOK: OnceLock<RoleHook> = OnceLock::new();

pub fn set_role_hook(hook: RoleHook) {
    let _ = ROLE_HOOK.set(hook);
}

#[derive(Clone)]
pub struct Active {
    pub session_id: String,
    pub info: Arc<CloudRuntimeInfo>,
}

/// The cloud installation shown in the window (a model views observe).
#[derive(Default)]
pub struct CloudRuntime {
    active: Option<Active>,
}

impl EventEmitter<RuntimeChanged> for CloudRuntime {}
pub struct RuntimeChanged;

struct RuntimeGlobal(Entity<CloudRuntime>);
impl Global for RuntimeGlobal {}

impl CloudRuntime {
    pub fn global(cx: &mut App) -> Entity<CloudRuntime> {
        if cx.try_global::<RuntimeGlobal>().is_none() {
            let model = cx.new(|_| CloudRuntime::default());
            cx.set_global(RuntimeGlobal(model));
        }
        cx.global::<RuntimeGlobal>().0.clone()
    }

    pub fn active(&self) -> Option<&Active> {
        self.active.as_ref()
    }

    /// The role of the open cloud installation.
    pub fn role(&self) -> Option<AssignedRole> {
        self.active.as_ref().and_then(|a| runtime_role_for(&a.info))
    }

    fn changed(&self, cx: &mut Context<Self>) {
        let role = self.role();
        cx.emit(RuntimeChanged);
        cx.notify();
        if let Some(hook) = ROLE_HOOK.get() {
            hook(role, cx);
        }
    }

    pub fn set(&mut self, session_id: &str, info: CloudRuntimeInfo, cx: &mut Context<Self>) {
        self.active = Some(Active { session_id: session_id.to_string(), info: Arc::new(info) });
        self.changed(cx);
    }

    /// Clears the cloud runtime unless `session_id` is the cloud session (the app frame
    /// calls it on every switch).
    pub fn release_unless(&mut self, session_id: Option<&str>, cx: &mut Context<Self>) {
        match &self.active {
            Some(a) if Some(a.session_id.as_str()) != session_id => {
                self.active = None;
                self.changed(cx);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use serde_json::json;

    pub fn info(role_id: Option<&str>, permissions: Value) -> CloudRuntimeInfo {
        CloudRuntimeInfo {
            app_id: "app".into(),
            app_name: "Orders".into(),
            version_id: "v1".into(),
            version: "1.0.0".into(),
            user_id: "u1".into(),
            email: "ada@example.com".into(),
            role_id: role_id.map(str::to_string),
            role_name: role_id.map(|_| "Sales".to_string()),
            role_permissions: permissions,
            owner: false,
            installation_id: "i1".into(),
            fingerprint: "f".into(),
            issued_at: String::new(),
            expires_at: String::new(),
            public_key_fingerprint: String::new(),
            postgres: false,
            datasource_id: String::new(),
            grant_expires_at: None,
        }
    }

    #[test]
    fn a_runtime_user_gets_the_manifests_role_name_and_permissions() {
        let permissions = json!({ "navigation": ["forms"], "objects": [], "actions": [] });
        let role = runtime_role_for(&info(Some("r1"), permissions)).unwrap();
        assert_eq!((role.id.as_str(), role.name.as_str()), ("r1", "Sales"));
        assert_eq!(role.navigation, ["forms"]);
        assert_eq!((role.user.email.as_str(), role.user.id.as_str()), ("ada@example.com", "u1"));
    }

    #[test]
    fn only_the_signed_owner_flag_gives_developer_access_and_a_missing_role_allows_nothing() {
        let mut owner = info(None, Value::Null);
        owner.owner = true;
        assert!(runtime_role_for(&owner).is_none());
        owner.owner = false;
        let role = runtime_role_for(&owner).unwrap();
        assert_eq!(role.id, "cloud:no-role");
        assert_eq!(role.name, "Runtime user");
        assert!(role.navigation.is_empty() && role.objects.is_empty() && role.actions.is_empty());
    }
}
