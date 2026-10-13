//! Tree-walking evaluator over JSON-shaped values.
use super::ast::{error_at, Ast, BinaryOp, ExprError, Node, UnaryOp};
use super::functions::{arity_message, lookup_function, Call};
use super::values::{
    as_f64, compare_values, is_container, normalize_number, num_value, to_text, type_name,
};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use std::borrow::Cow;
use std::cell::Cell;
use std::cmp::Ordering;

pub type EvalResult = Result<Value, ExprError>;

/// Deepest expression tree the evaluator walks. A 2 MB debug thread overflows
/// near 450 levels, so this keeps a long chain such as `1+1+1+...` from
/// overflowing the stack.
const MAX_EVAL_DEPTH: u32 = 200;

enum Kind<'a> {
    Root(&'a Map<String, Value>),
    Item { row: &'a Value, parent: &'a Env<'a> },
}

/// Name lookup and clock for one evaluation. The per-item argument of
/// `sumof` and friends evaluates in an `Item` environment layered on the root.
pub struct Env<'a> {
    kind: Kind<'a>,
    now: Option<i64>,
    depth: &'a Cell<u32>,
}

impl<'a> Env<'a> {
    pub fn root(
        scope: &'a Map<String, Value>,
        now: Option<DateTime<Utc>>,
        depth: &'a Cell<u32>,
    ) -> Self {
        Self {
            kind: Kind::Root(scope),
            now: now.map(|n| n.timestamp_millis()),
            depth,
        }
    }

    pub fn item<'b>(&'b self, row: &'b Value) -> Env<'b> {
        Env {
            kind: Kind::Item { row, parent: self },
            now: self.now,
            depth: self.depth,
        }
    }

    pub fn now_ms(&self) -> i64 {
        self.now.unwrap_or_else(|| Utc::now().timestamp_millis())
    }

    fn lookup(&self, name: &str) -> Option<&Value> {
        match &self.kind {
            Kind::Root(scope) => scope.get(name),
            Kind::Item { row, parent } => {
                if name == "item" {
                    return Some(row);
                }
                if let Some(found) = row.as_object().and_then(|m| m.get(name)) {
                    return Some(found);
                }
                parent.lookup(name)
            }
        }
    }
}

fn read_field<'v>(
    node: &Ast,
    target: Cow<'v, Value>,
    field: &str,
) -> Result<Cow<'v, Value>, ExprError> {
    let owned = |v: Value| Ok(Cow::Owned(v));
    match target {
        Cow::Borrowed(Value::Null) | Cow::Owned(Value::Null) => owned(Value::Null),
        Cow::Borrowed(Value::Array(items)) => {
            let read = |item| read_field(node, item, field).map(Cow::into_owned);
            let list: Result<Vec<Value>, _> =
                items.iter().map(|i| read(Cow::Borrowed(i))).collect();
            owned(Value::Array(list?))
        }
        Cow::Owned(Value::Array(items)) => {
            let read = |item| read_field(node, item, field).map(Cow::into_owned);
            let list: Result<Vec<Value>, _> =
                items.into_iter().map(|i| read(Cow::Owned(i))).collect();
            owned(Value::Array(list?))
        }
        Cow::Borrowed(Value::Object(map)) => Ok(match map.get(field) {
            Some(v) => Cow::Borrowed(v),
            None => Cow::Owned(Value::Null),
        }),
        Cow::Owned(Value::Object(mut map)) => owned(map.remove(field).unwrap_or(Value::Null)),
        other => Err(error_at(
            node,
            format!("Can't read '{field}' from {}", type_name(&other)),
        )),
    }
}

fn eval_name(node: &Ast, path: &[String], env: &Env) -> EvalResult {
    let root = &path[0];
    let Some(found) = env.lookup(root) else {
        return Err(error_at(node, format!("Unknown name '{root}'")));
    };
    let mut value = Cow::Borrowed(found);
    for field in &path[1..] {
        value = read_field(node, value, field)?;
    }
    Ok(value.into_owned())
}

fn boolean(node: &Ast, v: &Value, what: &str) -> Result<Option<bool>, ExprError> {
    match v {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(*b)),
        other => Err(error_at(
            node,
            format!("{what} needs true or false but got {}", type_name(other)),
        )),
    }
}

