//! Expression diagnostics for the whole document (PRD 17.1; port of
//! `src/shell/expressionIssues.ts`). Walks every expression a definition stores and runs
//! `check` with the same names its editor uses, so Problems, bundle export and cloud
//! publish see exactly what the field editors flag.

use super::{AutomationTab, RevealTarget};
use crate::archive::DocumentConfig;
use crate::automation::Step;
use crate::design::{ControlKind, Form, SourceKind};
use crate::forms::{conditions::filter_names, state::known_names};
use crate::report_engine::model::REPORT_SCOPE_NAMES;
use crate::reports::{Band, Report};
use crate::ui::automation::AUTOMATION_SCOPE_NAMES;
use serde_json::{Map, Value};

/// An expression that does not check, with the object and field that hold it.
#[derive(Clone, Debug, PartialEq)]
pub struct ExpressionIssue {
    pub object_kind: String,
    pub object_id: String,
    pub object_name: String,
    /// Field path in the editor's words, e.g. `Control "Total" › Visible when`.
    pub field: String,
    pub message: String,
    pub target: RevealTarget,
}

/// Column names of a table, or None when unknown (any `record.x` is then accepted).
pub type ColumnLookup<'a> = &'a dyn Fn(Option<&str>) -> Option<Vec<String>>;

const DASHBOARD_NAMES: [&str; 2] = ["params", "app"];
const DASHBOARD_ROW_NAMES: [&str; 3] = ["record", "params", "app"];
const DASHBOARD_KPI_NAMES: [&str; 3] = ["rows", "params", "app"];

#[derive(Clone)]
struct Owner {
    kind: &'static str,
    id: String,
    name: String,
    target: RevealTarget,
}

struct Collector<'a> {
    issues: Vec<ExpressionIssue>,
    columns: ColumnLookup<'a>,
}

impl Collector<'_> {
    fn add<S: AsRef<str>>(&mut self, owner: &Owner, field: &str, src: Option<&str>, names: &[S]) {
        let Some(src) = src.filter(|s| !s.trim().is_empty()) else {
            return;
        };
        let names: Vec<&str> = names.iter().map(AsRef::as_ref).collect();
        let diagnostics = crate::expr::check(src, Some(&names));
        if diagnostics.is_empty() {
            return;
        }
        self.issues.push(ExpressionIssue {
            object_kind: owner.kind.into(),
            object_id: owner.id.clone(),
            object_name: owner.name.clone(),
            field: field.into(),
            message: diagnostics.iter().map(|d| d.message.as_str()).collect::<Vec<_>>().join("; "),
            target: owner.target.clone(),
        });
    }

    /// A map of name to expression, one issue per entry.
    fn map<S: AsRef<str>>(&mut self, owner: &Owner, field: &str, values: Option<&Map<String, Value>>, names: &[S]) {
        for (key, src) in values.into_iter().flatten() {
            self.add(owner, &format!("{field} {key}"), src.as_str(), names);
        }
    }

    fn columns_of(&self, table: Option<&str>) -> Vec<String> {
        (self.columns)(table).unwrap_or_default()
    }
}

fn control_name(label: &str, column: Option<&str>, kind: &str) -> String {
    let shown = if !label.is_empty() {
        label
    } else {
        column.filter(|c| !c.is_empty()).unwrap_or(kind)
    };
    format!("Control \"{shown}\"")
}

