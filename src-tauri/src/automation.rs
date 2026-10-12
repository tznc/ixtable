//! Declarative actions and record triggers (PRD §17.2, §17.3).
//!
//! Rust stores and validates definitions; it never evaluates expressions (the
//! frontend runner in src/automation/runner.ts does). The durable async queue
//! lives in jobs.rs.
use crate::archive::{check_named_ids, DocumentConfig, Issue};
use crate::manager::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OnError {
    #[default]
    Stop,
    Continue,
    Rollback,
}

/// One action step. `kind` selects the step type; its fields (table, values,
/// match, queryId, …) stay as JSON so new step fields round-trip untouched.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    #[serde(default)]
    pub id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    #[serde(flatten)]
    pub fields: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActionDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub on_error: OnError,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TriggerEvent {
    #[default]
    Created,
    Updated,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TriggerMode {
    #[default]
    Sync,
    Async,
}

fn default_true() -> bool {
    true
}
fn default_attempts() -> u32 {
    3
}
fn default_backoff() -> u64 {
    1000
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Trigger {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub table: String,
    #[serde(default)]
    pub event: TriggerEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(default)]
    pub action_id: String,
    #[serde(default)]
    pub mode: TriggerMode,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default = "default_attempts")]
    pub max_attempts: u32,
    #[serde(default = "default_backoff")]
    pub backoff_ms: u64,
    /// Execution identity: "app" (default, see trigger_auth.rs) or "user".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_as: Option<String>,
}
impl Trigger {
    /// Whether the trigger's steps run under the signed-in user's role.
    pub fn runs_as_user(&self) -> bool {
        self.run_as.as_deref() == Some("user")
    }
}
impl Default for Trigger {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            table: String::new(),
            event: TriggerEvent::Created,
            condition: None,
            action_id: String::new(),
            mode: TriggerMode::Sync,
            enabled: true,
            idempotency_key: None,
            max_attempts: default_attempts(),
            backoff_ms: default_backoff(),
            run_as: None,
        }
    }
}

const STEP_KINDS: &[&str] = &[
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
];

fn text<'a>(fields: &'a Map<String, Value>, key: &str) -> &'a str {
    fields.get(key).and_then(Value::as_str).unwrap_or("")
}

/// All steps of an action, flattened (condition branches included), as owned values.
pub fn all_steps(steps: &[Step]) -> Vec<Step> {
    let mut out = vec![];
    let mut queue: Vec<Step> = steps.to_vec();
    while let Some(step) = queue.pop() {
        for branch in ["then", "else"] {
            if let Some(Value::Array(items)) = step.fields.get(branch) {
                for item in items {
                    if let Ok(nested) = serde_json::from_value::<Step>(item.clone()) {
                        queue.push(nested);
                    }
                }
            }
        }
        out.push(step);
    }
    out
}

/// Action ids each action calls through runAction steps.
fn call_graph(config: &DocumentConfig) -> HashMap<String, Vec<String>> {
    config
        .actions
        .iter()
        .map(|a| {
            let calls = all_steps(&a.steps)
                .into_iter()
                .filter(|s| s.kind == "runAction")
                .map(|s| text(&s.fields, "actionId").to_string())
                .filter(|id| !id.is_empty())
                .collect();
            (a.id.clone(), calls)
        })
        .collect()
}

/// Actions that take part in a runAction cycle, each with one cycle path.
pub fn cycles(config: &DocumentConfig) -> Vec<(String, Vec<String>)> {
    let graph = call_graph(config);
    let mut found = vec![];
    for start in config.actions.iter().map(|a| &a.id) {
        // DFS from `start`, looking for a path back to `start`.
        let mut stack = vec![(start.clone(), vec![start.clone()])];
        let mut seen = HashSet::new();
        while let Some((node, path)) = stack.pop() {
            for next in graph.get(&node).into_iter().flatten() {
                if next == start {
                    let mut cycle = path.clone();
                    cycle.push(next.clone());
                    found.push((start.clone(), cycle));
                    stack.clear();
                    break;
                }
                if seen.insert(next.clone()) {
                    let mut p = path.clone();
                    p.push(next.clone());
                    stack.push((next.clone(), p));
                }
            }
        }
    }
    found
}

