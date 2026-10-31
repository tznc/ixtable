//! Repairs for shapes Access allows but ixtable does not: names that differ only by case,
//! and macros that run each other in a loop.
use crate::access::model::AccessDb;
use serde_json::Value;
use std::collections::BTreeMap;

/// Access names are case-insensitive, but a template can hold parts whose names differ
/// only by case. The later ones get a number so neither is lost: (kind, old, new).
pub fn rename_case_clashes(db: &mut AccessDb) -> Vec<(String, String, String)> {
    fn dedupe(kind: &str, names: Vec<&mut String>, out: &mut Vec<(String, String, String)>) {
        let mut seen = std::collections::BTreeSet::new();
        let taken: std::collections::BTreeSet<String> = names.iter().map(|n| n.to_lowercase()).collect();
        for name in names {
            if seen.insert(name.to_lowercase()) {
                continue;
            }
            let mut n = 2;
            let new = loop {
                let candidate = format!("{name} {n}");
                if !taken.contains(&candidate.to_lowercase()) && !seen.contains(&candidate.to_lowercase()) {
                    break candidate;
                }
                n += 1;
            };
            seen.insert(new.to_lowercase());
            out.push((kind.into(), std::mem::replace(name, new.clone()), new));
        }
    }
    let mut out = vec![];
    dedupe("query", db.queries.iter_mut().map(|q| &mut q.name).collect(), &mut out);
    dedupe("form", db.forms.iter_mut().map(|f| &mut f.name).collect(), &mut out);
    dedupe("report", db.reports.iter_mut().map(|r| &mut r.name).collect(), &mut out);
    dedupe("macro", db.macros.iter_mut().map(|m| &mut m.name).collect(), &mut out);
    out
}

/// runAction targets of an action's steps, at any depth.
fn run_targets(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            if o.get("kind").and_then(Value::as_str) == Some("runAction") {
                if let Some(id) = o.get("actionId").and_then(Value::as_str) {
                    out.push(id.to_string());
                }
            }
            o.values().for_each(|x| run_targets(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| run_targets(x, out)),
        _ => {}
    }
}

fn drop_run_steps(v: &mut Value, target: &str) {
    match v {
        Value::Array(a) => {
            a.retain(|x| {
                !(x.get("kind").and_then(Value::as_str) == Some("runAction")
                    && x.get("actionId").and_then(Value::as_str) == Some(target))
            });
            a.iter_mut().for_each(|x| drop_run_steps(x, target));
        }
        Value::Object(o) => o.values_mut().for_each(|x| drop_run_steps(x, target)),
        _ => {}
    }
}

/// Access macros may call each other in a loop; ixtable refuses such actions. Cuts the
/// call that closes each loop: (macro name, called macro name).
pub fn break_run_cycles(actions: &mut [Value]) -> Vec<(String, String)> {
    let id_of = |a: &Value| a.get("id").and_then(Value::as_str).unwrap_or("").to_string();
    let name_of = |actions: &[Value], id: &str| {
        actions
            .iter()
            .find(|a| id_of(a) == id)
            .and_then(|a| a.get("name").and_then(Value::as_str))
            .unwrap_or(id)
            .to_string()
    };
    let mut cut = vec![];
    loop {
        let edges: BTreeMap<String, Vec<String>> = actions
            .iter()
            .map(|a| {
                let mut t = vec![];
                run_targets(a.get("steps").unwrap_or(&Value::Null), &mut t);
                (id_of(a), t)
            })
            .collect();
        let reaches = |from: &str, to: &str| {
            let mut stack = vec![from.to_string()];
            let mut seen = std::collections::BTreeSet::new();
            while let Some(n) = stack.pop() {
                if n == to {
                    return true;
                }
                if seen.insert(n.clone()) {
                    stack.extend(edges.get(&n).cloned().unwrap_or_default());
                }
            }
            false
        };
        let closing = edges
            .iter()
            .find_map(|(a, ts)| ts.iter().find(|b| reaches(b, a)).map(|b| (a.clone(), b.clone())));
        let Some((a, b)) = closing else { break };
        cut.push((name_of(actions, &a), name_of(actions, &b)));
        if let Some(action) = actions.iter_mut().find(|x| id_of(x) == a) {
            if let Some(steps) = action.get_mut("steps") {
                drop_run_steps(steps, &b);
            }
        }
    }
    cut
}

