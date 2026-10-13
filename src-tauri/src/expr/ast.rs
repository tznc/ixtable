//! Syntax tree, diagnostics, and the error type of the expression language.
//! Positions are UTF-16 code unit offsets into the source, so they equal the
//! offsets an editor built on JavaScript strings reports.
use serde::Serialize;
use serde_json::Value;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

/// One node with its source range `[start, end)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Ast {
    pub start: usize,
    pub end: usize,
    pub node: Node,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// Number, text, boolean, or null.
    Literal(Value),
    /// A `#...#` literal, stored as normalized ISO text.
    Date(String),
    Name(Vec<String>),
    Unary {
        op: UnaryOp,
        operand: Box<Ast>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Ast>,
        right: Box<Ast>,
    },
    In {
        operand: Box<Ast>,
        list: Vec<Ast>,
        negated: bool,
    },
    Between {
        operand: Box<Ast>,
        low: Box<Ast>,
        high: Box<Ast>,
        negated: bool,
    },
    IsNull {
        operand: Box<Ast>,
        negated: bool,
    },
    Call {
        /// Lowercased function name.
        name: String,
        args: Vec<Ast>,
        name_end: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// A problem found by `check`, with the character range it applies to.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub start: usize,
    pub end: usize,
}

/// A syntax or evaluation error with the offending character range.
#[derive(Debug, Clone, PartialEq)]
pub struct ExprError {
    pub message: String,
    pub start: usize,
    pub end: usize,
}

impl ExprError {
    pub fn new(message: impl Into<String>, start: usize, end: usize) -> Self {
        Self {
            message: message.into(),
            start,
            end,
        }
    }

    pub fn pos(&self) -> usize {
        self.start
    }
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExprError {}

pub(crate) fn error_at(node: &Ast, message: impl Into<String>) -> ExprError {
    ExprError::new(message, node.start, node.end)
}