fn check_step(config: &DocumentConfig, action: &ActionDef, step: &Step, issues: &mut Vec<Issue>) {
    let err = |issues: &mut Vec<Issue>, msg: String| {
        issues.push(Issue::error(
            "action",
            &action.id,
            format!("action \"{}\": {} step: {msg}", action.name, step.kind),
        ))
    };
    let f = &step.fields;
    if !STEP_KINDS.contains(&step.kind.as_str()) {
        return err(issues, "unknown step kind".into());
    }
    if step.kind == "condition" && step.when.as_deref().map(str::trim).unwrap_or("").is_empty() {
        err(issues, "condition expression is empty".into());
    }
    let expr_map = |issues: &mut Vec<Issue>, key: &str, required: bool| match f.get(key) {
        Some(Value::Object(map)) => {
            if required && map.is_empty() {
                err(issues, format!("{key} has no columns"));
            }
            for (col, v) in map {
                if v.as_str().map(str::trim).unwrap_or("").is_empty() {
                    err(issues, format!("{key}.{col} expression is empty"));
                }
            }
        }
        Some(Value::String(s)) if key == "match" && s == "current" => {}
        None if !required => {}
        _ => err(issues, format!("{key} is missing")),
    };
    let need = |issues: &mut Vec<Issue>, key: &str, what: &str| {
        if text(f, key).trim().is_empty() {
            err(issues, format!("{what} is empty"));
        }
    };
    let forms: HashSet<&str> = config.design.forms.iter().map(|x| x.id.as_str()).collect();
    let reports: HashSet<&str> = config.reports.iter().map(|x| x.id.as_str()).collect();
    let dashboards: HashSet<&str> = config.dashboards.iter().map(|x| x.id.as_str()).collect();
    let missing = |issues: &mut Vec<Issue>, kind: &str, id: &str, set: &HashSet<&str>| {
        if id.is_empty() {
            err(issues, format!("no {kind} selected"));
        } else if !set.contains(id) {
            err(issues, format!("{kind} {id} does not exist"));
        }
    };
    match step.kind.as_str() {
        "createRecord" => {
            need(issues, "table", "table");
            expr_map(issues, "values", true);
        }
        "updateRecord" => {
            need(issues, "table", "table");
            expr_map(issues, "match", true);
            expr_map(issues, "values", true);
        }
        "deleteRecord" => {
            need(issues, "table", "table");
            expr_map(issues, "match", true);
        }
        "runQuery" => {
            let id = text(f, "queryId");
            let set = config.saved_queries.iter().map(|q| q.id.as_str()).collect();
            missing(issues, "query", id, &set);
            expr_map(issues, "params", false);
            let action_query = config
                .saved_queries
                .iter()
                .any(|q| q.id == id && q.action.is_some());
            if action_query && action.on_error == OnError::Rollback {
                err(issues, "an action query writes at once, so it cannot run in an action that rolls back on error".into());
            }
        }
        "navigate" => {
            let target = f.get("target").and_then(Value::as_object);
            let kind = target.map(|t| text(t, "kind")).unwrap_or("");
            let id = target.map(|t| text(t, "id")).unwrap_or("");
            match kind {
                "form" => missing(issues, "form", id, &forms),
                "report" => missing(issues, "report", id, &reports),
                "dashboard" => missing(issues, "dashboard", id, &dashboards),
                "table" if !id.is_empty() => {}
                _ => err(issues, "navigation target is not set".into()),
            }
        }
        "openForm" => missing(issues, "form", text(f, "formId"), &forms),
        "openReport" => {
            missing(issues, "report", text(f, "reportId"), &reports);
            expr_map(issues, "params", false);
        }
        "openDashboard" => {
            missing(issues, "dashboard", text(f, "dashboardId"), &dashboards);
            expr_map(issues, "params", false);
        }
        "setState" => {
            need(issues, "key", "state key");
            need(issues, "value", "value expression");
            if !matches!(text(f, "scope"), "app" | "form") {
                err(issues, "scope must be app or form".into());
            }
        }
        "confirm" => need(issues, "message", "message expression"),
        "message" => need(issues, "text", "text expression"),
        "fail" => need(issues, "message", "message expression"),
        "runAction" => {
            let set = config.actions.iter().map(|a| a.id.as_str()).collect();
            missing(issues, "action", text(f, "actionId"), &set);
        }
        _ => {}
    }
}

