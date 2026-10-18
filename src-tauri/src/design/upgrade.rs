//! Upgrades older serialized design schemas (v1, v2) to the current shape.
use super::DESIGN_SCHEMA_VERSION;
use serde_json::{json, Map, Value};

/// Rewrites a v1/v2 design value into v3. Newer versions pass through unchanged
/// so structural validation can reject them.
pub fn upgrade_value(mut value: Value) -> Value {
    let Some(design) = value.as_object_mut() else {
        return value;
    };
    let version = design
        .get("version")
        .and_then(Value::as_u64)
        .unwrap_or(DESIGN_SCHEMA_VERSION as u64);
    if version >= DESIGN_SCHEMA_VERSION as u64 {
        return value;
    }
    if let Some(forms) = design.get_mut("forms").and_then(Value::as_array_mut) {
        forms
            .iter_mut()
            .filter_map(Value::as_object_mut)
            .for_each(upgrade_form);
    }
    let mut first_nav = None;
    if let Some(items) = design.get_mut("navigation").and_then(Value::as_array_mut) {
        for item in items.iter_mut().filter_map(Value::as_object_mut) {
            if let Some(form_id) = item.remove("formId") {
                item.insert("kind".into(), json!("form"));
                item.insert("targetId".into(), form_id);
            }
            if first_nav.is_none() {
                first_nav = item.get("id").cloned();
            }
        }
    }
    if !design.contains_key("startPage") {
        design.insert("startPage".into(), first_nav.unwrap_or(Value::Null));
    }
    design.insert("version".into(), json!(DESIGN_SCHEMA_VERSION));
    value
}

fn upgrade_form(form: &mut Map<String, Value>) {
    if let Some(table) = form.remove("table") {
        if let Some(name) = table.as_str().filter(|name| !name.is_empty()) {
            form.insert("source".into(), json!({"kind": "table", "table": name}));
        }
    }
    let Some(controls) = form.get_mut("controls").and_then(Value::as_array_mut) else {
        return;
    };
    for control in controls.iter_mut().filter_map(Value::as_object_mut) {
        if control.get("kind").and_then(Value::as_str) == Some("checkbox") {
            control.insert("kind".into(), json!("boolean"));
        }
    }
}

/// Id the old default design gave its form and its navigation item (not a UUID).
pub const LEGACY_DEFAULT_ID: &str = "main";

/// New UUIDv7 ids for the legacy `main` form and navigation item, when the design has them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LegacyIds {
    pub form: Option<String>,
    pub navigation: Option<String>,
}

/// Rewrites the legacy `main` form and navigation ids of a loaded config to UUIDv7 ids,
/// with every reference to them: navigation targets, `startPage`, `detailFormId`,
/// related-list forms, `openForm`/`navigate` action steps, page form components,
/// and role permissions. Returns the ids it assigned. Mirrors `upgradeLegacyIds` in
/// src/design/schema.ts. Studio runs it on open; runtime sessions keep ids as published.
pub fn rekey_legacy_ids(config: &mut crate::archive::DocumentConfig) -> Result<LegacyIds, String> {
    let mut value = serde_json::to_value(&*config).map_err(|e| e.to_string())?;
    let ids = legacy_ids(&value, || uuid::Uuid::now_v7().to_string());
    if ids == LegacyIds::default() {
        return Ok(ids);
    }
    rekey_value(&mut value, &ids);
    *config = serde_json::from_value(value).map_err(|e| e.to_string())?;
    Ok(ids)
}

/// Which legacy ids the config holds, each paired with a fresh id from `new_id`.
pub fn legacy_ids(config: &Value, mut new_id: impl FnMut() -> String) -> LegacyIds {
    let design = &config["design"];
    let has_form = design["forms"]
        .as_array()
        .is_some_and(|forms| forms.iter().any(|f| is_legacy(&f["id"])));
    let has_nav = design["navigation"]
        .as_array()
        .is_some_and(|items| any_nav(items, &|item| is_legacy(&item["id"])));
    LegacyIds {
        form: has_form.then(&mut new_id),
        navigation: has_nav.then(&mut new_id),
    }
}

