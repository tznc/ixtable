//! The expression language (PRD §17.1), evaluated in Rust. One declarative,
//! Access-style language backs validation rules, computed values, show and
//! enable conditions, filters, formatting, action conditions, and report
//! totals. See `README.md` in this directory for the user-facing reference
//! and `docs/decisions/expression-language.md` for the design record.
//!
//! Scopes and results are `serde_json` values: the caller passes JSON-shaped
//! roots (`record`, `form`, `app`, `params`, `rows`, ...) and gets a JSON value
//! back. Whole numbers come back as integers, so they compare equal to
//! `json!(7)`; a function result that is not a finite number comes back null.
mod ast;
mod evaluate;
mod format;
mod functions;
mod parser;
mod regex;
mod values;

#[cfg(test)]
mod tests;

pub use ast::{Ast, BinaryOp, Diagnostic, ExprError, Node, Severity, UnaryOp};
pub use format::{format_date, format_number, format_value, FormatError};
pub use functions::function_names;

use chrono::{DateTime, Utc};
use evaluate::{eval_node, Env};
use functions::{arity_message, lookup_function, parse_unit};
use serde_json::{Map, Value};
use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use values::type_name;

/// Root names to values, such as `record`, `form`, `params`, `rows`.
pub type Scope = Map<String, Value>;

/// Options for one evaluation.
#[derive(Debug, Clone, Copy, Default)]
pub struct EvaluateOptions {
    /// Clock used by `today()` and `now()`. Defaults to the current time.
    pub now: Option<DateTime<Utc>>,
}

impl EvaluateOptions {
    pub fn at(now: DateTime<Utc>) -> Self {
        Self { now: Some(now) }
    }
}

const CACHE_LIMIT: usize = 500;

#[derive(Default)]
struct ParseCache {
    entries: HashMap<String, Arc<Ast>>,
    order: VecDeque<String>,
}

fn cache() -> &'static Mutex<ParseCache> {
    static CACHE: std::sync::OnceLock<Mutex<ParseCache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

/// Parses an expression. The error carries the offending character range.
pub fn parse(src: &str) -> Result<Ast, ExprError> {
    parser::parse_source(src)
}

/// Parses an expression through a cache of the 500 most recently used ones.
pub fn parse_cached(src: &str) -> Result<Arc<Ast>, ExprError> {
    let mut guard = cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(hit) = guard.entries.get(src).cloned() {
        guard.order.retain(|k| k != src);
        guard.order.push_back(src.to_string());
        return Ok(hit);
    }
    drop(guard);
    let ast = Arc::new(parse(src)?);
    let mut guard = cache().lock().unwrap_or_else(|e| e.into_inner());
    if guard.entries.len() >= CACHE_LIMIT {
        if let Some(oldest) = guard.order.pop_front() {
            guard.entries.remove(&oldest);
        }
    }
    guard.entries.insert(src.to_string(), ast.clone());
    guard.order.push_back(src.to_string());
    Ok(ast)
}

/// Evaluates a parsed expression against `scope`. Root names are looked up as
/// keys of `scope`. Errors carry the range of the offending node.
pub fn evaluate_ast(
    ast: &Ast,
    scope: &Scope,
    options: EvaluateOptions,
) -> Result<Value, ExprError> {
    let depth = Cell::new(0);
    eval_node(ast, &Env::root(scope, options.now, &depth))
}

/// Parses and evaluates `src` against `scope`.
pub fn evaluate(src: &str, scope: &Scope, options: EvaluateOptions) -> Result<Value, ExprError> {
    evaluate_ast(&*parse_cached(src)?, scope, options)
}

/// Evaluates a parsed condition. Null counts as false; any non-boolean result
/// is an error.
pub fn evaluate_ast_boolean(
    ast: &Ast,
    scope: &Scope,
    options: EvaluateOptions,
) -> Result<bool, ExprError> {
    match evaluate_ast(ast, scope, options)? {
        Value::Null => Ok(false),
        Value::Bool(b) => Ok(b),
        other => Err(ExprError::new(
            format!("Expected true or false but got {}", type_name(&other)),
            ast.start,
            ast.end,
        )),
    }
}

/// Parses and evaluates a condition (see `evaluate_ast_boolean`).
pub fn evaluate_boolean(
    src: &str,
    scope: &Scope,
    options: EvaluateOptions,
) -> Result<bool, ExprError> {
    evaluate_ast_boolean(&*parse_cached(src)?, scope, options)
}

