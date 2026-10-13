//! The step list as JSON values and the edits the step editor makes to it (port of
//! `src/automation/steps.ts` and the list operations of `StepEditor.tsx`).
//!
//! Steps keep their kind-specific fields as JSON (`automation::Step::fields`), and the
//! branches of a condition are nested arrays inside it. The editor therefore edits the
//! action's steps as a `Vec<Value>` and converts back to `Step`s when it saves. Steps are
//! addressed by their stable ids, never by position.

use crate::automation::{ActionDef, Step};
use serde_json::{json, Map, Value};

/// The step kinds in the order the editor offers them.
pub const STEP_KINDS: [&str; 16] = [
    "createRecord",
    "updateRecord",
    "deleteRecord",
    "runQuery",
    "navigate",
    "openForm",
    "closeForm",
    "openReport",
    "openDashboard",
    "setState",
    "confirm",
    "message",
    "condition",
    "runAction",
    "fail",
    "setField",
];

/// A fresh step of `kind` with its default fields.
pub fn new_step(kind: &str) -> Value {
    let id = uuid::Uuid::new_v4().to_string();
    match kind {
        "createRecord" => json!({"id": id, "kind": kind, "table": "", "values": {}}),
        "updateRecord" => {
            json!({"id": id, "kind": kind, "table": "", "match": "current", "values": {}})
        }
        "deleteRecord" => json!({"id": id, "kind": kind, "table": "", "match": "current"}),
        "runQuery" => json!({"id": id, "kind": kind, "queryId": "", "params": {}, "storeAs": "rows"}),
        "navigate" => json!({"id": id, "kind": kind, "target": {"kind": "form", "id": ""}}),
        "openForm" => json!({"id": id, "kind": kind, "formId": ""}),
        "openReport" => json!({"id": id, "kind": kind, "reportId": ""}),
        "openDashboard" => json!({"id": id, "kind": kind, "dashboardId": ""}),
        "setState" => json!({"id": id, "kind": kind, "scope": "app", "key": "", "value": ""}),
        "confirm" => json!({"id": id, "kind": kind, "message": "'Are you sure?'"}),
        "message" => json!({"id": id, "kind": kind, "text": "", "tone": "info"}),
        "condition" => json!({"id": id, "kind": kind, "when": "", "then": [], "else": []}),
        "runAction" => json!({"id": id, "kind": kind, "actionId": ""}),
        "fail" => json!({"id": id, "kind": kind, "message": ""}),
        "setField" => json!({"id": id, "kind": kind, "field": "", "value": ""}),
        // closeForm and anything else have no fields
        _ => json!({"id": id, "kind": kind}),
    }
}

/// The steps of an action as JSON values.
pub fn to_values(steps: &[Step]) -> Vec<Value> {
    steps
        .iter()
        .filter_map(|s| serde_json::to_value(s).ok())
        .collect()
}

/// The values back as steps; a value that is not a step is dropped.
pub fn from_values(values: &[Value]) -> Vec<Step> {
    values
        .iter()
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect()
}

/// One branch of a condition step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Branch {
    Then,
    Else,
}

impl Branch {
    pub fn key(self) -> &'static str {
        match self {
            Branch::Then => "then",
            Branch::Else => "else",
        }
    }
}

/// A list of steps: the action's own (empty path) or a branch of a condition step,
/// reached through the (condition step id, branch) pairs from the root.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ListPath(pub Vec<(String, Branch)>);

impl ListPath {
    pub fn root() -> Self {
        Self::default()
    }

    pub fn branch(&self, step_id: &str, branch: Branch) -> Self {
        let mut path = self.0.clone();
        path.push((step_id.to_string(), branch));
        Self(path)
    }
}

fn id_of(step: &Value) -> &str {
    step.get("id").and_then(Value::as_str).unwrap_or("")
}

