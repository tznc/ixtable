//! Warnings for "run as signed-in user" triggers (port of `src/runtime/trigger-warnings.ts`).

use super::rbac::{Operation, can};
use crate::archive::{ActionKind, DocumentConfig};
use crate::automation::Step;
use std::collections::HashSet;

struct Needed {
    kind: &'static str,
    id: String,
    op: Operation,
}

fn op_word(op: Operation) -> &'static str {
    match op {
        Operation::Read => "read",
        Operation::Create => "create",
        Operation::Update => "update",
        Operation::Delete => "delete",
        Operation::Execute => "execute",
        Operation::View => "view",
    }
}

/// The action a `customAction` entity routes updates and deletes of `table` to.
fn routed_to(config: &DocumentConfig, table: &str) -> Option<String> {
    config
        .entities
        .iter()
        .find(|e| e.table == table && e.concurrency == "customAction")
        .and_then(|e| e.action_id.clone())
}

fn text(step: &Step, key: &str) -> Option<String> {
    step.fields.get(key)?.as_str().map(str::to_string)
}

fn branch(step: &Step, key: &str) -> Vec<Step> {
    step.fields
        .get(key)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Permissions a trigger's action needs, following branches, runAction calls and the custom
/// actions its updates and deletes are routed to (`trigger_auth.rs`).
fn needed(config: &DocumentConfig, action_id: &str) -> Vec<Needed> {
    let mut out = vec![Needed {
        kind: "action",
        id: action_id.into(),
        op: Operation::Execute,
    }];
    let mut seen = HashSet::new();
    let mut queue = vec![action_id.to_string()];
    while let Some(id) = queue.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let mut steps: Vec<Step> = config
            .actions
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.steps.clone())
            .unwrap_or_default();
        while let Some(step) = steps.pop() {
            let table = text(&step, "table").unwrap_or_default();
            let mut table_op = |op| {
                out.push(Needed {
                    kind: "table",
                    id: table.clone(),
                    op,
                })
            };
            match step.kind.as_str() {
                "condition" => {
                    steps.extend(branch(&step, "then"));
                    steps.extend(branch(&step, "else"));
                }
                "createRecord" => table_op(Operation::Create),
                "updateRecord" => table_op(Operation::Update),
                "deleteRecord" => table_op(Operation::Delete),
                "runQuery" => {
                    let query_id = text(&step, "queryId").unwrap_or_default();
                    let action = config
                        .saved_queries
                        .iter()
                        .find(|q| q.id == query_id)
                        .and_then(|q| q.action.as_ref());
                    // An action query needs the table operations it performs, not query read.
                    match action {
                        Some(action) => {
                            let ops: &[Operation] = match action.kind {
                                ActionKind::Insert => &[Operation::Create],
                                ActionKind::Update => &[Operation::Update],
                                ActionKind::Delete => &[Operation::Delete],
                                ActionKind::Replace => &[Operation::Delete, Operation::Create],
                            };
                            for op in ops {
                                out.push(Needed {
                                    kind: "table",
                                    id: action.table.clone(),
                                    op: *op,
                                });
                            }
                        }
                        None => out.push(Needed {
                            kind: "query",
                            id: query_id,
                            op: Operation::Read,
                        }),
                    }
                }
                "runAction" => {
                    let target = text(&step, "actionId").unwrap_or_default();
                    out.push(Needed {
                        kind: "action",
                        id: target.clone(),
                        op: Operation::Execute,
                    });
                    queue.push(target);
                }
                _ => {}
            }
            if matches!(step.kind.as_str(), "updateRecord" | "deleteRecord") {
                if let Some(custom) = routed_to(config, &table) {
                    out.push(Needed {
                        kind: "action",
                        id: custom.clone(),
                        op: Operation::Execute,
                    });
                    queue.push(custom);
                }
            }
        }
    }
    out
}

/// `op kind "name"`, naming actions and queries by their display name (ids are uuids).
fn describe(config: &DocumentConfig, n: &Needed) -> String {
    let name = match n.kind {
        "action" => config.actions.iter().find(|a| a.id == n.id).map(|a| a.name.clone()),
        "query" => config
            .saved_queries
            .iter()
            .find(|q| q.id == n.id)
            .map(|q| q.name.clone()),
        _ => None,
    };
    format!("{} {} \"{}\"", op_word(n.op), n.kind, name.unwrap_or_else(|| n.id.clone()))
}

#[derive(Clone, Debug, PartialEq)]
pub struct TriggerGap {
    pub trigger_id: String,
    pub name: String,
    pub missing: Vec<String>,
}

/// Enabled "run as signed-in user" triggers whose steps `role_id` cannot perform (Rust refuses
/// the initiating save for that role). Explicit grants only, so the warning may also list
/// access a form grant would imply.
pub fn user_trigger_gaps(config: &DocumentConfig, role_id: &str) -> Vec<TriggerGap> {
    config
        .triggers
        .iter()
        .filter(|t| t.enabled && t.runs_as_user())
        .filter_map(|t| {
            let mut missing: Vec<String> = vec![];
            for n in needed(config, &t.action_id) {
                if can(config, Some(role_id), n.kind, &n.id, n.op) {
                    continue;
                }
                let line = describe(config, &n);
                if !missing.contains(&line) {
                    missing.push(line);
                }
            }
            (!missing.is_empty()).then(|| TriggerGap {
                trigger_id: t.id.clone(),
                name: t.name.clone(),
                missing,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "trigger_warnings_tests.rs"]
mod tests;
