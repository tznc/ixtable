//! Runtime RBAC checks (PRD §20; port of `src/runtime/rbac.ts`). Rust enforces the same
//! permissions (`authz.rs`); these checks decide what the Runtime shows and what its
//! actions may attempt.

use crate::archive::DocumentConfig;
use crate::dashboards::{ComponentKind, Dashboard};
use crate::roles::{ObjectPermission, Permissions, Role};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Read,
    Create,
    Update,
    Delete,
    Execute,
    View,
}

impl Operation {
    pub fn parse(op: &str) -> Option<Self> {
        Some(match op {
            "read" => Self::Read,
            "create" => Self::Create,
            "update" => Self::Update,
            "delete" => Self::Delete,
            "execute" => Self::Execute,
            "view" => Self::View,
            _ => return None,
        })
    }
}

/// A runtime role assigned by ixtable Cloud (signed manifest), with the signed-in user.
#[derive(Clone, Debug, PartialEq)]
pub struct AssignedRole {
    pub role: Role,
    pub user_name: String,
    pub user_email: Option<String>,
    pub user_id: Option<String>,
}

fn assigned() -> std::sync::MutexGuard<'static, Option<AssignedRole>> {
    static ASSIGNED: Mutex<Option<AssignedRole>> = Mutex::new(None);
    ASSIGNED.lock().unwrap_or_else(|e| e.into_inner())
}

fn previewed() -> std::sync::MutexGuard<'static, HashMap<String, String>> {
    static PREVIEWED: std::sync::OnceLock<Mutex<HashMap<String, String>>> =
        std::sync::OnceLock::new();
    PREVIEWED
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Pins the runtime to a cloud-assigned role (PRD §20, §21.2): while set, every check uses
/// it, a `None` role no longer means developer access, and the role's permissions come from
/// the signed manifest, not the archive. Process-wide, like a cloud sign-in.
pub fn assign_runtime_role(role: Option<AssignedRole>) {
    *assigned() = role;
}

pub fn assigned_runtime_role() -> Option<AssignedRole> {
    assigned().clone()
}

/// Role previewed in Studio Run mode for window `label` (Rust enforces it too).
pub fn set_previewed_role(label: &str, role_id: Option<&str>) {
    let mut map = previewed();
    match role_id {
        Some(id) => map.insert(label.to_string(), id.to_string()),
        None => map.remove(label),
    };
}

/// The role runtime checks use now: the cloud-assigned role, else the previewed one.
pub fn active_role_id(label: &str) -> Option<String> {
    if let Some(role) = assigned().as_ref() {
        return Some(role.role.id.clone());
    }
    previewed().get(label).cloned()
}

/// An action-context `authorize` for the active role (trigger and job contexts). `role_id`
/// (a job's originating role) further restricts it: both must allow.
pub fn authorizer(
    config: DocumentConfig,
    label: String,
    role_id: Option<String>,
) -> impl Fn(&str, &str, &str) -> bool + Send + Sync {
    move |kind, id, op| {
        let Some(op) = Operation::parse(op) else {
            return false;
        };
        can(&config, active_role_id(&label).as_deref(), kind, id, op)
            && role_id
                .as_deref()
                .is_none_or(|r| can(&config, Some(r), kind, id, op))
    }
}

/// The role with this id: the cloud-assigned role when pinned, else a local one.
/// `Ok(None)` is the developer (no role); `Err(())` is an unknown role.
pub fn find_role(config: &DocumentConfig, role_id: Option<&str>) -> Result<Option<Role>, ()> {
    if let Some(role) = assigned().as_ref() {
        return match role_id {
            None => Ok(Some(role.role.clone())),
            Some(id) if id == role.role.id => Ok(Some(role.role.clone())),
            Some(_) => Err(()),
        };
    }
    match role_id {
        None => Ok(None),
        Some(id) => config
            .roles
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .map(Some)
            .ok_or(()),
    }
}

