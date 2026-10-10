//! Access macros → ixtable actions (`docs/access-format.md` §3.4).
//!
//! Since Access 2010 every macro carries its XML form ("AXL") split across
//! `Comment ="_AXL:..."` rows; older macros only have positional rows
//! (`Action`, `Argument`...). Navigation, messages, temporary variables and
//! conditions convert to action steps. Window, focus and error-handling
//! actions have no ixtable meaning and are dropped; data actions (RunSQL,
//! SetValue, ...) are reported.
use super::forms::Context;
use super::report::{ImportReport, Notes, Status};
use crate::access::text_format::{Item, Node, PropValue};
use crate::access::translate::expr::{quote_text, translate, Target};
use crate::access::xml::{self, Element};
use serde_json::{json, Value};

/// One macro statement in a format-neutral shape.
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Action {
        name: String,
        args: Vec<(String, String)>,
    },
    If {
        branches: Vec<(Option<String>, Vec<Statement>)>,
    },
}

/// Joins the `_AXL:` comment chunks of a macro block into its XML.
pub fn axl(node: &Node) -> Option<String> {
    let mut xml = String::new();
    collect_axl(node, &mut xml);
    (!xml.is_empty()).then_some(xml)
}

fn collect_axl(node: &Node, out: &mut String) {
    for item in &node.items {
        match item {
            Item::Prop {
                key,
                value: PropValue::Str(s),
                ..
            } if key == "Comment" => {
                if let Some(chunk) = s.strip_prefix("_AXL:") {
                    out.push_str(chunk);
                }
            }
            Item::Block(b) => collect_axl(b, out),
            _ => {}
        }
    }
}

fn statements(el: &Element) -> Vec<Statement> {
    let mut out = vec![];
    for child in el.elements() {
        match child.name.as_str() {
            "Action" => out.push(Statement::Action {
                name: child.attr("Name").unwrap_or_default().to_string(),
                args: child
                    .children_named("Argument")
                    .map(|a| (a.attr("Name").unwrap_or_default().to_string(), a.text()))
                    .collect(),
            }),
            "ConditionalBlock" => {
                let mut branches = vec![];
                for b in child.elements() {
                    let cond = b.child("Condition").map(|c| c.text());
                    let body = b.child("Statements").map(statements).unwrap_or_default();
                    match b.name.as_str() {
                        "If" | "ElseIf" => branches.push((cond, body)),
                        "Else" => branches.push((None, body)),
                        _ => {}
                    }
                }
                out.push(Statement::If { branches });
            }
            "Group" => out.extend(
                child
                    .child("Statements")
                    .map(statements)
                    .unwrap_or_default(),
            ),
            _ => {}
        }
    }
    out
}

/// Parses AXL into the main statements and named submacros.
pub fn parse_axl(text: &str) -> Result<(Vec<Statement>, Vec<(String, Vec<Statement>)>), String> {
    let root = xml::parse(text)?;
    let main = root.child("Statements").map(statements).unwrap_or_default();
    let subs = root
        .children_named("Sub")
        .map(|s| {
            (
                s.attr("Name").unwrap_or_default().to_string(),
                s.child("Statements").map(statements).unwrap_or_default(),
            )
        })
        .collect();
    Ok((main, subs))
}

/// Positional arguments of the legacy rows, named as AXL names them.
fn legacy_args(action: &str) -> &'static [&'static str] {
    match action {
        "OpenForm" => &[
            "FormName",
            "View",
            "FilterName",
            "WhereCondition",
            "DataMode",
            "WindowMode",
        ],
        "OpenReport" => &[
            "ReportName",
            "View",
            "FilterName",
            "WhereCondition",
            "WindowMode",
        ],
        "OpenTable" => &["TableName", "View", "DataMode"],
        "OpenQuery" => &["QueryName", "View", "DataMode"],
        "RunSQL" => &["SQLStatement", "UseTransaction"],
        "MsgBox" => &["Message", "Beep", "Type", "Title"],
        "RunMacro" => &["MacroName", "RepeatCount", "RepeatExpression"],
        "SetTempVar" => &["Name", "Expression"],
        "GoToRecord" => &["ObjectType", "ObjectName", "Record", "Offset"],
        _ => &[],
    }
}

