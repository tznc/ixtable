//! Row filter pushdown (port of `src/runtime/pushdown.ts`): the parts of a form's row filter
//! that DuckDB can apply as table page filters, so a filtered list over a large table reads
//! only matching rows instead of scanning them. Every value a pushed filter compares with is
//! computed by `crate::expr`; anything whose SQL result could differ from the expression's
//! result stays in the residual expression, evaluated per row.

use crate::data::{Column, DataValue, Filter, FilterOperator, LogicalType};
use crate::expr::{self, Ast, BinaryOp, EvaluateOptions, Node, Scope};
use serde_json::Value;
use std::collections::HashMap;

/// `filters` run in DuckDB; `rest` (None when nothing is left) runs per row.
#[derive(Debug)]
pub struct Pushdown {
    pub filters: Vec<Filter>,
    pub rest: Option<Ast>,
}

/// Top-level `and` operands: a row passes the filter exactly when each one is true.
fn conjuncts(node: &Ast) -> Vec<&Ast> {
    match &node.node {
        Node::Binary {
            op: BinaryOp::And,
            left,
            right,
        } => {
            let mut parts = conjuncts(left);
            parts.extend(conjuncts(right));
            parts
        }
        _ => vec![node],
    }
}

fn reads_record(node: &Ast) -> bool {
    match &node.node {
        Node::Name(path) => path.first().is_some_and(|p| p == "record"),
        Node::Unary { operand, .. } | Node::IsNull { operand, .. } => reads_record(operand),
        Node::Binary { left, right, .. } => reads_record(left) || reads_record(right),
        Node::In { operand, list, .. } => reads_record(operand) || list.iter().any(reads_record),
        Node::Between {
            operand, low, high, ..
        } => [operand, low, high].into_iter().any(|n| reads_record(n)),
        Node::Call { args, .. } => args.iter().any(reads_record),
        Node::Literal(_) | Node::Date(_) => false,
    }
}

/// `record.<column>` of a known column.
fn column_of<'c>(node: &Ast, columns: &HashMap<&str, &'c Column>) -> Option<&'c Column> {
    match &node.node {
        Node::Name(path) if path.len() == 2 && path[0] == "record" => {
            columns.get(path[1].as_str()).copied()
        }
        _ => None,
    }
}

/// The value of a subexpression that does not read the row; None when it fails.
fn constant(node: &Ast, scope: &Scope) -> Option<Value> {
    if reads_record(node) {
        return None;
    }
    expr::evaluate_ast(node, scope, EvaluateOptions::default()).ok()
}

/// True when text reads as an ISO date or timestamp (the expression compares those by instant).
fn reads_as_date(text: &str) -> bool {
    let b = text.as_bytes();
    let shape = b.len() >= 10
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'-'
        && b[8..10].iter().all(u8::is_ascii_digit);
    shape && chrono::NaiveDate::parse_from_str(&text[..10], "%Y-%m-%d").is_ok()
}

/// The bound value for comparing `column` with `value` in SQL, or None when SQL and the
/// expression could disagree: numbers only against integer and real columns (decimal values
/// are text in expressions), text only against text columns, and only for equality and never
/// with text that reads as a date.
fn bindable(column: &Column, value: &Value, equality: bool) -> Option<DataValue> {
    match (value, &column.logical_type) {
        (Value::Number(n), LogicalType::Integer | LogicalType::Real) => {
            let float = n.as_f64().filter(|f| f.is_finite())?;
            match n.as_i64() {
                Some(i) => Some(DataValue::Integer(i)),
                None if float.fract() == 0.0 && n.is_u64() => None,
                None => Some(DataValue::Real(float)),
            }
        }
        (Value::String(s), LogicalType::Text) if equality && !reads_as_date(s) => {
            Some(DataValue::Text(s.clone()))
        }
        _ => None,
    }
}