/// Runtime RBAC check (PRD §20). A `None` role is the developer: full access. Unknown
/// roles are denied everything. Kinds:
/// - `navigation` + `view`: the item id is listed in the role's navigation.
/// - `action` + `execute`: the action id is listed in the role's actions.
/// - form/report/dashboard/table/query + read/create/update/delete (`view` = read).
pub fn can(
    config: &DocumentConfig,
    role_id: Option<&str>,
    object_kind: &str,
    object_id: &str,
    op: Operation,
) -> bool {
    if role_id.is_none() && assigned().is_none() {
        return true;
    }
    let Ok(Some(role)) = find_role(config, role_id) else {
        return false;
    };
    let permissions = &role.permissions;
    if object_kind == "navigation" {
        return permissions.navigation.iter().any(|id| id == object_id);
    }
    if object_kind == "action" {
        return op == Operation::Execute && permissions.actions.iter().any(|id| id == object_id);
    }
    let Some(entry) = permissions
        .objects
        .iter()
        .find(|o| o.kind == object_kind && o.id == object_id)
    else {
        return false;
    };
    match op {
        Operation::Read | Operation::View => entry.read,
        Operation::Create => entry.create,
        Operation::Update => entry.update,
        Operation::Delete => entry.delete,
        Operation::Execute => false,
    }
}

/// Saved queries a dashboard shows: KPI, table and chart queries, and filter options.
pub fn dashboard_query_ids(dashboard: &Dashboard) -> Vec<String> {
    let mut ids: Vec<String> = dashboard
        .components
        .iter()
        .filter(|c| {
            matches!(
                c.kind,
                ComponentKind::Kpi | ComponentKind::Table | ComponentKind::Chart
            )
        })
        .filter_map(|c| c.query_id.clone().filter(|q| !q.is_empty()))
        .collect();
    ids.extend(
        dashboard
            .filters
            .iter()
            .filter_map(|f| f.options_query_id.clone().filter(|q| !q.is_empty())),
    );
    ids
}

/// Read access to a query a dashboard shows: a dashboard grant implies read on its own
/// queries (`authz.rs`), never on the queries of the forms or reports it embeds, writes, or
/// actions.
pub fn can_read_dashboard_query(
    config: &DocumentConfig,
    role_id: Option<&str>,
    dashboard: &Dashboard,
    query_id: &str,
) -> bool {
    can(config, role_id, "query", query_id, Operation::Read)
        || (can(config, role_id, "dashboard", &dashboard.id, Operation::Read)
            && dashboard_query_ids(dashboard).iter().any(|q| q == query_id))
}

/// Returns `permissions` with one object flag set, adding the object entry when missing.
pub fn set_object_permission(
    permissions: &Permissions,
    kind: &str,
    id: &str,
    flag: &str,
    value: bool,
) -> Permissions {
    let existing = permissions
        .objects
        .iter()
        .position(|o| o.kind == kind && o.id == id);
    let mut next = existing.map_or_else(
        || ObjectPermission {
            kind: kind.into(),
            id: id.into(),
            ..Default::default()
        },
        |i| permissions.objects[i].clone(),
    );
    match flag {
        "read" => next.read = value,
        "create" => next.create = value,
        "update" => next.update = value,
        "delete" => next.delete = value,
        _ => {}
    }
    if flag != "read" && value {
        next.read = true;
    }
    if flag == "read" && !value {
        next.create = false;
        next.update = false;
        next.delete = false;
    }
    let mut objects = permissions.objects.clone();
    match existing {
        Some(i) => objects[i] = next,
        None => objects.push(next),
    }
    objects.retain(|o| o.read || o.create || o.update || o.delete);
    Permissions {
        objects,
        ..permissions.clone()
    }
}

/// Adds or removes `id` in a navigation/actions list.
pub fn toggle_listed(list: &[String], id: &str, on: bool) -> Vec<String> {
    if on {
        let mut next = list.to_vec();
        if !next.iter().any(|i| i == id) {
            next.push(id.to_string());
        }
        next
    } else {
        list.iter().filter(|i| *i != id).cloned().collect()
    }
}

/// Serializes tests that touch the process-wide assigned or previewed role.
#[cfg(test)]
pub fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
#[path = "rbac_tests.rs"]
pub(crate) mod tests;