/// Reads the legacy macro rows (`Begin` blocks with Condition/Action/Argument).
pub fn parse_legacy(node: &Node) -> Vec<Statement> {
    let mut out = vec![];
    for b in node.blocks() {
        let Some(action) = b.get("Action") else {
            continue;
        };
        let names = legacy_args(action);
        let args = b
            .props("Argument")
            .enumerate()
            .map(|(i, v)| {
                (
                    names.get(i).copied().unwrap_or("").to_string(),
                    v.to_string(),
                )
            })
            .collect();
        let name = if action == "MsgBox" {
            "MessageBox".to_string()
        } else {
            action.to_string()
        };
        let stmt = Statement::Action { name, args };
        match b.get("Condition") {
            Some(c) if c != "..." => out.push(Statement::If {
                branches: vec![(Some(c.to_string()), vec![stmt])],
            }),
            _ => out.push(stmt),
        }
    }
    out
}

/// Macro statements of an embedded macro block or a macro object.
pub fn read_macro(node: &Node) -> (Vec<Statement>, Vec<(String, Vec<Statement>)>) {
    if let Some(text) = axl(node) {
        if let Ok(parsed) = parse_axl(&text) {
            return parsed;
        }
    }
    (parse_legacy(node), vec![])
}

/// Actions with no ixtable meaning (window, focus, error handling).
const IGNORED: [&str; 33] = [
    "OnError",
    "ClearMacroError",
    "StopMacro",
    "StopAllMacros",
    "Beep",
    "Echo",
    "SetWarnings",
    "Requery",
    "Refresh",
    "RefreshRecord",
    "RepaintObject",
    "GoToControl",
    "MoveAndSizeWindow",
    "MaximizeWindow",
    "MinimizeWindow",
    "RestoreWindow",
    "LockNavigationPane",
    "SetMenuItem",
    "CancelEvent",
    "RemoveTempVar",
    "RemoveAllTempVars",
    "SaveRecord",
    "CloseWindow",
    "Close",
    "RunMenuCommand",
    "RunCommand",
    "Hourglass",
    "SelectObject",
    "ShowToolbar",
    "ShowAllRecords",
    "SearchForRecord",
    "FindRecord",
    "FindNext",
];

/// Converts statements to ixtable steps.
pub struct StepWriter<'c, 'a> {
    pub ctx: &'c Context<'a>,
    /// Names on the form the macro runs on → ixtable expressions.
    pub resolve: &'c dyn Fn(&str) -> Option<String>,
    /// The form the macro belongs to (GoToRecord New opens it in create mode).
    pub form_id: Option<String>,
    pub notes: Notes,
    /// Names the saved queries RunSQL actions become (`<owner> (RunSQL n)`).
    pub owner: String,
    /// Saved action queries made from RunSQL actions; the caller adds them to the document.
    pub queries: Vec<Value>,
}

fn arg<'s>(args: &'s [(String, String)], name: &str) -> Option<&'s str> {
    args.iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
}

fn step(kind: &str, fields: Value) -> Value {
    let mut v = json!({ "id": uuid::Uuid::now_v7().to_string(), "kind": kind });
    if let (Some(o), Some(f)) = (v.as_object_mut(), fields.as_object()) {
        for (k, x) in f {
            o.insert(k.clone(), x.clone());
        }
    }
    v
}

impl StepWriter<'_, '_> {
    fn expr(&self, text: &str) -> Result<String, String> {
        let resolve = |n: &str| (self.resolve)(n);
        translate(text, Target::Form, &resolve)
    }

    /// A macro argument that is either literal text or `=expression`.
    fn text_or_expr(&self, v: &str) -> Result<String, String> {
        match v.strip_prefix('=') {
            Some(e) => self.expr(e),
            None => Ok(quote_text(v)),
        }
    }

    pub fn steps(&mut self, stmts: &[Statement]) -> Vec<Value> {
        let mut out = vec![];
        for s in stmts {
            match s {
                Statement::Action { name, args } => {
                    if let Some(v) = self.action(name, args) {
                        out.push(v);
                    }
                }
                Statement::If { branches } => {
                    if let Some(v) = self.conditional(branches) {
                        out.push(v);
                    }
                }
            }
        }
        out
    }