/// The step list at `path`.
pub fn list_mut<'a>(root: &'a mut Vec<Value>, path: &ListPath) -> Option<&'a mut Vec<Value>> {
    let mut list = root;
    for (id, branch) in &path.0 {
        let step = list.iter_mut().find(|s| id_of(s) == id)?;
        let object = step.as_object_mut()?;
        let next = object
            .entry(branch.key())
            .or_insert_with(|| Value::Array(vec![]));
        list = match next {
            Value::Array(items) => items,
            _ => return None,
        };
    }
    Some(list)
}

/// Where the step with `id` is: the (condition index, branch) hops from the root and its
/// index in the final list.
fn locate(steps: &[Value], id: &str) -> Option<(Vec<(usize, &'static str)>, usize)> {
    if let Some(index) = steps.iter().position(|s| id_of(s) == id) {
        return Some((vec![], index));
    }
    for (i, step) in steps.iter().enumerate() {
        for branch in ["then", "else"] {
            let Some(items) = step.get(branch).and_then(Value::as_array) else {
                continue;
            };
            if let Some((mut hops, index)) = locate(items, id) {
                hops.insert(0, (i, branch));
                return Some((hops, index));
            }
        }
    }
    None
}

/// The step with `id` anywhere in the tree.
pub fn find_mut<'a>(steps: &'a mut [Value], id: &str) -> Option<&'a mut Map<String, Value>> {
    let (hops, index) = locate(steps, id)?;
    let mut list = steps;
    for (i, branch) in hops {
        list = list[i].as_object_mut()?.get_mut(branch)?.as_array_mut()?;
    }
    list[index].as_object_mut()
}

/// Appends a new step of `kind` to the list; returns its id.
pub fn add_step(root: &mut Vec<Value>, path: &ListPath, kind: &str) -> Option<String> {
    let list = list_mut(root, path)?;
    let step = new_step(kind);
    let id = id_of(&step).to_string();
    list.push(step);
    Some(id)
}

/// Moves a step up (`delta` -1) or down (+1) within its list; false at the ends.
pub fn move_step(root: &mut Vec<Value>, path: &ListPath, id: &str, delta: i32) -> bool {
    let Some(list) = list_mut(root, path) else {
        return false;
    };
    let Some(index) = list.iter().position(|s| id_of(s) == id) else {
        return false;
    };
    let target = index as i32 + delta;
    if target < 0 || target as usize >= list.len() {
        return false;
    }
    list.swap(index, target as usize);
    true
}

pub fn remove_step(root: &mut Vec<Value>, path: &ListPath, id: &str) -> bool {
    let Some(list) = list_mut(root, path) else {
        return false;
    };
    let before = list.len();
    list.retain(|s| id_of(s) != id);
    list.len() != before
}

/// Sets a field of a step; `None` removes it. `when` is a field too.
pub fn set_field(root: &mut Vec<Value>, id: &str, key: &str, value: Option<Value>) -> bool {
    let Some(step) = find_mut(root, id) else {
        return false;
    };
    match value {
        Some(v) => {
            let changed = step.get(key) != Some(&v);
            step.insert(key.to_string(), v);
            changed
        }
        None => step.remove(key).is_some(),
    }
}

