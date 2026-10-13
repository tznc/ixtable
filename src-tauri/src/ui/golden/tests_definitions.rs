//! Port of `tests/integration/golden/definitions.test.tsx`: the templates are listed, their
//! definitions validate, every expression parses and every saved query runs.

use super::journey::{
    code_of, try_saved_query, validation_errors, with_journey,
};
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;
use serde_json::{Value, json};

const FORM_ROOTS: &[&str] = &["record", "form", "app", "value"];
const REPORT_ROOTS: &[&str] = &["record", "rows", "params", "page", "pages"];
const DASHBOARD_ROOTS: &[&str] = &["rows", "params"];
const ACTION_ROOTS: &[&str] = &["record", "old", "form", "app", "params", "results", "steps"];
const TRIGGER_ROOTS: &[&str] = &["record", "old", "app", "trigger"];

struct Found {
    place: String,
    source: String,
    roots: &'static [&'static str],
}

fn add(out: &mut Vec<Found>, place: String, source: Option<&Value>, roots: &'static [&'static str]) {
    if let Some(s) = source.and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
        out.push(Found { place, source: s.into(), roots });
    }
}

fn list<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value.get(key).and_then(Value::as_array).map_or(&[], |v| v.as_slice())
}

fn s(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn steps(out: &mut Vec<Found>, action_id: &str, items: &[Value]) {
    for step in items {
        let place = format!("action {action_id} step {}", s(step, "id"));
        add(out, format!("{place} when"), step.get("when"), ACTION_ROOTS);
        for key in ["text", "message", "value", "recordId"] {
            add(out, place.clone(), step.get(key), ACTION_ROOTS);
        }
        for key in ["values", "params", "match"] {
            if let Some(map) = step.get(key).and_then(Value::as_object) {
                for (column, source) in map {
                    add(out, format!("{place} {column}"), Some(source), ACTION_ROOTS);
                }
            }
        }
        for branch in ["then", "else"] {
            steps(out, action_id, list(step, branch));
        }
    }
}

/// Every expression in the definitions, with the names it may use (same walk as the React test).
fn expressions(config: &Value) -> Vec<Found> {
    let mut out = vec![];
    for form in list(&config["design"], "forms") {
        let fid = s(form, "id");
        for rule in list(form, "rules") {
            add(&mut out, format!("form {fid} rule"), rule.get("expression"), FORM_ROOTS);
        }
        for c in list(form, "controls") {
            let place = format!("form {fid} control {}", s(c, "id"));
            for key in ["visibleWhen", "enabledWhen", "computed", "defaultValue"] {
                add(&mut out, format!("{place} {key}"), c.get(key), FORM_ROOTS);
            }
            add(&mut out, format!("{place} validation"), c.get("validation").and_then(|v| v.get("expression")), FORM_ROOTS);
        }
    }
    for report in list(config, "reports") {
        let rid = s(report, "id");
        let bands = &report["bands"];
        let mut all: Vec<&Value> = ["reportHeader", "pageHeader", "detail", "pageFooter", "reportFooter"]
            .iter()
            .map(|k| &bands[*k])
            .collect();
        for g in list(bands, "groups") {
            add(&mut out, format!("report {rid} group"), g.get("groupBy"), REPORT_ROOTS);
            all.push(&g["header"]);
            all.push(&g["footer"]);
        }
        for band in all {
            for c in list(band, "components") {
                add(&mut out, format!("report {rid} {}", s(c, "id")), c.get("expression"), REPORT_ROOTS);
                if s(c, "kind") == "table" {
                    for col in list(c, "columns") {
                        add(&mut out, format!("report {rid} {}", s(col, "id")), col.get("expression"), REPORT_ROOTS);
                    }
                }
            }
        }
    }
    for d in list(config, "dashboards") {
        let did = s(d, "id");
        for c in list(d, "components") {
            add(&mut out, format!("dashboard {did} {}", s(c, "id")), c.get("expression"), DASHBOARD_ROOTS);
            add(
                &mut out,
                format!("dashboard {did} {} comparison", s(c, "id")),
                c.get("comparison").and_then(|v| v.get("expression")),
                DASHBOARD_ROOTS,
            );
        }
    }
    for action in list(config, "actions") {
        steps(&mut out, &s(action, "id"), list(action, "steps"));
    }
    for t in list(config, "triggers") {
        let tid = s(t, "id");
        add(&mut out, format!("trigger {tid} condition"), t.get("condition"), TRIGGER_ROOTS);
        add(&mut out, format!("trigger {tid} key"), t.get("idempotencyKey"), TRIGGER_ROOTS);
    }
    out
}

#[gpui_kit::test]
fn golden_lists_the_three_golden_applications_on_the_start_screen(cx: &mut TestAppContext) {
    with_journey("templates", "start-screen", |journey| {
        let listed = journey.step("List templates", |_| crate::templates::list_templates().expect("templates"));
        journey.check(
            "templates",
            listed.iter().map(|t| (&t.id, &t.name, &t.version)).collect::<Vec<_>>(),
            [("crm", "CRM", "1.0.0"), ("inventory", "Inventory", "1.0.0"), ("work-orders", "Work orders", "1.0.0")],
        );
        journey.step("Create Inventory from the start screen", |journey| {
            let mut h = Harness::start(cx);
            h.click("template:inventory");
            assert!(h.has_shell());
            let state = h.state();
            journey.check(
                "new untitled document",
                (&state.name, &state.path, &state.active_mode),
                ("Inventory", None::<String>, "run"),
            );
            assert_eq!(h.mode(), ModeId::Run);
        });
    });
}

fn definitions_of(id: &str) {
    with_journey(id, "definitions", |journey| {
        let label = format!("golden-defs-{id}");
        journey.step("Create from template", |_| {
            crate::templates::create_from_template(label.clone(), id.into()).expect("create from template");
        });
        let config = crate::read_document_config(label.clone()).expect("config");
        journey.step("Validate document", |journey| {
            journey.check("no error issues", validation_errors(&label), Vec::<String>::new());
        });
        journey.step("Parse every expression", |journey| {
            let found = expressions(&serde_json::to_value(&config).expect("config json"));
            assert!(found.len() > 10, "only {} expressions", found.len());
            let problems: Vec<String> = found
                .iter()
                .flat_map(|e| {
                    crate::expr::check(&e.source, Some(e.roots))
                        .into_iter()
                        .map(move |d| format!("{}: {}: {}", e.place, e.source, d.message))
                })
                .collect();
            journey.check(&format!("{} expressions", found.len()), problems, Vec::<String>::new());
        });
        journey.step("Run every saved query", |journey| {
            let mut failures = vec![];
            for query in &config.saved_queries {
                let required: Vec<(&str, Value)> =
                    query.parameters.iter().filter(|p| p.required).map(|p| (p.name.as_str(), json!(1))).collect();
                if let Err(e) = try_saved_query(&label, &query.name, &required) {
                    failures.push(format!("{}: {}", query.name, e.message));
                }
            }
            journey.check(&format!("{} queries", config.saved_queries.len()), failures, Vec::<String>::new());
        });
        crate::close_document(label, true).expect("close");
    });
}

#[gpui_kit::test]
fn golden_crm_definitions_validate_expressions_parse_and_every_saved_query_runs(_cx: &mut TestAppContext) {
    definitions_of("crm");
}

#[gpui_kit::test]
fn golden_inventory_definitions_validate_expressions_parse_and_every_saved_query_runs(_cx: &mut TestAppContext) {
    definitions_of("inventory");
}

#[gpui_kit::test]
fn golden_work_orders_definitions_validate_expressions_parse_and_every_saved_query_runs(_cx: &mut TestAppContext) {
    definitions_of("work-orders");
}

#[gpui_kit::test]
fn golden_rejects_unknown_templates_without_leaving_a_document_open(_cx: &mut TestAppContext) {
    let label = "golden-missing-template".to_string();
    assert_eq!(code_of(crate::templates::create_from_template(label.clone(), "missing".into())), "NOT_FOUND");
    assert_eq!(code_of(crate::document_state(label)), "NO_DOCUMENT");
}