fn comparison(op: BinaryOp) -> Option<(FilterOperator, Option<BinaryOp>)> {
    Some(match op {
        BinaryOp::Eq => (FilterOperator::Eq, None),
        BinaryOp::Ne => (FilterOperator::Ne, None),
        BinaryOp::Lt => (FilterOperator::Lt, Some(BinaryOp::Gt)),
        BinaryOp::Le => (FilterOperator::Lte, Some(BinaryOp::Ge)),
        BinaryOp::Gt => (FilterOperator::Gt, Some(BinaryOp::Lt)),
        BinaryOp::Ge => (FilterOperator::Gte, Some(BinaryOp::Le)),
        _ => return None,
    })
}

fn filter(column: &str, operator: FilterOperator, value: Option<DataValue>) -> Filter {
    Filter {
        column: column.into(),
        operator,
        value,
        values: None,
    }
}

fn translate(node: &Ast, scope: &Scope, columns: &HashMap<&str, &Column>) -> Option<Vec<Filter>> {
    match &node.node {
        Node::IsNull { operand, negated } => {
            let column = column_of(operand, columns)?;
            let operator = if *negated {
                FilterOperator::IsNotNull
            } else {
                FilterOperator::IsNull
            };
            Some(vec![filter(&column.name, operator, None)])
        }
        Node::Binary { op, left, right } => {
            let (_, _) = comparison(*op)?;
            let (mut field, mut other, mut op) = (left.as_ref(), right.as_ref(), *op);
            if column_of(field, columns).is_none() {
                std::mem::swap(&mut field, &mut other);
                // A comparison with its sides swapped flips direction; equality does not.
                if let Some((_, Some(flipped))) = comparison(op) {
                    op = flipped;
                }
            }
            let column = column_of(field, columns)?;
            let value = constant(other, scope).filter(|v| !v.is_null())?;
            let equality = matches!(op, BinaryOp::Eq | BinaryOp::Ne);
            let bound = bindable(column, &value, equality)?;
            let (operator, _) = comparison(op)?;
            Some(vec![filter(&column.name, operator, Some(bound))])
        }
        Node::Between {
            operand,
            low,
            high,
            negated: false,
        } => {
            let column = column_of(operand, columns)?;
            let low = constant(low, scope).filter(|v| !v.is_null())?;
            let high = constant(high, scope).filter(|v| !v.is_null())?;
            let (low, high) = (bindable(column, &low, false)?, bindable(column, &high, false)?);
            Some(vec![
                filter(&column.name, FilterOperator::Gte, Some(low)),
                filter(&column.name, FilterOperator::Lte, Some(high)),
            ])
        }
        Node::In {
            operand,
            list,
            negated: false,
        } => {
            let column = column_of(operand, columns)?;
            let mut values = vec![];
            for item in list {
                let raw = constant(item, scope)?;
                let items = match raw {
                    Value::Array(items) => items,
                    other => vec![other],
                };
                for v in items {
                    if v.is_null() {
                        return None;
                    }
                    values.push(bindable(column, &v, true)?);
                }
            }
            Some(vec![Filter {
                column: column.name.clone(),
                operator: FilterOperator::In,
                value: None,
                values: Some(values),
            }])
        }
        _ => None,
    }
}

/// Splits a parsed row filter into DuckDB filters over `columns` and a residual expression.
pub fn pushdown(ast: &Ast, scope: &Scope, columns: &[Column]) -> Pushdown {
    let by_name: HashMap<&str, &Column> = columns.iter().map(|c| (c.name.as_str(), c)).collect();
    let mut filters = vec![];
    let mut rest: Vec<&Ast> = vec![];
    for part in conjuncts(ast) {
        match translate(part, scope, &by_name) {
            Some(pushed) => filters.extend(pushed),
            None => rest.push(part),
        }
    }
    let residual = rest.into_iter().fold(None, |left: Option<Ast>, right| {
        Some(match left {
            None => right.clone(),
            Some(left) => Ast {
                start: left.start,
                end: right.end,
                node: Node::Binary {
                    op: BinaryOp::And,
                    left: Box::new(left),
                    right: Box::new(right.clone()),
                },
            },
        })
    });
    Pushdown {
        filters,
        rest: residual,
    }
}

#[cfg(test)]
#[path = "pushdown_tests.rs"]
mod tests;