fn kind_key(kind: ControlKind) -> String {
    serde_json::to_value(kind).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

fn walk_form(c: &mut Collector, form: &Form) {
    let design = |element: Option<String>| RevealTarget::Design { object_id: form.id.clone(), element_id: element };
    let owner = Owner { kind: "form", id: form.id.clone(), name: form.name.clone(), target: design(None) };
    let table = form.source.as_ref().filter(|s| s.kind == SourceKind::Table).and_then(|s| s.table.as_deref());
    let columns = c.columns_of(table);
    let names = known_names(&columns);
    if let Some(source) = form.source.as_ref().filter(|s| s.kind == SourceKind::Query) {
        for (key, src) in &source.params {
            c.add(&owner, &format!("Query parameter {key}"), Some(src), &["app", "params"]);
        }
    }
    c.add(&owner, "Filter", form.filter.as_deref(), &filter_names(&columns));
    for (i, rule) in form.rules.iter().enumerate() {
        c.add(&owner, &format!("Rule {}", i + 1), Some(&rule.expression), &names);
    }
    for control in &form.controls {
        let at = Owner { target: design(Some(control.id.clone())), ..owner.clone() };
        let label = control_name(
            &control.label,
            control.binding.as_ref().map(|b| b.column.as_str()),
            &kind_key(control.kind),
        );
        let field = |what: &str| format!("{label} › {what}");
        c.add(&at, &field("Visible when"), control.visible_when.as_deref(), &names);
        c.add(&at, &field("Enabled when"), control.enabled_when.as_deref(), &names);
        c.add(&at, &field("Computed value"), control.computed.as_deref(), &names);
        c.add(&at, &field("Default value"), control.default_value.as_deref(), &names);
        c.add(&at, &field("Validation"), control.validation.expression.as_deref(), &names);
        if let Some(relationship) = &control.relationship {
            let lookup = filter_names(&c.columns_of(Some(&relationship.table)));
            c.add(&at, &field("Lookup filter"), relationship.filter.as_deref(), &lookup);
        }
        if let Some(related) = &control.related {
            let lookup = filter_names(&c.columns_of(Some(&related.table)));
            c.add(&at, &field("Related list filter"), related.filter.as_deref(), &lookup);
        }
        for (i, style) in control.styles.iter().enumerate() {
            c.add(&at, &field(&format!("Style {}", i + 1)), Some(&style.when), &names);
        }
    }
}

fn walk_band(c: &mut Collector, report: &Report, band: &Band, band_name: &str) {
    for component in &band.components {
        let owner = Owner {
            kind: "report",
            id: report.id.clone(),
            name: report.name.clone(),
            target: RevealTarget::Reports { object_id: report.id.clone(), element_id: Some(component.id.clone()) },
        };
        let field = format!(
            "{band_name} › {}",
            match component.kind.as_str() {
                "table" => "Table",
                "field" => "Field",
                _ => "Calculated value",
            }
        );
        let text = |key: &str| component.extra.get(key).and_then(Value::as_str);
        if matches!(component.kind.as_str(), "field" | "calculated") {
            c.add(&owner, &field, text("expression"), &REPORT_SCOPE_NAMES);
        }
        if component.kind == "table" {
            for column in component.extra.get("columns").and_then(Value::as_array).into_iter().flatten() {
                let header = column.get("header").and_then(Value::as_str).unwrap_or("");
                let expression = column.get("expression").and_then(Value::as_str);
                c.add(&owner, &format!("{field} column \"{header}\""), expression, &REPORT_SCOPE_NAMES);
            }
        }
    }
}

fn walk_report(c: &mut Collector, report: &Report) {
    let bands = &report.bands;
    let owner = Owner {
        kind: "report",
        id: report.id.clone(),
        name: report.name.clone(),
        target: RevealTarget::Reports { object_id: report.id.clone(), element_id: None },
    };
    walk_band(c, report, &bands.report_header, "Report header");
    walk_band(c, report, &bands.page_header, "Page header");
    for (i, group) in bands.groups.iter().enumerate() {
        let n = i + 1;
        c.add(&owner, &format!("Group {n} › Group by"), Some(&group.group_by), &REPORT_SCOPE_NAMES);
        walk_band(c, report, &group.header, &format!("Group {n} header"));
        walk_band(c, report, &group.footer, &format!("Group {n} footer"));
    }
    walk_band(c, report, &bands.detail, "Detail");
    walk_band(c, report, &bands.page_footer, "Page footer");
    walk_band(c, report, &bands.report_footer, "Report footer");
}

fn walk_dashboard(c: &mut Collector, dashboard: &crate::pages::Page) {
    for component in &dashboard.components {
        let owner = Owner {
            kind: "page",
            id: dashboard.id.clone(),
            name: dashboard.name.clone(),
            target: RevealTarget::Page {
                object_id: dashboard.id.clone(),
                element_id: Some(component.id.clone()),
            },
        };
        let shown = if component.title.is_empty() {
            serde_json::to_value(component.kind).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
        } else {
            component.title.clone()
        };
        let label = format!("Component \"{shown}\"");
        let field = |what: &str| format!("{label} › {what}");
        c.add(&owner, &field("Value"), component.expression.as_deref(), &DASHBOARD_KPI_NAMES);
        let comparison = component.comparison.as_ref().and_then(|k| k.expression.as_deref());
        c.add(&owner, &field("Comparison"), comparison, &DASHBOARD_KPI_NAMES);
        c.add(&owner, &field("Record id"), component.record_id.as_deref(), &DASHBOARD_KPI_NAMES);
        c.add(&owner, &field("Filter"), component.filter.as_deref(), &DASHBOARD_ROW_NAMES);
        let mut style_names: Vec<&str> = DASHBOARD_ROW_NAMES.to_vec();
        style_names.push("value");
        for (i, style) in component.styles.iter().enumerate() {
            c.add(&owner, &field(&format!("Style {}", i + 1)), Some(&style.when), &style_names);
        }
        c.add(&owner, &field("Visible when"), component.visible_when.as_deref(), &DASHBOARD_NAMES);
        c.add(&owner, &field("Enabled when"), component.enabled_when.as_deref(), &DASHBOARD_NAMES);
    }
}

fn steps_of(value: Option<&Value>) -> Vec<Step> {
    value
        .and_then(Value::as_array)
        .map(|steps| steps.iter().filter_map(|s| serde_json::from_value(s.clone()).ok()).collect())
        .unwrap_or_default()
}

fn walk_steps(c: &mut Collector, owner: &Owner, steps: &[Step], path: &str) {
    let names = AUTOMATION_SCOPE_NAMES;
    for (i, step) in steps.iter().enumerate() {
        let at = format!("{path}Step {}", i + 1);
        let when = if step.kind == "condition" { "If" } else { "Only when" };
        c.add(owner, &format!("{at} › {when}"), step.when.as_deref(), &names);
        let get = |key: &str| step.fields.get(key);
        let text = |key: &str| get(key).and_then(Value::as_str);
        let object = |key: &str| get(key).and_then(Value::as_object);
        match step.kind.as_str() {
            "createRecord" => c.map(owner, &format!("{at} › Value"), object("values"), &names),
            "updateRecord" => {
                if text("match") != Some("current") {
                    c.map(owner, &format!("{at} › Match"), object("match"), &names);
                }
                c.map(owner, &format!("{at} › Value"), object("values"), &names);
            }
            "deleteRecord" => {
                if text("match") != Some("current") {
                    c.map(owner, &format!("{at} › Match"), object("match"), &names);
                }
            }
            "runQuery" | "openReport" | "openDashboard" => {
                c.map(owner, &format!("{at} › Parameter"), object("params"), &names)
            }
            "navigate" => {
                let record = get("target").and_then(|t| t.get("recordId")).and_then(Value::as_str);
                c.add(owner, &format!("{at} › Record id"), record, &names);
            }
            "openForm" => c.add(owner, &format!("{at} › Record id"), text("recordId"), &names),
            "setState" => c.add(owner, &format!("{at} › Value"), text("value"), &names),
            "confirm" | "fail" => c.add(owner, &format!("{at} › Message"), text("message"), &names),
            "message" => c.add(owner, &format!("{at} › Text"), text("text"), &names),
            "condition" => {
                walk_steps(c, owner, &steps_of(get("then")), &format!("{at} › Then › "));
                walk_steps(c, owner, &steps_of(get("else")), &format!("{at} › Else › "));
            }
            _ => {}
        }
    }
}

/// Every expression error in `config`, in definition order.
pub fn expression_issues(config: &DocumentConfig, columns: ColumnLookup) -> Vec<ExpressionIssue> {
    let mut c = Collector { issues: vec![], columns };
    for form in &config.design.forms {
        walk_form(&mut c, form);
    }
    for report in &config.reports {
        walk_report(&mut c, report);
    }
    for dashboard in &config.pages {
        walk_dashboard(&mut c, dashboard);
    }
    for action in &config.actions {
        let owner = Owner {
            kind: "action",
            id: action.id.clone(),
            name: action.name.clone(),
            target: RevealTarget::Automation { tab: AutomationTab::Actions, object_id: action.id.clone() },
        };
        walk_steps(&mut c, &owner, &action.steps, "");
    }
    for trigger in &config.triggers {
        let owner = Owner {
            kind: "trigger",
            id: trigger.id.clone(),
            name: trigger.name.clone(),
            target: RevealTarget::Automation { tab: AutomationTab::Triggers, object_id: trigger.id.clone() },
        };
        c.add(&owner, "Condition", trigger.condition.as_deref(), &AUTOMATION_SCOPE_NAMES);
        if trigger.mode == crate::automation::TriggerMode::Async {
            c.add(&owner, "Idempotency key", trigger.idempotency_key.as_deref(), &AUTOMATION_SCOPE_NAMES);
        }
    }
    c.issues
}

/// Tables whose columns limit `record.<field>` in form expressions.
pub fn referenced_tables(config: &DocumentConfig) -> Vec<String> {
    let mut tables: Vec<String> = vec![];
    let mut push = |table: &str| {
        if !table.is_empty() && !tables.iter().any(|t| t == table) {
            tables.push(table.to_string());
        }
    };
    for form in &config.design.forms {
        if let Some(source) = form.source.as_ref().filter(|s| s.kind == SourceKind::Table) {
            push(source.table.as_deref().unwrap_or(""));
        }
        for control in &form.controls {
            if let Some(relationship) = &control.relationship {
                push(&relationship.table);
            }
            if let Some(related) = &control.related {
                push(&related.table);
            }
        }
    }
    tables
}

/// `expression_issues` with the current columns of every referenced table (a blocking read).
pub fn load_expression_issues(label: &str, config: &DocumentConfig) -> Vec<ExpressionIssue> {
    let known: std::collections::HashMap<String, Option<Vec<String>>> = referenced_tables(config)
        .into_iter()
        .map(|table| {
            let columns = crate::inspect_table(label.to_string(), table.clone(), None)
                .ok()
                .map(|schema| schema.columns.into_iter().map(|c| c.name).collect());
            (table, columns)
        })
        .collect();
    expression_issues(config, &|table| table.and_then(|t| known.get(t).cloned().flatten()))
}

#[cfg(test)]
#[path = "expression_issues_tests.rs"]
mod tests;

#[cfg(not(test))]
static GATE_LABEL: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

// Tests run in parallel on their own threads, each with its own document.
#[cfg(test)]
thread_local! {
    static GATE_LABEL: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

fn gate_label() -> String {
    #[cfg(not(test))]
    return GATE_LABEL.lock().unwrap_or_else(|e| e.into_inner()).clone();
    #[cfg(test)]
    GATE_LABEL.with(|l| l.borrow().clone())
}

fn count_errors(config: &DocumentConfig) -> usize {
    load_expression_issues(&gate_label(), config).len()
}

/// Registers the document-wide expression check that bundle export and cloud publish consult
/// (`release::gate`), reading table columns through the document `label`.
pub fn register_gate(label: &str) {
    #[cfg(not(test))]
    {
        *GATE_LABEL.lock().unwrap_or_else(|e| e.into_inner()) = label.to_string();
    }
    #[cfg(test)]
    GATE_LABEL.with(|l| *l.borrow_mut() = label.to_string());
    crate::ui::release::gate::set_expression_gate(Some(count_errors));
}