    fn conditional(&mut self, branches: &[(Option<String>, Vec<Statement>)]) -> Option<Value> {
        let ((cond, body), rest) = branches.split_first()?;
        let cond = cond.as_deref()?;
        // Conditions on Access' own state have a known answer in ixtable.
        let parsed = crate::access::translate::ast::parse_expression(cond);
        let rest_expr = match parsed.as_ref().map(fold) {
            Ok(Folded::Known(true)) => return self.inline(body),
            Ok(Folded::Known(false)) => {
                return match rest.first() {
                    Some((None, body)) => self.inline(body),
                    Some(_) => self.conditional(rest),
                    None => None,
                };
            }
            Ok(Folded::Rest(e)) => Some(e),
            Err(_) => None,
        };
        let translated = match rest_expr {
            Some(e) => {
                let resolve = |n: &str| (self.resolve)(n);
                crate::access::translate::expr::ExprWriter::new(Target::Form, &resolve).write(&e)
            }
            None => self.expr(cond),
        };
        let when = match translated {
            Ok(w) => w,
            Err(e) => {
                self.notes
                    .push(format!("a macro condition was not converted ({cond}: {e})"));
                return None;
            }
        };
        let then = self.steps(body);
        let otherwise = match rest.first() {
            Some((None, body)) => self.steps(body),
            Some(_) => self.conditional(rest).into_iter().collect(),
            None => vec![],
        };
        if then.is_empty() && otherwise.is_empty() {
            return None;
        }
        Some(step(
            "condition",
            json!({ "when": when, "then": then, "else": otherwise }),
        ))
    }

    /// Statements of a branch whose condition is known, as one step (or none).
    fn inline(&mut self, body: &[Statement]) -> Option<Value> {
        let steps = self.steps(body);
        match steps.len() {
            0 => None,
            1 => steps.into_iter().next(),
            _ => Some(step(
                "condition",
                json!({ "when": "true", "then": steps, "else": [] }),
            )),
        }
    }

    fn action(&mut self, name: &str, args: &[(String, String)]) -> Option<Value> {
        if IGNORED.iter().any(|i| i.eq_ignore_ascii_case(name)) {
            return None;
        }
        match name {
            "OpenForm" => {
                let form = arg(args, "FormName")?;
                let Some(id) = self.ctx.form_ids.get(&form.to_lowercase()).cloned() else {
                    self.notes.push(format!("OpenForm {form}: no such form"));
                    return None;
                };
                let mut fields = json!({ "formId": id });
                if arg(args, "DataMode").is_some_and(|m| m.eq_ignore_ascii_case("Add") || m == "0")
                {
                    fields["mode"] = json!("create");
                }
                // `1=0` shows no rows: Access' idiom for a blank new record.
                if arg(args, "WhereCondition").is_some_and(|w| w.replace(' ', "") == "1=0") {
                    fields["mode"] = json!("create");
                } else if let Some(w) = arg(args, "WhereCondition") {
                    match self.record_id(w) {
                        Some(r) => {
                            fields["recordId"] = json!(r);
                            fields["mode"] = json!("edit");
                        }
                        None => self
                            .notes
                            .push(format!("OpenForm {form}: the filter {w} was not converted")),
                    }
                }
                Some(step("openForm", fields))
            }
            "OpenReport" => {
                let report = arg(args, "ReportName")?;
                let id = self.ctx.report_ids.get(&report.to_lowercase()).cloned()?;
                if arg(args, "WhereCondition").is_some() {
                    self.notes
                        .push(format!("OpenReport {report}: the filter was not converted"));
                }
                Some(step("openReport", json!({ "reportId": id })))
            }
            "OpenTable" => {
                let t = arg(args, "TableName")?;
                let name = self.ctx.table_name(t)?;
                Some(step(
                    "navigate",
                    json!({ "target": { "kind": "table", "id": name } }),
                ))
            }
            "OpenQuery" => {
                let name = arg(args, "QueryName")?;
                let q = self.ctx.queries.queries.get(&name.to_lowercase());
                let Some(q) = q.filter(|q| q.action.is_some()) else {
                    self.notes.push(format!(
                        "OpenQuery {name}: showing a query's rows has no ixtable equivalent"
                    ));
                    return None;
                };
                let (id, params) = (q.id.clone(), q.all_params(&self.ctx.queries.queries));
                Some(step(
                    "runQuery",
                    json!({ "queryId": id, "params": self.query_params(&params), "storeAs": "" }),
                ))
            }
            "RunSQL" => {
                let sql = arg(args, "SQLStatement")?;
                if sql.starts_with('=') {
                    self.notes
                        .push("RunSQL with SQL built by an expression was not converted");
                    return None;
                }
                let name = format!("{} (RunSQL {})", self.owner, self.queries.len() + 1);
                match self.ctx.sql_action_query(sql, &name) {
                    Ok(query) => {
                        let id = query["id"].clone();
                        self.queries.push(query);
                        Some(step(
                            "runQuery",
                            json!({ "queryId": id, "params": {}, "storeAs": "" }),
                        ))
                    }
                    Err(e) => {
                        self.notes.push(format!("RunSQL was not converted ({e})"));
                        None
                    }
                }
            }
            "MessageBox" | "MsgBox" => {
                let message = arg(args, "Message")?;
                match self.text_or_expr(message) {
                    Ok(text) => Some(step("message", json!({ "text": text }))),
                    Err(e) => {
                        self.notes
                            .push(format!("a message box was not converted ({e})"));
                        None
                    }
                }
            }
            "SetTempVar" => {
                let key = arg(args, "Name")?;
                let value = arg(args, "Expression").unwrap_or("Null");
                match self.expr(value) {
                    Ok(v) => Some(step(
                        "setState",
                        json!({ "scope": "app", "key": key, "value": v }),
                    )),
                    Err(e) => {
                        self.notes
                            .push(format!("SetTempVar {key} was not converted ({e})"));
                        None
                    }
                }
            }
            "RunMacro" => {
                let m = arg(args, "MacroName")?;
                let id = self.ctx.action_ids.get(&m.to_lowercase()).cloned()?;
                Some(step("runAction", json!({ "actionId": id })))
            }
            "GoToRecord"
                if arg(args, "Record")
                    .is_some_and(|r| r.eq_ignore_ascii_case("New") || r == "5") =>
            {
                let id = self.form_id.clone()?;
                Some(step("openForm", json!({ "formId": id, "mode": "create" })))
            }
            other => {
                self.notes.push(format!(
                    "the macro action {other} has no ixtable equivalent"
                ));
                None
            }
        }
    }

