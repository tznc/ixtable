//! Parser for the Access "SaveAsText" format: forms, reports, macros and query
//! definitions in template packages (`docs/access-format.md` §3).
//!
//! The format is line based. A block opens with `Begin [Name]` and closes with
//! `End`. A property is `Name =Value` where the value is a quoted string (which
//! may continue on following lines that hold only another quoted string), a bare
//! token (`-1`, `NotDefault`, `65.0`), or `= Begin` followed by either hex lines
//! (`0x...`) for binary data or a nested property block (embedded macros).
//! Query definitions add typed properties: `dbText "Name" ="Value"`.
use std::fmt;

/// A property value.
#[derive(Debug, Clone, PartialEq)]
pub enum PropValue {
    /// A quoted string with escapes resolved.
    Str(String),
    /// A bare token: number, `NotDefault`, ...
    Token(String),
    /// Hex data from a `= Begin` block.
    Binary(Vec<u8>),
    /// A nested block from a `= Begin` property (embedded macro).
    Block(Node),
}

/// One item of a block, in file order (repeated keys are meaningful).
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Prop {
        key: String,
        /// The DAO type of a typed property (`dbText`, `dbBoolean`, ...).
        ty: Option<String>,
        value: PropValue,
    },
    Block(Node),
}

/// A block: `Begin <kind>` ... `End`. The file itself is the root, kind "".
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Node {
    pub kind: String,
    pub items: Vec<Item>,
    /// VBA after a `CodeBehindForm` line (root only).
    pub code: Option<String>,
}

impl Node {
    /// The first property `key` as text (string or token).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.items.iter().find_map(|i| match i {
            Item::Prop {
                key: k,
                value: PropValue::Str(s) | PropValue::Token(s),
                ..
            } if k.eq_ignore_ascii_case(key) => Some(s.as_str()),
            _ => None,
        })
    }

    /// Every property `key` as text, in order.
    pub fn props<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.items.iter().filter_map(move |i| match i {
            Item::Prop { key: k, value, .. } if k.eq_ignore_ascii_case(key) => match value {
                PropValue::Str(s) | PropValue::Token(s) => Some(s.as_str()),
                _ => None,
            },
            _ => None,
        })
    }

    pub fn binary(&self, key: &str) -> Option<&[u8]> {
        self.items.iter().find_map(|i| match i {
            Item::Prop {
                key: k,
                value: PropValue::Binary(b),
                ..
            } if k.eq_ignore_ascii_case(key) => Some(b.as_slice()),
            _ => None,
        })
    }

    /// A nested property block (`OnClickEmMacro = Begin ... End`).
    pub fn prop_block(&self, key: &str) -> Option<&Node> {
        self.items.iter().find_map(|i| match i {
            Item::Prop {
                key: k,
                value: PropValue::Block(b),
                ..
            } if k.eq_ignore_ascii_case(key) => Some(b),
            _ => None,
        })
    }

    pub fn int(&self, key: &str) -> Option<i64> {
        self.get(key)
            .and_then(|v| v.trim().parse::<f64>().ok())
            .map(|v| v as i64)
    }

    /// `NotDefault` flags and -1/1 booleans.
    pub fn flag(&self, key: &str) -> Option<bool> {
        self.get(key)
            .map(|v| matches!(v.trim(), "NotDefault" | "-1" | "1" | "True"))
    }

    pub fn blocks(&self) -> impl Iterator<Item = &Node> {
        self.items.iter().filter_map(|i| match i {
            Item::Block(b) => Some(b),
            _ => None,
        })
    }

    pub fn block(&self, kind: &str) -> Option<&Node> {
        self.blocks().find(|b| b.kind.eq_ignore_ascii_case(kind))
    }

    /// Ordered `(key, text)` pairs of a block such as `Begin OutputColumns`.
    pub fn pairs(&self) -> Vec<(&str, &str)> {
        self.items
            .iter()
            .filter_map(|i| match i {
                Item::Prop {
                    key,
                    value: PropValue::Str(s) | PropValue::Token(s),
                    ..
                } => Some((key.as_str(), s.as_str())),
                _ => None,
            })
            .collect()
    }
}

#[derive(Debug, PartialEq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// Decodes a SaveAsText file: UTF-16LE with a BOM, else UTF-8 (with or without BOM).
pub fn decode(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(body) {
        Ok(s) => s.to_string(),
        // Older exports are in the ANSI code page.
        Err(_) => encoding_rs::WINDOWS_1252.decode(body).0.into_owned(),
    }
}