/// Config-only checks: ids, step fields, references to forms/reports/dashboards/
/// queries/actions, runAction cycles, and trigger settings. Table existence needs
/// the database: see `validate_tables`.
pub fn validate(config: &DocumentConfig) -> Vec<Issue> {
    let mut issues = check_named_ids(
        "action",
        config
            .actions
            .iter()
            .map(|a| (a.id.as_str(), a.name.as_str())),
    );
    issues.extend(check_named_ids(
        "trigger",
        config
            .triggers
            .iter()
            .map(|t| (t.id.as_str(), t.name.as_str())),
    ));
    for action in &config.actions {
        if action.steps.is_empty() {
            issues.push(Issue::warning(
                "action",
                &action.id,
                format!("action \"{}\" has no steps", action.name),
            ));
        }
        for step in all_steps(&action.steps) {
            check_step(config, action, &step, &mut issues);
        }
    }
    let names: HashMap<&str, &str> = config
        .actions
        .iter()
        .map(|a| (a.id.as_str(), a.name.as_str()))
        .collect();
    for (id, path) in cycles(config) {
        let path: Vec<&str> = path
            .iter()
            .map(|p| names.get(p.as_str()).copied().unwrap_or(p))
            .collect();
        issues.push(Issue::error(
            "action",
            &id,
            format!("recursive runAction cycle: {}", path.join(" → ")),
        ));
    }
    for t in &config.triggers {
        let mut err = |m: String| {
            issues.push(Issue::error(
                "trigger",
                &t.id,
                format!("trigger \"{}\": {m}", t.name),
            ))
        };
        if t.table.trim().is_empty() {
            err("no table selected".into());
        }
        if t.action_id.is_empty() {
            err("no action selected".into());
        } else if !names.contains_key(t.action_id.as_str()) {
            err(format!("action {} does not exist", t.action_id));
        }
        if !matches!(t.run_as.as_deref(), None | Some("app") | Some("user")) {
            err("run as must be app or user".into());
        }
        if t.max_attempts == 0 {
            err("max attempts must be at least 1".into());
        }
        if t.condition.as_deref().is_some_and(|c| c.trim().is_empty()) {
            err("condition expression is empty".into());
        }
        if t.idempotency_key
            .as_deref()
            .is_some_and(|c| c.trim().is_empty())
        {
            err("idempotency key expression is empty".into());
        }
    }
    issues
}

/// Flags actions and triggers that name tables missing from `tables`.
pub fn validate_tables(config: &DocumentConfig, tables: &[String]) -> Vec<Issue> {
    let known: HashSet<&str> = tables.iter().map(String::as_str).collect();
    let mut issues = vec![];
    for action in &config.actions {
        for step in all_steps(&action.steps) {
            let table = text(&step.fields, "table");
            if !table.is_empty() && !known.contains(table) {
                issues.push(Issue::error(
                    "action",
                    &action.id,
                    format!("action \"{}\": table {table} does not exist", action.name),
                ));
            }
        }
    }
    for t in &config.triggers {
        if !t.table.is_empty() && !known.contains(t.table.as_str()) {
            issues.push(Issue::error(
                "trigger",
                &t.id,
                format!("trigger \"{}\": table {} does not exist", t.name, t.table),
            ));
        }
    }
    issues
}

