//! Hand-written tokenizer and recursive-descent parser.
use super::ast::{Ast, BinaryOp, ExprError, Node, UnaryOp};
use super::values::{is_js_space, js_trim, num_value, parse_iso_date};
use serde_json::Value;

type Parsed<T> = Result<T, ExprError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Num,
    Str,
    Date,
    Ident,
    Bracket,
    Kw,
    Op,
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    text: String,
    /// Keyword (lowercased), identifier, bracket name, string, or date text.
    value: String,
    number: f64,
    start: usize,
    end: usize,
}

const KEYWORDS: [&str; 9] = [
    "and", "or", "not", "in", "between", "is", "null", "true", "false",
];
const OPERATORS: [&str; 20] = [
    "&&", "||", "<=", ">=", "<>", "!=", "==", "=", "<", ">", "+", "-", "*", "/", "%", "&", "!",
    "(", ")", ",",
];
const BLOCKED_NAMES: [&str; 3] = ["__proto__", "constructor", "prototype"];
const MAX_SOURCE_LENGTH: usize = 10_000;
const MAX_DEPTH: usize = 100;

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}

fn is_ident_part(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

fn comparison_op(text: &str) -> Option<BinaryOp> {
    Some(match text {
        "=" | "==" => BinaryOp::Eq,
        "!=" | "<>" => BinaryOp::Ne,
        "<" => BinaryOp::Lt,
        "<=" => BinaryOp::Le,
        ">" => BinaryOp::Gt,
        ">=" => BinaryOp::Ge,
        _ => return None,
    })
}

/// Length in characters of the number starting at `i`.
fn number_len(chars: &[char], i: usize) -> usize {
    let digit_at = |j: usize| chars.get(j).is_some_and(char::is_ascii_digit);
    let mut j = i;
    while digit_at(j) {
        j += 1;
    }
    if j > i {
        if chars.get(j) == Some(&'.') && digit_at(j + 1) {
            j += 1;
            while digit_at(j) {
                j += 1;
            }
        }
    } else {
        j += 1;
        while digit_at(j) {
            j += 1;
        }
    }
    if matches!(chars.get(j), Some('e' | 'E')) {
        let mut k = j + 1;
        if matches!(chars.get(k), Some('+' | '-')) {
            k += 1;
        }
        if digit_at(k) {
            while digit_at(k) {
                k += 1;
            }
            j = k;
        }
    }
    j - i
}

fn tokenize(src: &str) -> Parsed<Vec<Token>> {
    let chars: Vec<char> = src.chars().collect();
    let mut offs = Vec::with_capacity(chars.len() + 1);
    let mut acc = 0;
    for c in &chars {
        offs.push(acc);
        acc += c.len_utf16();
    }
    offs.push(acc);
    let total = acc;
    let text_of = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if is_js_space(ch) {
            i += 1;
            continue;
        }
        let begin = i;
        let start = offs[i];
        let next_is_digit = chars.get(i + 1).is_some_and(char::is_ascii_digit);
        if ch.is_ascii_digit() || (ch == '.' && next_is_digit) {
            i += number_len(&chars, i);
            let text = text_of(begin, i);
            if let Some(&c) = chars.get(i) {
                if is_ident_part(c) {
                    return Err(ExprError::new(
                        format!("Invalid number '{text}{c}'"),
                        start,
                        offs[i] + 1,
                    ));
                }
            }
            let number = text.parse::<f64>().unwrap_or(f64::NAN);
            if !number.is_finite() {
                return Err(ExprError::new("Number is too large", start, offs[i]));
            }
            tokens.push(Token {
                kind: Kind::Num,
                text,
                value: String::new(),
                number,
                start,
                end: offs[i],
            });
            continue;
        }
        if ch == '\'' || ch == '"' {
            let mut value = String::new();
            i += 1;
            loop {
                if i >= chars.len() {
                    return Err(ExprError::new("Unterminated string", start, total));
                }
                if chars[i] == ch {
                    if chars.get(i + 1) == Some(&ch) {
                        value.push(ch);
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                value.push(chars[i]);
                i += 1;
            }
            tokens.push(Token {
                kind: Kind::Str,
                text: text_of(begin, i),
                value,
                number: 0.0,
                start,
                end: offs[i],
            });
            continue;
        }
        if ch == '#' {
            let Some(rel) = chars[i + 1..].iter().position(|c| *c == '#') else {
                return Err(ExprError::new("Unterminated date literal", start, total));
            };
            let close = i + 1 + rel;
            let raw = text_of(i + 1, close);
            let raw = js_trim(&raw).to_string();
            i = close + 1;
            let Some(parsed) = parse_iso_date(&raw) else {
                return Err(ExprError::new(
                    format!("Invalid date '{raw}'; use #YYYY-MM-DD# or #YYYY-MM-DDTHH:MM:SS#"),
                    start,
                    offs[i],
                ));
            };
            tokens.push(Token {
                kind: Kind::Date,
                text: text_of(begin, i),
                value: parsed.iso,
                number: 0.0,
                start,
                end: offs[i],
            });
            continue;
        }
        if ch == '[' {
            let Some(rel) = chars[i + 1..].iter().position(|c| *c == ']') else {
                return Err(ExprError::new("Unterminated [name]", start, total));
            };
            let close = i + 1 + rel;
            let name = text_of(i + 1, close);
            let name = js_trim(&name).to_string();
            i = close + 1;
            if name.is_empty() {
                return Err(ExprError::new("Empty [name]", start, offs[i]));
            }
            tokens.push(Token {
                kind: Kind::Bracket,
                text: text_of(begin, i),
                value: name,
                number: 0.0,
                start,
                end: offs[i],
            });
            continue;
        }
        if is_ident_start(ch) {
            while i < chars.len() && is_ident_part(chars[i]) {
                i += 1;
            }
            let text = text_of(begin, i);
            let lower = text.to_lowercase();
            let is_kw = KEYWORDS.contains(&lower.as_str());
            tokens.push(Token {
                kind: if is_kw { Kind::Kw } else { Kind::Ident },
                value: if is_kw { lower } else { text.clone() },
                text,
                number: 0.0,
                start,
                end: offs[i],
            });
            continue;
        }
        let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
        let op = OPERATORS
            .iter()
            .chain(std::iter::once(&"."))
            .find(|o| rest.starts_with(**o));
        let Some(op) = op else {
            return Err(ExprError::new(
                format!("Unexpected character '{ch}'"),
                start,
                start + 1,
            ));
        };
        i += op.chars().count();
        tokens.push(Token {
            kind: Kind::Op,
            text: (*op).to_string(),
            value: String::new(),
            number: 0.0,
            start,
            end: offs[i],
        });
    }
    tokens.push(Token {
        kind: Kind::Eof,
        text: String::new(),
        value: String::new(),
        number: 0.0,
        start: total,
        end: total,
    });
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
}

fn node(start: usize, end: usize, node: Node) -> Ast {
    Ast { start, end, node }
}

fn binary(op: BinaryOp, left: Ast, right: Ast) -> Ast {
    node(
        left.start,
        right.end,
        Node::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        },
    )
}

impl Parser {
    fn parse_root(&mut self) -> Parsed<Ast> {
        if self.peek(0).kind == Kind::Eof {
            return Err(ExprError::new("Expression is empty", 0, 0));
        }
        let ast = self.parse_or()?;
        let next = self.peek(0);
        if next.kind != Kind::Eof {
            return Err(Self::unexpected(next));
        }
        Ok(ast)
    }

    fn peek(&self, offset: usize) -> &Token {
        &self.tokens[(self.pos + offset).min(self.tokens.len() - 1)]
    }

    fn next(&mut self) -> Token {
        let t = self.peek(0).clone();
        if t.kind != Kind::Eof {
            self.pos += 1;
        }
        t
    }

    fn is_op(&self, text: &str, offset: usize) -> bool {
        let t = self.peek(offset);
        t.kind == Kind::Op && t.text == text
    }

    fn is_kw(&self, word: &str, offset: usize) -> bool {
        let t = self.peek(offset);
        t.kind == Kind::Kw && t.value == word
    }

    fn expect_op(&mut self, text: &str) -> Parsed<Token> {
        if !self.is_op(text, 0) {
            let t = self.peek(0);
            let message = if t.kind == Kind::Eof {
                format!("Expected '{text}' but the expression ended")
            } else {
                format!("Expected '{text}'")
            };
            return Err(ExprError::new(message, t.start, t.end));
        }
        Ok(self.next())
    }

    fn unexpected(t: &Token) -> ExprError {
        if t.kind == Kind::Eof {
            return ExprError::new("Unexpected end of expression", t.start, t.end);
        }
        ExprError::new(format!("Unexpected '{}'", t.text), t.start, t.end)
    }

    fn enter(&mut self, t: &Token) -> Parsed<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(ExprError::new(
                "Expression is nested too deeply",
                t.start,
                t.start,
            ));
        }
        Ok(())
    }

    fn parse_or(&mut self) -> Parsed<Ast> {
        let mut left = self.parse_and()?;
        while self.is_kw("or", 0) || self.is_op("||", 0) {
            self.next();
            let right = self.parse_and()?;
            left = binary(BinaryOp::Or, left, right);
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Parsed<Ast> {
        let mut left = self.parse_not()?;
        while self.is_kw("and", 0) || self.is_op("&&", 0) {
            self.next();
            let right = self.parse_not()?;
            left = binary(BinaryOp::And, left, right);
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Parsed<Ast> {
        if self.is_kw("not", 0) || self.is_op("!", 0) {
            let t = self.next();
            self.enter(&t)?;
            let operand = self.parse_not()?;
            self.depth -= 1;
            return Ok(node(
                t.start,
                operand.end,
                Node::Unary {
                    op: UnaryOp::Not,
                    operand: Box::new(operand),
                },
            ));
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Parsed<Ast> {
        let left = self.parse_concat()?;
        let (ast, changed) = self.parse_comparison_tail(left)?;
        if changed && self.at_comparison() {
            let t = self.peek(0);
            return Err(ExprError::new(
                "Comparisons can't be chained; combine them with 'and'",
                t.start,
                t.end,
            ));
        }
        Ok(ast)
    }

    fn at_comparison(&self) -> bool {
        let t = self.peek(0);
        if t.kind == Kind::Op && comparison_op(&t.text).is_some() {
            return true;
        }
        if self.is_kw("in", 0) || self.is_kw("between", 0) || self.is_kw("is", 0) {
            return true;
        }
        self.is_kw("not", 0) && (self.is_kw("in", 1) || self.is_kw("between", 1))
    }

    fn parse_comparison_tail(&mut self, left: Ast) -> Parsed<(Ast, bool)> {
        let t = self.peek(0).clone();
        if t.kind == Kind::Op {
            if let Some(op) = comparison_op(&t.text) {
                self.next();
                let right = self.parse_concat()?;
                return Ok((binary(op, left, right), true));
            }
        }
        if self.is_kw("is", 0) {
            self.next();
            let negated = self.is_kw("not", 0);
            if negated {
                self.next();
            }
            let n = self.peek(0).clone();
            if !self.is_kw("null", 0) {
                return Err(ExprError::new("Expected 'null' after 'is'", n.start, n.end));
            }
            self.next();
            let ast = node(
                left.start,
                n.end,
                Node::IsNull {
                    operand: Box::new(left),
                    negated,
                },
            );
            return Ok((ast, true));
        }
        let mut negated = false;
        if self.is_kw("not", 0) && (self.is_kw("in", 1) || self.is_kw("between", 1)) {
            self.next();
            negated = true;
        }
        if self.is_kw("in", 0) {
            self.next();
            self.expect_op("(")?;
            let mut list = Vec::new();
            if !self.is_op(")", 0) {
                list.push(self.parse_or()?);
                while self.is_op(",", 0) {
                    self.next();
                    list.push(self.parse_or()?);
                }
            }
            let close = self.expect_op(")")?;
            let ast = node(
                left.start,
                close.end,
                Node::In {
                    operand: Box::new(left),
                    list,
                    negated,
                },
            );
            return Ok((ast, true));
        }
        if self.is_kw("between", 0) {
            self.next();
            let low = self.parse_concat()?;
            if !self.is_kw("and", 0) {
                let n = self.peek(0);
                return Err(ExprError::new(
                    "Expected 'and' in 'between … and …'",
                    n.start,
                    n.end,
                ));
            }
            self.next();
            let high = self.parse_concat()?;
            let ast = node(
                left.start,
                high.end,
                Node::Between {
                    operand: Box::new(left),
                    low: Box::new(low),
                    high: Box::new(high),
                    negated,
                },
            );
            return Ok((ast, true));
        }
        Ok((left, false))
    }

    fn parse_concat(&mut self) -> Parsed<Ast> {
        let mut left = self.parse_additive()?;
        while self.is_op("&", 0) {
            self.next();
            let right = self.parse_additive()?;
            left = binary(BinaryOp::Concat, left, right);
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Parsed<Ast> {
        let mut left = self.parse_multiplicative()?;
        while self.is_op("+", 0) || self.is_op("-", 0) {
            let op = if self.next().text == "+" {
                BinaryOp::Add
            } else {
                BinaryOp::Sub
            };
            let right = self.parse_multiplicative()?;
            left = binary(op, left, right);
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Parsed<Ast> {
        let mut left = self.parse_unary()?;
        while self.is_op("*", 0) || self.is_op("/", 0) || self.is_op("%", 0) {
            let op = match self.next().text.as_str() {
                "*" => BinaryOp::Mul,
                "/" => BinaryOp::Div,
                _ => BinaryOp::Rem,
            };
            let right = self.parse_unary()?;
            left = binary(op, left, right);
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Parsed<Ast> {
        if self.is_op("-", 0) || self.is_op("+", 0) {
            let t = self.next();
            self.enter(&t)?;
            let operand = self.parse_unary()?;
            self.depth -= 1;
            if t.text == "+" {
                return Ok(Ast {
                    start: t.start,
                    ..operand
                });
            }
            if let Node::Literal(Value::Number(n)) = &operand.node {
                let value = -n.as_f64().unwrap_or(0.0);
                let value = if value == 0.0 { 0.0 } else { value };
                return Ok(node(t.start, operand.end, Node::Literal(num_value(value))));
            }
            return Ok(node(
                t.start,
                operand.end,
                Node::Unary {
                    op: UnaryOp::Neg,
                    operand: Box::new(operand),
                },
            ));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Parsed<Ast> {
        let t = self.peek(0).clone();
        match t.kind {
            Kind::Num => {
                self.next();
                Ok(node(t.start, t.end, Node::Literal(num_value(t.number))))
            }
            Kind::Str => {
                self.next();
                Ok(node(t.start, t.end, Node::Literal(Value::String(t.value))))
            }
            Kind::Date => {
                self.next();
                Ok(node(t.start, t.end, Node::Date(t.value)))
            }
            Kind::Kw => {
                let value = match t.value.as_str() {
                    "null" => Value::Null,
                    "true" => Value::Bool(true),
                    "false" => Value::Bool(false),
                    _ => return Err(Self::unexpected(&t)),
                };
                self.next();
                Ok(node(t.start, t.end, Node::Literal(value)))
            }
            Kind::Ident => {
                if self.is_op("(", 1) {
                    self.parse_call()
                } else {
                    self.parse_path()
                }
            }
            Kind::Bracket => self.parse_path(),
            Kind::Op if t.text == "(" => {
                self.next();
                self.enter(&t)?;
                let inner = self.parse_or()?;
                self.depth -= 1;
                let close = self.expect_op(")")?;
                Ok(Ast {
                    start: t.start,
                    end: close.end,
                    ..inner
                })
            }
            _ => Err(Self::unexpected(&t)),
        }
    }

    fn parse_call(&mut self) -> Parsed<Ast> {
        let name = self.next();
        self.enter(&name)?;
        self.expect_op("(")?;
        let mut args = Vec::new();
        if !self.is_op(")", 0) {
            args.push(self.parse_or()?);
            while self.is_op(",", 0) {
                self.next();
                args.push(self.parse_or()?);
            }
        }
        let close = self.expect_op(")")?;
        self.depth -= 1;
        Ok(node(
            name.start,
            close.end,
            Node::Call {
                name: name.text.to_lowercase(),
                args,
                name_end: name.end,
            },
        ))
    }

    fn parse_path(&mut self) -> Parsed<Ast> {
        let first = self.next();
        let mut path = vec![Self::segment(&first)?];
        let mut end = first.end;
        while self.is_op(".", 0) {
            self.next();
            let seg = self.peek(0).clone();
            if !matches!(seg.kind, Kind::Ident | Kind::Bracket | Kind::Kw) {
                return Err(ExprError::new(
                    "Expected a field name after '.'",
                    seg.start,
                    seg.end,
                ));
            }
            self.next();
            path.push(Self::segment(&seg)?);
            end = seg.end;
        }
        Ok(node(first.start, end, Node::Name(path)))
    }

    fn segment(t: &Token) -> Parsed<String> {
        let name = if t.value.is_empty() {
            &t.text
        } else {
            &t.value
        };
        if BLOCKED_NAMES.contains(&name.as_str()) {
            return Err(ExprError::new(
                format!("The name '{name}' is not allowed"),
                t.start,
                t.end,
            ));
        }
        Ok(name.clone())
    }
}

pub fn parse_source(src: &str) -> Parsed<Ast> {
    if src.encode_utf16().count() > MAX_SOURCE_LENGTH {
        return Err(ExprError::new(
            format!("Expression is longer than {MAX_SOURCE_LENGTH} characters"),
            0,
            0,
        ));
    }
    let mut parser = Parser {
        tokens: tokenize(src)?,
        pos: 0,
        depth: 0,
    };
    parser.parse_root()
}