    /// Values for an action query's parameters: form references become expressions.
    fn query_params(&mut self, params: &[crate::access::translate::sql::Param]) -> Value {
        let mut out = serde_json::Map::new();
        for p in params {
            match self.expr(&format!("[{}]", p.original)) {
                Ok(e) if p.original.contains('!') => {
                    out.insert(p.name.clone(), json!(e));
                }
                _ => self.notes.push(format!(
                    "the query parameter {} has no value in ixtable; set it on the step",
                    p.original
                )),
            }
        }
        Value::Object(out)
    }

    /// `"[ID]=" & [ID]` / `[ID]=Forms!X!ID` → the record id expression.
    fn record_id(&self, where_condition: &str) -> Option<String> {
        let w = where_condition.trim().trim_start_matches('=').trim();
        // "[Field]=" & <expr>
        if let Some(rest) = w.strip_prefix('"') {
            let close = rest.find('"')?;
            let lhs = rest[..close].trim();
            if !lhs.ends_with('=') {
                return None;
            }
            let after = rest[close + 1..].trim().strip_prefix('&')?.trim();
            return self.expr(after).ok();
        }
        None
    }
}

/// A condition with Access' own state folded in: forms save as you go (never
/// dirty, never a pending new record), a macro has no pending error, and the
/// database is trusted. Returns the known value, or the rest of the condition.
pub enum Folded {
    Known(bool),
    Rest(crate::access::translate::ast::Expr),
}

