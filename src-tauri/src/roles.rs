//! Runtime roles (PRD §20): local role definitions for navigation, object CRUD, and action access.
//! Field-level and row-level rules are deferred; cloud membership is out of scope.
use crate::archive::{check_named_ids, DocumentConfig, Issue};
use crate::design::NavigationItem;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Role {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub permissions: Permissions,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Permissions {
    /// Navigation item ids the role can see.
    #[serde(default)]
    pub navigation: Vec<String>,
    #[serde(default)]
    pub objects: Vec<ObjectPermission>,
    /// Action ids the role can execute.
    #[serde(default)]
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ObjectPermission {
    /// One of form, report, dashboard, table, query.
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub read: bool,
    #[serde(default)]
    pub create: bool,
    #[serde(default)]
    pub update: bool,
    #[serde(default)]
    pub delete: bool,
}

const OBJECT_KINDS: [&str; 5] = ["form", "report", "page", "table", "query"];

fn nav_ids(items: &[NavigationItem], out: &mut Vec<String>) {
    for item in items {
        out.push(item.id.clone());
        nav_ids(&item.children, out);
    }
}

pub fn validate(config: &DocumentConfig) -> Vec<Issue> {
    let mut issues = check_named_ids(
        "role",
        config
            .roles
            .iter()
            .map(|r| (r.id.as_str(), r.name.as_str())),
    );
    let mut navigation = vec![];
    nav_ids(&config.design.navigation, &mut navigation);
    for role in &config.roles {
        let warn = |message: String| Issue::warning("role", &role.id, message);
        let name = &role.name;
        for id in &role.permissions.navigation {
            if !navigation.contains(id) {
                issues.push(warn(format!(
                    "role \"{name}\" grants a navigation item that no longer exists"
                )));
            }
        }
        for id in &role.permissions.actions {
            if !config.actions.iter().any(|a| &a.id == id) {
                issues.push(warn(format!(
                    "role \"{name}\" grants an action that no longer exists"
                )));
            }
        }
        for object in &role.permissions.objects {
            let exists = match object.kind.as_str() {
                "form" => config.design.forms.iter().any(|f| f.id == object.id),
                "report" => config.reports.iter().any(|r| r.id == object.id),
                "page" => config.pages.iter().any(|d| d.id == object.id),
                "query" => config.saved_queries.iter().any(|q| q.id == object.id),
                _ => true,
            };
            if !OBJECT_KINDS.contains(&object.kind.as_str()) {
                issues.push(Issue::error(
                    "role",
                    &role.id,
                    format!(
                        "role \"{name}\" has a permission for unknown object kind \"{}\"",
                        object.kind
                    ),
                ));
            } else if !exists || object.id.is_empty() {
                issues.push(warn(format!(
                    "role \"{name}\" grants {} {}, which no longer exists",
                    object.kind, object.id
                )));
            }
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role(permissions: Permissions) -> Role {
        Role {
            id: "r1".into(),
            name: "Clerk".into(),
            permissions,
        }
    }

    #[test]
    fn roles_deserialize_without_permissions() {
        let role: Role =
            serde_json::from_value(serde_json::json!({"id": "r", "name": "Viewer"})).unwrap();
        assert_eq!(role.permissions, Permissions::default());
    }

    #[test]
    fn valid_role_has_no_issues() {
        let mut config = DocumentConfig::default();
        let (nav, form) = (
            config.design.navigation[0].id.clone(),
            config.design.forms[0].id.clone(),
        );
        config.roles.push(role(Permissions {
            navigation: vec![nav],
            objects: vec![ObjectPermission {
                kind: "form".into(),
                id: form,
                read: true,
                ..Default::default()
            }],
            actions: vec![],
        }));
        assert!(validate(&config).is_empty());
    }

    #[test]
    fn dangling_and_unknown_grants_are_reported() {
        let mut config = DocumentConfig::default();
        config.roles.push(role(Permissions {
            navigation: vec!["gone".into()],
            objects: vec![
                ObjectPermission {
                    kind: "form".into(),
                    id: "missing".into(),
                    ..Default::default()
                },
                ObjectPermission {
                    kind: "widget".into(),
                    id: "x".into(),
                    ..Default::default()
                },
            ],
            actions: vec!["nope".into()],
        }));
        let issues = validate(&config);
        assert_eq!(issues.len(), 4);
        assert_eq!(
            issues
                .iter()
                .filter(|i| i.severity == crate::validation::Severity::Error)
                .count(),
            1
        );
    }

    #[test]
    fn duplicate_role_ids_are_errors() {
        let mut config = DocumentConfig::default();
        config.roles.push(role(Permissions::default()));
        config.roles.push(role(Permissions::default()));
        assert!(validate(&config)
            .iter()
            .any(|i| i.message.contains("duplicate")));
    }
}
