//! App navigation for an imported database.
//!
//! An Access navigation form's buttons give the menu, in their order. Without
//! one, list forms and stand-alone forms are listed, reports go in a group, and
//! the form an AutoExec macro (or the Display Form option) opens is the start page.
use super::forms::Context;
use super::schema::{TablePlan, TableRole};
use serde_json::{json, Value};

pub struct Navigation {
    pub items: Vec<Value>,
    pub start_page: Option<String>,
}

fn id() -> String {
    uuid::Uuid::now_v7().to_string()
}

fn form_item(form: &Value) -> Value {
    json!({ "id": id(), "label": form["name"], "kind": "form", "targetId": form["id"] })
}

pub fn build(ctx: &Context, forms: &[Value], reports: &[Value], plans: &[TablePlan]) -> Navigation {
    let form_by_name = |n: &str| {
        forms.iter().find(|f| {
            f["name"]
                .as_str()
                .is_some_and(|x| x.eq_ignore_ascii_case(n))
        })
    };
    let report_by_name = |n: &str| {
        reports.iter().find(|r| {
            r["name"]
                .as_str()
                .is_some_and(|x| x.eq_ignore_ascii_case(n))
        })
    };
    // Forms other forms open or embed are reached through them.
    let opened: Vec<&str> = forms
        .iter()
        .filter_map(|f| f["detailFormId"].as_str())
        .collect();
    let mut items = vec![];
    for h in &ctx.nav_hints {
        let item = match h.kind.as_str() {
            "report" => report_by_name(&h.target).map(
                |r| json!({ "id": id(), "label": h.label, "kind": "report", "targetId": r["id"] }),
            ),
            _ => form_by_name(&h.target).map(
                |f| json!({ "id": id(), "label": h.label, "kind": "form", "targetId": f["id"] }),
            ),
        };
        if let Some(i) =
            item.filter(|i| !items.iter().any(|x: &Value| x["targetId"] == i["targetId"]))
        {
            items.push(i);
        }
    }
    if items.is_empty() {
        for f in forms {
            let name = f["name"].as_str().unwrap_or("").to_lowercase();
            let fid = f["id"].as_str().unwrap_or("");
            let list = f["modes"] == json!(["list"]);
            let standalone = f.get("source").is_none() && !ctx.subforms.contains(&name);
            if (list && !ctx.subforms.contains(&name)) || (standalone && !opened.contains(&fid)) {
                items.push(form_item(f));
            }
        }
        if !reports.is_empty() {
            let children: Vec<Value> = reports.iter().map(|r| json!({ "id": id(), "label": r["name"], "kind": "report", "targetId": r["id"] })).collect();
            items.push(
                json!({ "id": id(), "label": "Reports", "kind": "group", "children": children }),
            );
        }
    }
    if forms.is_empty() || items.is_empty() {
        let tables: Vec<Value> = plans
            .iter()
            .filter(|p| p.role == TableRole::Main)
            .map(|p| json!({ "id": id(), "label": p.name, "kind": "table", "targetId": p.name }))
            .collect();
        if !tables.is_empty() {
            items.push(
                json!({ "id": id(), "label": "Tables", "kind": "group", "children": tables }),
            );
        }
    }
    let start = ctx
        .start_form
        .clone()
        .or_else(|| ctx.db.props.get("StartUpForm").cloned());
    let mut start_page = None;
    if let Some(f) = start.as_deref().and_then(form_by_name) {
        match items.iter().find(|i| i["targetId"] == f["id"]) {
            Some(i) => start_page = i["id"].as_str().map(str::to_string),
            None => {
                let i = form_item(f);
                start_page = i["id"].as_str().map(str::to_string);
                items.insert(0, i);
            }
        }
    }
    if start_page.is_none() {
        start_page = items
            .iter()
            .find(|i| i["kind"] != "group")
            .and_then(|i| i["id"].as_str())
            .map(str::to_string);
    }
    Navigation { items, start_page }
}