/// Automation issues for the open document, including table existence.
#[tauri::command]
pub fn validate_automation(window_label: String) -> Result<Vec<Issue>, AppError> {
    let m = crate::manager()?;
    let config = m.config(&window_label)?;
    let tables: Vec<String> = m
        .database_objects(&window_label)?
        .into_iter()
        .map(|o| o.name)
        .collect();
    let mut issues = validate(&config);
    issues.extend(validate_tables(&config, &tables));
    Ok(issues)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config(actions: Value, triggers: Value) -> DocumentConfig {
        let mut c = crate::archive::create_document("t").unwrap().config;
        c.actions = serde_json::from_value(actions).unwrap();
        c.triggers = serde_json::from_value(triggers).unwrap();
        c
    }
    fn messages(issues: &[Issue]) -> Vec<String> {
        issues.iter().map(|i| i.message.clone()).collect()
    }

    #[test]
    fn round_trips_the_frontend_shape() {
        let action = json!({
            "id": "a1", "name": "Close", "description": "", "onError": "rollback",
            "steps": [
                {"id": "s1", "kind": "updateRecord", "table": "orders", "match": "current",
                 "values": {"status": "'closed'"}, "when": "record.status = 'open'"},
                {"id": "s2", "kind": "condition", "when": "true",
                 "then": [{"id": "s3", "kind": "message", "text": "'ok'", "tone": "info"}], "else": []}
            ]
        });
        let parsed: ActionDef = serde_json::from_value(action.clone()).unwrap();
        assert_eq!(parsed.on_error, OnError::Rollback);
        assert_eq!(
            parsed.steps[0].when.as_deref(),
            Some("record.status = 'open'")
        );
        assert_eq!(serde_json::to_value(&parsed).unwrap(), action);
        let t: Trigger = serde_json::from_value(json!({"id": "t", "name": "T"})).unwrap();
        assert_eq!((t.enabled, t.max_attempts, t.backoff_ms), (true, 3, 1000));
        assert_eq!(all_steps(&parsed.steps).len(), 3);
    }

    #[test]
    fn open_dashboard_params_are_expressions() {
        let c = config(
            json!([{"id": "a1", "name": "Go", "steps": [
                {"id": "1", "kind": "openDashboard", "dashboardId": "none", "params": {"region": " "}}
            ]}]),
            json!([]),
        );
        let all = messages(&validate(&c)).join("\n");
        assert!(all.contains("params.region expression is empty"), "{all}");
        assert!(all.contains("dashboard none does not exist"), "{all}");
    }

    #[test]
    fn accepts_popup_open_form_and_close_form_steps() {
        let mut c = config(json!([]), json!([]));
        let form = c.design.forms[0].id.clone();
        c.actions = serde_json::from_value(json!([{
            "id": "a1", "name": "Pick", "steps": [
                {"id": "1", "kind": "openForm", "formId": form, "popup": true, "storeAs": "picked"},
                {"id": "2", "kind": "closeForm", "value": "results.picked"},
                {"id": "3", "kind": "closeForm"}
            ]
        }]))
        .unwrap();
        let all = messages(&validate(&c));
        assert!(all.is_empty(), "{all:?}");
    }

    #[test]
    fn validates_steps_references_and_triggers() {
        let c = config(
            json!([{
                "id": "a1", "name": "Bad", "steps": [
                    {"id": "1", "kind": "createRecord", "table": "", "values": {"x": " "}},
                    {"id": "2", "kind": "runQuery", "queryId": "nope", "params": {}, "storeAs": "q"},
                    {"id": "3", "kind": "openForm", "formId": "missing"},
                    {"id": "4", "kind": "runAction", "actionId": "ghost"},
                    {"id": "5", "kind": "condition", "then": [{"id": "6", "kind": "confirm", "message": ""}], "else": []},
                    {"id": "7", "kind": "teleport"},
                    {"id": "8", "kind": "setState", "scope": "global", "key": "k", "value": "1"},
                    {"id": "9", "kind": "fail", "message": " "}
                ]
            }, {"id": "a2", "name": "Empty", "steps": []}]),
            json!([
                {"id": "t1", "name": "T", "table": "", "event": "created", "actionId": "zzz", "mode": "async", "maxAttempts": 0, "condition": "", "runAs": "root"}
            ]),
        );
        let all = messages(&validate(&c));
        for expected in [
            "createRecord step: table is empty",
            "createRecord step: values.x expression is empty",
            "query nope does not exist",
            "form missing does not exist",
            "action ghost does not exist",
            "condition step: condition expression is empty",
            "confirm step: message expression is empty",
            "teleport step: unknown step kind",
            "fail step: message expression is empty",
            "scope must be app or form",
            "action \"Empty\" has no steps",
            "trigger \"T\": no table selected",
            "trigger \"T\": action zzz does not exist",
            "max attempts must be at least 1",
            "trigger \"T\": condition expression is empty",
            "run as must be app or user",
        ] {
            assert!(
                all.iter().any(|m| m.contains(expected)),
                "missing {expected:?} in {all:#?}"
            );
        }
    }

    #[test]
    fn detects_run_action_cycles_through_conditions() {
        let c = config(
            json!([
                {"id": "a", "name": "A", "steps": [{"id": "1", "kind": "runAction", "actionId": "b"}]},
                {"id": "b", "name": "B", "steps": [{"id": "2", "kind": "condition", "when": "true",
                    "then": [{"id": "3", "kind": "runAction", "actionId": "a"}], "else": []}]},
                {"id": "c", "name": "C", "steps": [{"id": "4", "kind": "runAction", "actionId": "a"}]}
            ]),
            json!([]),
        );
        let cyc: Vec<String> = validate(&c)
            .into_iter()
            .filter(|i| i.message.contains("cycle"))
            .map(|i| format!("{}: {}", i.object_id, i.message))
            .collect();
        assert_eq!(
            cyc,
            vec![
                "a: recursive runAction cycle: A → B → A",
                "b: recursive runAction cycle: B → A → B"
            ]
        );
    }

    #[test]
    fn valid_config_has_no_errors_and_tables_are_checked() {
        let c = config(
            json!([{"id": "a", "name": "A", "steps": [
                {"id": "1", "kind": "createRecord", "table": "audit", "values": {"m": "'x'"}},
                {"id": "2", "kind": "deleteRecord", "table": "orders", "match": "current"},
                {"id": "3", "kind": "fail", "message": "'Over the credit limit'", "when": "false"}
            ]}]),
            json!([{"id": "t", "name": "T", "table": "orders", "actionId": "a"}]),
        );
        assert!(validate(&c).is_empty(), "{:#?}", validate(&c));
        let missing = validate_tables(&c, &["orders".into()]);
        assert_eq!(
            messages(&missing),
            vec!["action \"A\": table audit does not exist"]
        );
        assert!(validate_tables(&c, &["orders".into(), "audit".into()]).is_empty());
    }
}