/// Visits every node. The flag is true inside the per-item argument of
/// `sumof`, `countof` and friends, where names refer to item fields.
fn walk(node: &Ast, visit: &mut dyn FnMut(&Ast, bool), in_item: bool) {
    visit(node, in_item);
    match &node.node {
        Node::Unary { operand, .. } | Node::IsNull { operand, .. } => {
            walk(operand, visit, in_item);
        }
        Node::Binary { left, right, .. } => {
            walk(left, visit, in_item);
            walk(right, visit, in_item);
        }
        Node::In { operand, list, .. } => {
            walk(operand, visit, in_item);
            for item in list {
                walk(item, visit, in_item);
            }
        }
        Node::Between {
            operand, low, high, ..
        } => {
            walk(operand, visit, in_item);
            walk(low, visit, in_item);
            walk(high, visit, in_item);
        }
        Node::Call { name, args, .. } => {
            let per_item = lookup_function(name).and_then(|f| f.per_item_arg);
            for (i, arg) in args.iter().enumerate() {
                walk(arg, visit, in_item || Some(i) == per_item);
            }
        }
        Node::Literal(_) | Node::Date(_) | Node::Name(_) => {}
    }
}

/// Lists the names an expression reads, as dotted paths (`record.amount`), in
/// order of first use. Names inside the per-item argument of `sumof`,
/// `countof` and friends refer to item fields and are left out.
pub fn referenced_names(src: &str) -> Vec<String> {
    try_referenced_names(src).unwrap_or_default()
}

/// Like `referenced_names`, but reports a syntax error instead of returning
/// an empty list.
pub fn try_referenced_names(src: &str) -> Result<Vec<String>, ExprError> {
    let ast = parse_cached(src)?;
    let mut names: Vec<String> = Vec::new();
    walk(
        &ast,
        &mut |node, in_item| {
            if let (Node::Name(path), false) = (&node.node, in_item) {
                let dotted = path.join(".");
                if !names.contains(&dotted) {
                    names.push(dotted);
                }
            }
        },
        false,
    );
    Ok(names)
}

fn literal_text(node: Option<&Ast>) -> Option<&str> {
    match node.map(|n| &n.node) {
        Some(Node::Literal(Value::String(s))) => Some(s),
        _ => None,
    }
}

fn error_diagnostic(message: impl Into<String>, start: usize, end: usize) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        message: message.into(),
        start,
        end,
    }
}

/// Reports problems without running the expression: syntax errors, unknown
/// functions, wrong argument counts, bad literal units and regex patterns,
/// and (when `known_names` is given) names that aren't known. A known name
/// covers everything below it: `record` allows `record.anything`.
pub fn check(src: &str, known_names: Option<&[&str]>) -> Vec<Diagnostic> {
    let ast = match parse_cached(src) {
        Ok(ast) => ast,
        Err(e) => return vec![error_diagnostic(e.message, e.start, e.end)],
    };
    let mut diagnostics = Vec::new();
    let mut has_names = false;
    walk(
        &ast,
        &mut |node, in_item| match &node.node {
            Node::Name(path) => {
                has_names = true;
                let Some(known) = known_names.filter(|_| !in_item) else {
                    return;
                };
                let covered =
                    (1..=path.len()).any(|n| known.contains(&path[..n].join(".").as_str()));
                if covered {
                    return;
                }
                let root_known = known.iter().any(|k| k.split('.').next() == Some(&path[0]));
                let message = if root_known {
                    format!("Unknown field '{}'", path.join("."))
                } else {
                    format!("Unknown name '{}'", path[0])
                };
                diagnostics.push(error_diagnostic(message, node.start, node.end));
            }
            Node::Call {
                name,
                args,
                name_end,
            } => check_call(node, name, args, *name_end, &mut diagnostics),
            _ => {}
        },
        false,
    );
    if diagnostics.is_empty() && !has_names {
        let depth = Cell::new(0);
        let empty = Scope::new();
        if let Err(e) = eval_node(&ast, &Env::root(&empty, None, &depth)) {
            diagnostics.push(error_diagnostic(e.message, e.start, e.end));
        }
    }
    diagnostics
}

fn check_call(node: &Ast, name: &str, args: &[Ast], name_end: usize, out: &mut Vec<Diagnostic>) {
    let Some(def) = lookup_function(name) else {
        let message = format!("Unknown function '{name}'");
        out.push(error_diagnostic(message, node.start, name_end));
        return;
    };
    if let Some(message) = arity_message(name, def, args.len()) {
        out.push(error_diagnostic(message, node.start, node.end));
    }
    if name == "datediff" || name == "dateadd" {
        if let Some(unit) = literal_text(args.first()) {
            if parse_unit(unit).is_none() {
                let (start, end) = (args[0].start, args[0].end);
                out.push(error_diagnostic(
                    format!("Unknown date unit '{unit}'"),
                    start,
                    end,
                ));
            }
        }
    }
    if name == "regexmatch" {
        if let Some(pattern) = literal_text(args.get(1)) {
            if let Err(message) = regex::compile_safe_regex(pattern) {
                let (start, end) = (args[1].start, args[1].end);
                out.push(error_diagnostic(
                    format!("regexmatch: {message}"),
                    start,
                    end,
                ));
            }
        }
    }
}