pub(crate) fn scalar<'v>(node: &Ast, v: &'v Value) -> Result<&'v Value, ExprError> {
    if is_container(v) {
        return Err(error_at(node, format!("Can't compare a {}", type_name(v))));
    }
    Ok(v)
}

fn tri(b: Option<bool>) -> Value {
    b.map_or(Value::Null, Value::Bool)
}

fn arithmetic_verb(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "add",
        BinaryOp::Sub => "subtract",
        BinaryOp::Mul => "multiply",
        BinaryOp::Div => "divide",
        _ => "take the remainder of",
    }
}

fn eval_logic(op: BinaryOp, l: &Ast, r: &Ast, env: &Env) -> EvalResult {
    let word = if op == BinaryOp::And { "and" } else { "or" };
    let what = format!("'{word}'");
    let left = boolean(l, &eval_node(l, env)?, &what)?;
    if op == BinaryOp::And && left == Some(false) {
        return Ok(Value::Bool(false));
    }
    if op == BinaryOp::Or && left == Some(true) {
        return Ok(Value::Bool(true));
    }
    let right = boolean(r, &eval_node(r, env)?, &what)?;
    Ok(tri(if op == BinaryOp::And {
        if right == Some(false) {
            Some(false)
        } else if left.is_none() || right.is_none() {
            None
        } else {
            Some(true)
        }
    } else if right == Some(true) {
        Some(true)
    } else if left.is_none() || right.is_none() {
        None
    } else {
        Some(false)
    }))
}

fn eval_arithmetic(node: &Ast, op: BinaryOp, left: &Value, right: &Value) -> EvalResult {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    let (Some(a), Some(b)) = (as_f64(left), as_f64(right)) else {
        let text = left.is_string() || right.is_string();
        let hint = if op == BinaryOp::Add && text {
            "; use & to join text"
        } else {
            ""
        };
        return Err(error_at(
            node,
            format!(
                "Can't {} {} and {}{hint}",
                arithmetic_verb(op),
                type_name(left),
                type_name(right)
            ),
        ));
    };
    if matches!(op, BinaryOp::Div | BinaryOp::Rem) && b == 0.0 {
        return Ok(Value::Null);
    }
    let result = normalize_number(match op {
        BinaryOp::Add => a + b,
        BinaryOp::Sub => a - b,
        BinaryOp::Mul => a * b,
        BinaryOp::Div => a / b,
        _ => a % b,
    });
    if !result.is_finite() {
        return Err(error_at(node, "Number is too large"));
    }
    Ok(num_value(result))
}

fn eval_binary(node: &Ast, op: BinaryOp, l: &Ast, r: &Ast, env: &Env) -> EvalResult {
    if matches!(op, BinaryOp::And | BinaryOp::Or) {
        return eval_logic(op, l, r, env);
    }
    let left = eval_node(l, env)?;
    let right = eval_node(r, env)?;
    match op {
        BinaryOp::Concat => {
            for (side, v) in [(l, &left), (r, &right)] {
                if is_container(v) {
                    return Err(error_at(
                        side,
                        format!("Can't join {} as text", type_name(v)),
                    ));
                }
            }
            Ok(Value::String(to_text(&left) + &to_text(&right)))
        }
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
            eval_arithmetic(node, op, &left, &right)
        }
        _ => {
            let a = scalar(l, &left)?;
            let b = scalar(r, &right)?;
            if a.is_null() || b.is_null() {
                return Ok(Value::Null);
            }
            let cmp = compare_values(a, b);
            Ok(Value::Bool(match op {
                BinaryOp::Eq => cmp == Some(Ordering::Equal),
                BinaryOp::Ne => cmp != Some(Ordering::Equal),
                _ => {
                    let Some(cmp) = cmp else {
                        return Err(error_at(
                            node,
                            format!("Can't compare {} with {}", type_name(a), type_name(b)),
                        ));
                    };
                    match op {
                        BinaryOp::Lt => cmp == Ordering::Less,
                        BinaryOp::Le => cmp != Ordering::Greater,
                        BinaryOp::Gt => cmp == Ordering::Greater,
                        _ => cmp != Ordering::Less,
                    }
                }
            }))
        }
    }
}