/// Applies `ids` to a serialized DocumentConfig (see [`rekey_legacy_ids`]).
/// Only existing fields change: `get_mut` never inserts missing keys.
pub fn rekey_value(config: &mut Value, ids: &LegacyIds) {
    let form = ids.form.as_deref();
    let nav = ids.navigation.as_deref();
    if let Some(design) = config.get_mut("design") {
        for f in items(design, "forms") {
            replace(f.get_mut("id"), form);
            replace(f.get_mut("detailFormId"), form);
            for control in items(f, "controls") {
                replace(
                    control.get_mut("related").and_then(|r| r.get_mut("formId")),
                    form,
                );
            }
        }
        rekey_nav(items(design, "navigation"), form, nav);
        replace(design.get_mut("startPage"), nav);
    }
    for action in items(config, "actions") {
        rekey_steps(items(action, "steps"), form);
    }
    // Raw configs older than version 8 keep pages under `dashboards`.
    for key in ["pages", "dashboards"] {
        for page in items(config, key) {
            for component in items(page, "components") {
                replace(component.get_mut("formId"), form);
            }
        }
    }
    for role in items(config, "roles") {
        let Some(permissions) = role.get_mut("permissions") else {
            continue;
        };
        for item in items(permissions, "navigation") {
            replace(Some(item), nav);
        }
        for object in items(permissions, "objects") {
            if object["kind"] == "form" {
                replace(object.get_mut("id"), form);
            }
        }
    }
}

fn is_legacy(value: &Value) -> bool {
    value.as_str() == Some(LEGACY_DEFAULT_ID)
}

fn replace(value: Option<&mut Value>, to: Option<&str>) {
    if let (Some(value), Some(to)) = (value, to) {
        if is_legacy(value) {
            *value = json!(to);
        }
    }
}

/// Items of the array at `key`; nothing when it is missing or not an array.
fn items<'a>(value: &'a mut Value, key: &str) -> std::slice::IterMut<'a, Value> {
    match value.get_mut(key).and_then(Value::as_array_mut) {
        Some(list) => list.iter_mut(),
        None => [].iter_mut(),
    }
}

fn any_nav(items: &[Value], test: &dyn Fn(&Value) -> bool) -> bool {
    items.iter().any(|item| {
        test(item)
            || item["children"]
                .as_array()
                .is_some_and(|children| any_nav(children, test))
    })
}

fn rekey_nav<'a>(list: impl Iterator<Item = &'a mut Value>, form: Option<&str>, nav: Option<&str>) {
    for item in list {
        replace(item.get_mut("id"), nav);
        // Navigation kind defaults to form.
        if item.get("kind").and_then(Value::as_str).unwrap_or("form") == "form" {
            replace(item.get_mut("targetId"), form);
        }
        rekey_nav(items(item, "children"), form, nav);
    }
}

fn rekey_steps<'a>(steps: impl Iterator<Item = &'a mut Value>, form: Option<&str>) {
    for step in steps {
        match step["kind"].as_str() {
            Some("openForm") => replace(step.get_mut("formId"), form),
            Some("navigate") => {
                if let Some(target) = step.get_mut("target").filter(|t| t["kind"] == "form") {
                    replace(target.get_mut("id"), form);
                }
            }
            _ => {}
        }
        for branch in ["then", "else"] {
            rekey_steps(items(step, branch), form);
        }
    }
}

#[cfg(test)]
mod legacy_id_tests {
    use super::*;
    use crate::archive::DocumentConfig;

    fn legacy() -> Value {
        json!({
            "version": 3, "name": "Old", "activeMode": "design",
            "design": {
                "version": 3,
                "forms": [
                    {"id": "main", "name": "Main form", "detailFormId": "main", "controls": [
                        {"id": "c1", "kind": "relatedList", "label": "Lines", "placement": {"column": 1, "row": 1},
                         "related": {"table": "t", "foreignKey": "p", "parentColumn": "id", "columns": [], "formId": "main"}}
                    ]},
                    {"id": "other", "name": "Other", "detailFormId": null}
                ],
                "navigation": [
                    {"id": "main", "label": "Main form", "kind": "form", "targetId": "main"},
                    {"id": "g", "label": "Group", "kind": "group", "children": [
                        {"id": "n2", "label": "Again", "targetId": "main"},
                        {"id": "n3", "label": "Report", "kind": "report", "targetId": "main"}
                    ]}
                ],
                "startPage": "main"
            },
            "actions": [{"id": "a", "name": "Open", "onError": "stop", "steps": [
                {"id": "s1", "kind": "openForm", "formId": "main"},
                {"id": "s2", "kind": "condition", "then": [
                    {"id": "s3", "kind": "navigate", "target": {"kind": "form", "id": "main"}}
                ], "else": [
                    {"id": "s4", "kind": "navigate", "target": {"kind": "report", "id": "main"}}
                ]}
            ]}],
            "dashboards": [{"id": "d", "name": "D", "components": [
                {"id": "k", "kind": "form", "title": "F", "placement": {"column": 1, "row": 1}, "formId": "main"}
            ]}],
            "roles": [{"id": "r", "name": "Clerk", "permissions": {
                "navigation": ["main", "g"],
                "objects": [
                    {"kind": "form", "id": "main", "read": true},
                    {"kind": "report", "id": "main", "read": true}
                ],
                "actions": []
            }}]
        })
    }

