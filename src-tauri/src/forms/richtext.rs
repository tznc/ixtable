//! Rich-text fields store a small HTML subset (port of `src/fields/richtext.ts`).
//! Everything is sanitized on the way in (editor output) and again before display,
//! because a value can also arrive from an import, an action, or another
//! PostgreSQL client. The scanner below reads tags and text without a DOM.

const ALLOWED: &[&str] = &[
    "p",
    "br",
    "div",
    "b",
    "strong",
    "i",
    "em",
    "u",
    "s",
    "ul",
    "ol",
    "li",
    "a",
    "h1",
    "h2",
    "h3",
    "blockquote",
];
/// Elements whose content is dropped with them, not kept as text.
const DROPPED: &[&str] = &[
    "script", "style", "template", "iframe", "object", "embed", "noscript",
];
const BLOCKS: &[&str] = &["p", "div", "li", "h1", "h2", "h3", "blockquote", "br"];
const VOID: &[&str] = &["br", "img", "hr", "input", "meta", "link", "wbr"];

#[derive(Debug)]
pub(super) enum Token {
    Text(String),
    Open {
        name: String,
        href: Option<String>,
        self_closing: bool,
    },
    Close(String),
}

fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let decoded = rest.find(';').filter(|end| *end <= 10).and_then(|end| {
            let name = &rest[1..end];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ => name.strip_prefix('#').and_then(|n| {
                    let code = match n.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => n.parse().ok(),
                    };
                    code.and_then(char::from_u32)
                }),
            };
            c.map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            c => out.push(c),
        }
    }
    out
}

fn escape_attribute(text: &str) -> String {
    escape_text(text).replace('"', "&quot;")
}

/// The `href` attribute of a tag's source text (between the name and `>`).
fn href_of(attributes: &str) -> Option<String> {
    let lower = attributes.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find("href") {
        let at = from + found;
        from = at + 4;
        let boundary = at == 0
            || !lower.as_bytes()[at - 1].is_ascii_alphanumeric() && lower.as_bytes()[at - 1] != b'-';
        if !boundary {
            continue;
        }
        let after = attributes[at + 4..].trim_start();
        let Some(after) = after.strip_prefix('=') else {
            continue;
        };
        let after = after.trim_start();
        let value = match after.chars().next() {
            Some(q @ ('"' | '\'')) => after[1..].split(q).next().unwrap_or("").to_string(),
            _ => after
                .split(|c: char| c.is_whitespace() || c == '>')
                .next()
                .unwrap_or("")
                .to_string(),
        };
        return Some(decode_entities(&value));
    }
    None
}

pub(super) fn tokens(html: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut rest = html;
    while !rest.is_empty() {
        let Some(at) = rest.find('<') else {
            out.push(Token::Text(decode_entities(rest)));
            break;
        };
        if at > 0 {
            out.push(Token::Text(decode_entities(&rest[..at])));
        }
        rest = &rest[at..];
        if let Some(body) = rest.strip_prefix("<!--") {
            rest = body.find("-->").map_or("", |end| &body[end + 3..]);
            continue;
        }
        // A tag ends at the first `>` outside quotes.
        let mut quote: Option<char> = None;
        let mut end = None;
        for (i, c) in rest.char_indices().skip(1) {
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (None, '"' | '\'') => quote = Some(c),
                (None, '>') => {
                    end = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            out.push(Token::Text(decode_entities(rest)));
            break;
        };
        let inner = &rest[1..end];
        rest = &rest[end + 1..];
        let closing = inner.starts_with('/');
        let inner = inner.trim_start_matches('/');
        let name: String = inner
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if name.is_empty() || inner.starts_with('!') || inner.starts_with('?') {
            continue;
        }
        if closing {
            out.push(Token::Close(name));
        } else {
            let attributes = &inner[name.len()..];
            out.push(Token::Open {
                href: if name == "a" { href_of(attributes) } else { None },
                self_closing: attributes.trim_end().ends_with('/') || VOID.contains(&name.as_str()),
                name,
            });
        }
    }
    out
}

/// The allowed subset of `html`: formatting tags only, links limited to http(s) and mailto.
pub fn sanitize_rich_text(html: &str) -> String {
    if html.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    // Open allowed elements, innermost last.
    let mut stack: Vec<String> = Vec::new();
    // Depth inside a dropped element (its text and children are skipped).
    let mut dropped: Vec<String> = Vec::new();
    for token in tokens(html) {
        if let Some(open) = dropped.last() {
            match &token {
                Token::Close(name) if name == open => {
                    dropped.pop();
                }
                Token::Open { name, self_closing, .. } if name == open && !self_closing => {
                    dropped.push(name.clone())
                }
                _ => {}
            }
            continue;
        }
        match token {
            Token::Text(text) => out.push_str(&escape_text(&text)),
            Token::Open {
                name,
                href,
                self_closing,
            } => {
                if DROPPED.contains(&name.as_str()) {
                    if !self_closing {
                        dropped.push(name);
                    }
                    continue;
                }
                if !ALLOWED.contains(&name.as_str()) {
                    continue;
                }
                out.push('<');
                out.push_str(&name);
                if name == "a" {
                    let href = href.map(|h| h.trim().to_string()).unwrap_or_default();
                    let lower = href.to_ascii_lowercase();
                    if lower.starts_with("http:")
                        || lower.starts_with("https:")
                        || lower.starts_with("mailto:")
                    {
                        out.push_str(&format!(
                            " href=\"{}\" rel=\"noopener noreferrer\" target=\"_blank\"",
                            escape_attribute(&href)
                        ));
                    }
                }
                out.push('>');
                if !VOID.contains(&name.as_str()) && !self_closing {
                    stack.push(name);
                }
            }
            Token::Close(name) => {
                if let Some(at) = stack.iter().rposition(|n| *n == name) {
                    while stack.len() > at {
                        let open = stack.pop().expect("open element");
                        out.push_str(&format!("</{open}>"));
                    }
                }
            }
        }
    }
    while let Some(open) = stack.pop() {
        out.push_str(&format!("</{open}>"));
    }
    out
}

/// Plain text of a rich-text value, for lists, datasheets, reports, and validation.
pub fn rich_text_plain(html: Option<&str>) -> String {
    let Some(text) = html else {
        return String::new();
    };
    if !text.contains(['<', '&']) {
        return text.to_string();
    }
    let mut parts = String::new();
    let mut dropped: Vec<String> = Vec::new();
    for token in tokens(text) {
        if let Some(open) = dropped.last() {
            match &token {
                Token::Close(name) if name == open => {
                    dropped.pop();
                }
                Token::Open { name, self_closing, .. } if name == open && !self_closing => {
                    dropped.push(name.clone())
                }
                _ => {}
            }
            continue;
        }
        match token {
            Token::Text(t) => parts.push_str(&t),
            Token::Open {
                name, self_closing, ..
            } => {
                if DROPPED.contains(&name.as_str()) {
                    if !self_closing {
                        dropped.push(name);
                    }
                } else if BLOCKS.contains(&name.as_str()) && !parts.is_empty() {
                    parts.push('\n');
                }
            }
            Token::Close(_) => {}
        }
    }
    let mut collapsed = String::new();
    let mut newlines = 0;
    for c in parts.chars() {
        if c == '\n' {
            newlines += 1;
            if newlines == 1 {
                collapsed.push(c);
            }
        } else {
            newlines = 0;
            collapsed.push(c);
        }
    }
    collapsed.trim().to_string()
}