fn eval_in(operand: &Ast, list: &[Ast], negated: bool, env: &Env) -> EvalResult {
    let value = eval_node(operand, env)?;
    let v = scalar(operand, &value)?;
    if v.is_null() {
        return Ok(Value::Null);
    }
    let mut saw_null = false;
    for item_node in list {
        let raw = eval_node(item_node, env)?;
        let items = match raw {
            Value::Array(items) => items,
            single => vec![single],
        };
        for item in &items {
            if item.is_null() {
                saw_null = true;
            } else if compare_values(v, scalar(item_node, item)?) == Some(Ordering::Equal) {
                return Ok(Value::Bool(!negated));
            }
        }
    }
    Ok(if saw_null {
        Value::Null
    } else {
        Value::Bool(negated)
    })
}

fn eval_between(node: &Ast, parts: [&Ast; 3], negated: bool, env: &Env) -> EvalResult {
    let [operand, low, high] = parts;
    let v = eval_node(operand, env)?;
    let v = scalar(operand, &v)?;
    let lo = eval_node(low, env)?;
    let lo = scalar(low, &lo)?;
    let hi = eval_node(high, env)?;
    let hi = scalar(high, &hi)?;
    if v.is_null() || lo.is_null() || hi.is_null() {
        return Ok(Value::Null);
    }
    let (c1, c2) = (compare_values(v, lo), compare_values(v, hi));
    let (Some(c1), Some(c2)) = (c1, c2) else {
        let other = if c1.is_none() { lo } else { hi };
        return Err(error_at(
            node,
            format!("Can't compare {} with {}", type_name(v), type_name(other)),
        ));
    };
    let inside = c1 != Ordering::Less && c2 != Ordering::Greater;
    Ok(Value::Bool(inside != negated))
}

fn eval_call(node: &Ast, name: &str, args: &[Ast], name_end: usize, env: &Env) -> EvalResult {
    let Some(def) = lookup_function(name) else {
        return Err(ExprError::new(
            format!("Unknown function '{name}'"),
            node.start,
            name_end,
        ));
    };
    if let Some(message) = arity_message(name, def, args.len()) {
        return Err(error_at(node, message));
    }
    (def.imp)(&Call {
        node,
        name,
        args,
        env,
    })
}

/// Evaluates one node. Errors carry the node's source range.
pub fn eval_node(node: &Ast, env: &Env) -> EvalResult {
    let depth = env.depth.get() + 1;
    if depth > MAX_EVAL_DEPTH {
        return Err(ExprError::new(
            "Expression is nested too deeply",
            node.start,
            node.end,
        ));
    }
    env.depth.set(depth);
    let result = eval_inner(node, env);
    env.depth.set(depth - 1);
    result
}

fn eval_inner(node: &Ast, env: &Env) -> EvalResult {
    match &node.node {
        Node::Literal(v) => Ok(v.clone()),
        Node::Date(text) => Ok(Value::String(text.clone())),
        Node::Name(path) => eval_name(node, path, env),
        Node::Unary { op, operand } => {
            let v = eval_node(operand, env)?;
            if *op == UnaryOp::Not {
                let b = boolean(operand, &v, "'not'")?;
                return Ok(tri(b.map(|b| !b)));
            }
            if v.is_null() {
                return Ok(Value::Null);
            }
            let Some(n) = as_f64(&v) else {
                return Err(error_at(node, format!("Can't negate {}", type_name(&v))));
            };
            Ok(num_value(if n == 0.0 { 0.0 } else { -n }))
        }
        Node::Binary { op, left, right } => eval_binary(node, *op, left, right, env),
        Node::In {
            operand,
            list,
            negated,
        } => eval_in(operand, list, *negated, env),
        Node::Between {
            operand,
            low,
            high,
            negated,
        } => eval_between(node, [operand, low, high], *negated, env),
        Node::IsNull { operand, negated } => {
            let is_null = eval_node(operand, env)?.is_null();
            Ok(Value::Bool(is_null != *negated))
        }
        Node::Call {
            name,
            args,
            name_end,
        } => eval_call(node, name, args, *name_end, env),
    }
}