/// The text field `key` of a step ("" when absent).
pub fn text_of(step: &Value, key: &str) -> String {
    step.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// A nested object field (`target`) as text: `path` is the key inside it.
pub fn nested_text(step: &Value, object: &str, key: &str) -> String {
    step.get(object)
        .and_then(|o| o.get(key))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Sets `object.key` (creating the object); an empty `text` removes the key.
pub fn set_nested(
    root: &mut Vec<Value>,
    id: &str,
    object: &str,
    key: &str,
    value: Option<Value>,
) -> bool {
    let Some(step) = find_mut(root, id) else {
        return false;
    };
    let entry = step
        .entry(object.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(map) = entry.as_object_mut() else {
        return false;
    };
    match value {
        Some(v) => {
            let changed = map.get(key) != Some(&v);
            map.insert(key.to_string(), v);
            changed
        }
        None => map.remove(key).is_some(),
    }
}

/// The (column, expression) rows of a map field; `current` and absent maps are empty.
pub fn map_rows(step: &Value, field: &str) -> Vec<(String, String)> {
    step.get(field)
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn rows_to_map(rows: Vec<(String, String)>) -> Value {
    Value::Object(rows.into_iter().map(|(k, v)| (k, Value::String(v))).collect())
}

fn edit_map(
    root: &mut Vec<Value>,
    id: &str,
    field: &str,
    edit: impl FnOnce(&mut Vec<(String, String)>) -> bool,
) -> bool {
    let Some(step) = find_mut(root, id) else {
        return false;
    };
    let mut rows = map_rows(&Value::Object(step.clone()), field);
    if !edit(&mut rows) {
        return false;
    }
    step.insert(field.to_string(), rows_to_map(rows));
    true
}

/// Renames the column of row `index`; refused (false) for an empty or taken name.
pub fn map_rename(root: &mut Vec<Value>, id: &str, field: &str, index: usize, name: &str) -> bool {
    edit_map(root, id, field, |rows| {
        let taken = rows.iter().enumerate().any(|(i, (k, _))| i != index && k == name);
        match rows.get_mut(index) {
            Some((k, _)) if !name.is_empty() && !taken && k != name => {
                *k = name.to_string();
                true
            }
            _ => false,
        }
    })
}

/// Sets the expression of row `index`.
pub fn map_expr(root: &mut Vec<Value>, id: &str, field: &str, index: usize, expr: &str) -> bool {
    edit_map(root, id, field, |rows| match rows.get_mut(index) {
        Some((_, v)) if v != expr => {
            *v = expr.to_string();
            true
        }
        _ => false,
    })
}

/// Adds a row named `<key_label><n>` with the first free n (like the React editor).
pub fn map_add(root: &mut Vec<Value>, id: &str, field: &str, key_label: &str) -> bool {
    edit_map(root, id, field, |rows| {
        let base = key_label.to_lowercase();
        let mut n = rows.len() + 1;
        while rows.iter().any(|(k, _)| *k == format!("{base}{n}")) {
            n += 1;
        }
        rows.push((format!("{base}{n}"), String::new()));
        true
    })
}

pub fn map_remove(root: &mut Vec<Value>, id: &str, field: &str, index: usize) -> bool {
    edit_map(root, id, field, |rows| {
        if index < rows.len() {
            rows.remove(index);
            true
        } else {
            false
        }
    })
}

/// "2.then.0" -> "3.then.1": step paths are shown 1-based.
pub fn step_number(path: &str) -> String {
    path.split('.')
        .map(|part| match part.parse::<usize>() {
            Ok(n) => (n + 1).to_string(),
            Err(_) => part.to_string(),
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// An action with a fresh id, a numbered name and no steps.
pub fn new_action(number: usize) -> ActionDef {
    ActionDef {
        id: uuid::Uuid::new_v4().to_string(),
        name: format!("Action {number}"),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(step: &Value) -> String {
        text_of(step, "id")
    }

    #[test]
    fn new_steps_have_ids_and_the_defaults_of_their_kind() {
        for kind in STEP_KINDS {
            let step = new_step(kind);
            assert_eq!(step["kind"], kind);
            assert!(!id(&step).is_empty());
            let parsed: Step = serde_json::from_value(step).unwrap();
            assert_eq!(parsed.kind, kind);
        }
        assert_eq!(new_step("updateRecord")["match"], "current");
        assert_eq!(new_step("runQuery")["storeAs"], "rows");
        assert_eq!(new_step("confirm")["message"], "'Are you sure?'");
        assert_eq!(new_step("condition")["then"], json!([]));
    }

    #[test]
    fn steps_are_added_moved_and_removed_by_id_inside_branches() {
        let mut root = vec![];
        let a = add_step(&mut root, &ListPath::root(), "message").unwrap();
        let cond = add_step(&mut root, &ListPath::root(), "condition").unwrap();
        let then = ListPath::root().branch(&cond, Branch::Then);
        let inner = add_step(&mut root, &then, "fail").unwrap();
        let inner2 = add_step(&mut root, &then, "setField").unwrap();
        assert_eq!(root.len(), 2);
        assert_eq!(list_mut(&mut root, &then).unwrap().len(), 2);

        assert!(move_step(&mut root, &then, &inner2, -1));
        assert_eq!(id(&list_mut(&mut root, &then).unwrap()[0]), inner2);
        assert!(!move_step(&mut root, &then, &inner2, -1), "already first");
        assert!(move_step(&mut root, &ListPath::root(), &a, 1));
        assert_eq!(id(&root[1]), a);

        assert!(remove_step(&mut root, &then, &inner));
        assert_eq!(list_mut(&mut root, &then).unwrap().len(), 1);
        assert!(!remove_step(&mut root, &then, &inner));
        // Round trip through typed steps keeps the nesting.
        let steps = from_values(&root);
        assert_eq!(steps.len(), 2);
        assert_eq!(all_kinds(&steps), vec!["condition", "message", "setField"]);
    }

    fn all_kinds(steps: &[Step]) -> Vec<String> {
        let mut kinds: Vec<String> = crate::automation::all_steps(steps)
            .into_iter()
            .map(|s| s.kind)
            .collect();
        kinds.sort();
        kinds
    }

    #[test]
    fn fields_are_set_and_removed_and_report_whether_they_changed() {
        let mut root = vec![new_step("message")];
        let step = id(&root[0]);
        assert!(set_field(&mut root, &step, "text", Some(json!("'hi'"))));
        assert!(!set_field(&mut root, &step, "text", Some(json!("'hi'"))));
        assert!(set_field(&mut root, &step, "when", Some(json!("x = 1"))));
        assert_eq!(text_of(&root[0], "when"), "x = 1");
        assert!(set_field(&mut root, &step, "when", None));
        assert!(!set_field(&mut root, &step, "when", None));
        assert!(!set_field(&mut root, "missing", "text", Some(json!("x"))));
    }

    #[test]
    fn nested_target_fields_are_created_and_cleared() {
        let mut root = vec![new_step("navigate")];
        let step = id(&root[0]);
        assert!(set_nested(&mut root, &step, "target", "id", Some(json!("f1"))));
        assert!(set_nested(&mut root, &step, "target", "mode", Some(json!("edit"))));
        assert_eq!(nested_text(&root[0], "target", "mode"), "edit");
        assert!(set_nested(&mut root, &step, "target", "mode", None));
        assert_eq!(nested_text(&root[0], "target", "mode"), "");
        assert_eq!(nested_text(&root[0], "target", "kind"), "form");
    }

    #[test]
    fn value_maps_keep_their_order_and_refuse_bad_names() {
        let mut root = vec![new_step("createRecord")];
        let step = id(&root[0]);
        assert!(map_add(&mut root, &step, "values", "Column"));
        assert!(map_add(&mut root, &step, "values", "Column"));
        assert_eq!(
            map_rows(&root[0], "values"),
            vec![("column1".into(), "".into()), ("column2".into(), "".into())]
        );
        assert!(map_expr(&mut root, &step, "values", 0, "record.total"));
        assert!(map_rename(&mut root, &step, "values", 0, "total"));
        assert!(!map_rename(&mut root, &step, "values", 1, "total"), "taken");
        assert!(!map_rename(&mut root, &step, "values", 1, ""), "empty");
        assert_eq!(
            map_rows(&root[0], "values"),
            vec![
                ("total".into(), "record.total".into()),
                ("column2".into(), "".into())
            ]
        );
        assert!(map_remove(&mut root, &step, "values", 0));
        assert!(!map_remove(&mut root, &step, "values", 5));
        assert_eq!(map_rows(&root[0], "values").len(), 1);
    }

    #[test]
    fn step_paths_are_shown_one_based() {
        assert_eq!(step_number("0"), "1");
        assert_eq!(step_number("2.then.0"), "3.then.1");
    }
}