pub fn fold(e: &crate::access::translate::ast::Expr) -> Folded {
    use crate::access::translate::ast::{BinOp, Expr};
    let name = |e: &Expr| match e {
        Expr::Name(p) => Some(
            p.iter()
                .map(|x| x.text.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join("."),
        ),
        _ => None,
    };
    match e {
        Expr::Paren(x) => fold(x),
        Expr::Not(x) => match fold(x) {
            Folded::Known(b) => Folded::Known(!b),
            Folded::Rest(r) => Folded::Rest(Expr::Not(Box::new(r))),
        },
        Expr::Bin(op @ (BinOp::And | BinOp::Or), l, r) => match (fold(l), fold(r), op) {
            (Folded::Known(false), _, BinOp::And) | (_, Folded::Known(false), BinOp::And) => {
                Folded::Known(false)
            }
            (Folded::Known(true), _, BinOp::Or) | (_, Folded::Known(true), BinOp::Or) => {
                Folded::Known(true)
            }
            (Folded::Known(_), x, _) | (x, Folded::Known(_), _) => x,
            (Folded::Rest(a), Folded::Rest(b), op) => {
                Folded::Rest(Expr::Bin(*op, Box::new(a), Box::new(b)))
            }
        },
        Expr::Bin(op @ (BinOp::Ne | BinOp::Eq), l, r)
            if matches!(name(l).as_deref(), Some("macroerror.number" | "macroerror"))
                && matches!(&**r, Expr::Num(n) if n == "0") =>
        {
            Folded::Known(*op == BinOp::Eq)
        }
        other => match name(other).as_deref() {
            Some("form.dirty" | "me.dirty" | "form.newrecord" | "me.newrecord") => {
                Folded::Known(false)
            }
            Some("currentproject.istrusted") => Folded::Known(true),
            _ => Folded::Rest(other.clone()),
        },
    }
}

/// Converts the macro objects into actions (submacros are actions of their own).
pub fn convert_macros(ctx: &mut Context, report: &mut ImportReport) -> Vec<Value> {
    // Ids first so RunMacro can refer to any macro.
    let mut parsed = vec![];
    for m in &ctx.db.macros {
        let (main, subs) = read_macro(&m.root);
        if !main.is_empty() {
            ctx.action_ids
                .insert(m.name.to_lowercase(), uuid::Uuid::now_v7().to_string());
        }
        for (s, _) in &subs {
            ctx.action_ids.insert(
                format!("{}.{}", m.name, s).to_lowercase(),
                uuid::Uuid::now_v7().to_string(),
            );
        }
        parsed.push((m.name.clone(), main, subs));
    }
    let mut actions = vec![];
    let no_fields = |_: &str| None;
    for (name, main, subs) in parsed {
        if name.eq_ignore_ascii_case("AutoExec") {
            ctx.start_form = autoexec_form(&main);
        }
        let mut notes = Notes::default();
        let mut converted = 0;
        let parts = std::iter::once((name.clone(), main))
            .chain(subs.into_iter().map(|(s, b)| (format!("{name}.{s}"), b)));
        for (action_name, body) in parts {
            if body.is_empty() {
                continue;
            }
            let mut w = StepWriter {
                ctx,
                resolve: &no_fields,
                form_id: None,
                notes: Notes::default(),
                owner: action_name.clone(),
                queries: vec![],
            };
            let steps = w.steps(&body);
            let (lost, made) = (w.notes.0, w.queries);
            for n in lost {
                notes.push(n);
            }
            ctx.extra_queries.extend(made);
            let Some(id) = ctx.action_ids.get(&action_name.to_lowercase()).cloned() else {
                continue;
            };
            if steps.is_empty() {
                continue;
            }
            converted += 1;
            actions.push(json!({ "id": id, "name": action_name, "description": "Converted from an Access macro.", "steps": steps, "onError": "stop" }));
        }
        let status = if converted == 0 {
            Status::Skipped
        } else {
            notes.status()
        };
        let mut n = notes.0;
        if converted == 0 && n.is_empty() {
            n.push("none of its actions have an ixtable equivalent".into());
        }
        report.add("macro", &name, status, n);
    }
    // Actions that never got steps keep no id (RunMacro to them is dropped).
    let kept: Vec<String> = actions
        .iter()
        .filter_map(|a| a["id"].as_str().map(str::to_string))
        .collect();
    ctx.action_ids.retain(|_, id| kept.contains(id));
    actions
}

/// The form an AutoExec macro opens first: the app's start page.
fn autoexec_form(stmts: &[Statement]) -> Option<String> {
    stmts.iter().find_map(|s| match s {
        Statement::Action { name, args } if name == "OpenForm" => {
            arg(args, "FormName").map(str::to_string)
        }
        _ => None,
    })
}