    #[test]
    fn rewrites_the_legacy_form_and_navigation_ids_and_their_references() {
        let mut value = legacy();
        let mut next = ["F", "N"].into_iter().map(String::from);
        let ids = legacy_ids(&value, || next.next().unwrap());
        assert_eq!(
            ids,
            LegacyIds {
                form: Some("F".into()),
                navigation: Some("N".into())
            }
        );
        rekey_value(&mut value, &ids);
        let design = &value["design"];
        assert_eq!(design["forms"][0]["id"], "F");
        assert_eq!(design["forms"][0]["detailFormId"], "F");
        assert_eq!(design["forms"][0]["controls"][0]["related"]["formId"], "F");
        assert_eq!(design["forms"][1]["id"], "other");
        assert_eq!(design["navigation"][0]["id"], "N");
        assert_eq!(design["navigation"][0]["targetId"], "F");
        assert_eq!(design["navigation"][1]["children"][0]["targetId"], "F");
        // A report that happens to be called `main` is a different object.
        assert_eq!(design["navigation"][1]["children"][1]["targetId"], "main");
        assert_eq!(design["startPage"], "N");
        let steps = &value["actions"][0]["steps"];
        assert_eq!(steps[0]["formId"], "F");
        assert_eq!(steps[1]["then"][0]["target"]["id"], "F");
        assert_eq!(steps[1]["else"][0]["target"]["id"], "main");
        assert_eq!(value["dashboards"][0]["components"][0]["formId"], "F");
        let permissions = &value["roles"][0]["permissions"];
        assert_eq!(permissions["navigation"], json!(["N", "g"]));
        assert_eq!(permissions["objects"][0]["id"], "F");
        assert_eq!(permissions["objects"][1]["id"], "main");
    }

    #[test]
    fn rekeys_a_config_with_uuid_v7_ids_and_leaves_current_ones_alone() {
        let mut config: DocumentConfig = serde_json::from_value(legacy()).unwrap();
        let ids = rekey_legacy_ids(&mut config).unwrap();
        let form = ids.form.unwrap();
        let nav = ids.navigation.unwrap();
        assert_eq!(uuid::Uuid::parse_str(&form).unwrap().get_version_num(), 7);
        assert_ne!(form, nav);
        assert_eq!(config.design.forms[0].id, form);
        assert_eq!(config.design.start_page.as_deref(), Some(nav.as_str()));
        assert_eq!(config.roles[0].permissions.navigation[0], nav);
        assert!(config.design.validate().is_ok());
        let again = config.clone();
        assert_eq!(rekey_legacy_ids(&mut config).unwrap(), LegacyIds::default());
        assert_eq!(config, again);
    }

    #[test]
    fn new_documents_get_uuid_v7_default_ids() {
        let config = DocumentConfig::default();
        let form = &config.design.forms[0].id;
        assert_eq!(uuid::Uuid::parse_str(form).unwrap().get_version_num(), 7);
        let nav = &config.design.navigation[0];
        // Navigation opens the page that holds the form.
        let page = config.pages.iter().find(|p| Some(&p.id) == nav.target_id.as_ref()).unwrap();
        assert_eq!(uuid::Uuid::parse_str(&page.id).unwrap().get_version_num(), 7);
        assert_eq!(page.components[0].form_id.as_deref(), Some(form.as_str()));
        assert_eq!(config.design.start_page.as_deref(), Some(nav.id.as_str()));
        assert_ne!(&nav.id, form);
    }
}