/// Parses the text of a SaveAsText file.
pub fn parse(text: &str) -> Result<Node, ParseError> {
    let lines: Vec<&str> = text.lines().collect();
    let mut p = Parser { lines, pos: 0 };
    let mut root = p.block("", true)?;
    if p.pos < p.lines.len() {
        // Only reachable after `CodeBehindForm`.
        root.code = Some(p.lines[p.pos..].join("\n"));
    }
    Ok(root)
}

struct Parser<'a> {
    lines: Vec<&'a str>,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn err(&self, message: impl Into<String>) -> ParseError {
        // Lines are counted from 1, and `pos` is already past the line read.
        ParseError {
            line: self.pos.max(1),
            message: message.into(),
        }
    }

    fn block(&mut self, kind: &str, root: bool) -> Result<Node, ParseError> {
        let mut node = Node {
            kind: kind.to_string(),
            ..Default::default()
        };
        while self.pos < self.lines.len() {
            let line = self.lines[self.pos].trim();
            self.pos += 1;
            if line.is_empty() {
                continue;
            }
            if line == "End" {
                if root {
                    return Err(self.err("unexpected End"));
                }
                return Ok(node);
            }
            if root && line == "CodeBehindForm" {
                return Ok(node);
            }
            if line == "Begin" || line.starts_with("Begin ") {
                let child_kind = line[5..].trim().to_string();
                let child = self.block(&child_kind, false)?;
                node.items.push(Item::Block(child));
                continue;
            }
            node.items.push(self.property(line)?);
        }
        if root {
            Ok(node)
        } else {
            Err(self.err(format!("missing End for Begin {kind}")))
        }
    }

    fn property(&mut self, line: &str) -> Result<Item, ParseError> {
        let (head, rest) =
            split_assignment(line).ok_or_else(|| self.err(format!("not a property: {line}")))?;
        let (key, ty) = match head.split_once(' ') {
            // dbText "Name"
            Some((ty, name)) if ty.starts_with("db") && name.trim().starts_with('"') => (
                unquote(name.trim()).unwrap_or_default(),
                Some(ty.to_string()),
            ),
            _ => (head.to_string(), None),
        };
        let value = if rest == "Begin" {
            self.begin_value()?
        } else if rest.starts_with('"') {
            let mut text = unquote(rest).ok_or_else(|| self.err("unterminated string"))?;
            // Continuation lines hold only another quoted string.
            while let Some(next) = self.lines.get(self.pos).map(|l| l.trim()) {
                if next.starts_with('"') && next.ends_with('"') && next.len() >= 2 {
                    text.push_str(&unquote(next).ok_or_else(|| self.err("unterminated string"))?);
                    self.pos += 1;
                } else {
                    break;
                }
            }
            PropValue::Str(text)
        } else {
            PropValue::Token(rest.to_string())
        };
        Ok(Item::Prop { key, ty, value })
    }

    /// `Key = Begin`: hex lines up to `End`, or a nested block.
    fn begin_value(&mut self) -> Result<PropValue, ParseError> {
        let first = self.lines.get(self.pos).map(|l| l.trim()).unwrap_or("");
        if first.starts_with("0x") || first == "End" {
            let mut data = vec![];
            while self.pos < self.lines.len() {
                let line = self.lines[self.pos].trim();
                self.pos += 1;
                if line == "End" {
                    return Ok(PropValue::Binary(data));
                }
                let hex = line.trim_end_matches(',').trim().trim_start_matches("0x");
                data.extend(hex_bytes(hex).ok_or_else(|| self.err("bad hex data"))?);
            }
            return Err(self.err("missing End after binary data"));
        }
        Ok(PropValue::Block(self.block("", false)?))
    }
}

/// Splits `Key =Value` / `Key = Begin`. Keys may hold spaces in typed properties.
fn split_assignment(line: &str) -> Option<(&str, &str)> {
    // The key ends at the first '=' outside quotes.
    let mut in_quote = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quote = !in_quote,
            '=' if !in_quote => return Some((line[..i].trim(), line[i + 1..].trim())),
            _ => {}
        }
    }
    None
}

/// Resolves a quoted string: `\"`, `\\`, and octal `\015` escapes.
fn unquote(s: &str) -> Option<String> {
    let inner = s.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some(d) if d.is_ascii_digit() => {
                let mut code = 0u32;
                for _ in 0..3 {
                    match chars.peek().and_then(|d| d.to_digit(8)) {
                        Some(v) => {
                            code = code * 8 + v;
                            chars.next();
                        }
                        None => break,
                    }
                }
                out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
            }
            Some(other) => {
                out.push(other);
                chars.next();
            }
            None => out.push('\\'),
        }
    }
    Some(out)
}

fn hex_bytes(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}
