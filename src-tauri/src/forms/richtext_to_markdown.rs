//! Stored rich-text HTML written back as the editor's Markdown (`richtext_markup.rs` reads it).
//! The sanitized HTML becomes a small tree, and each element is written as its Markdown, with
//! text that would read as Markdown escaped, so a value survives being opened and saved.

use super::richtext::{Token, sanitize_rich_text, tokens};

#[derive(Clone)]
enum El {
    Text(String),
    Tag {
        name: String,
        attrs: Vec<(String, String)>,
        children: Vec<El>,
    },
}

const VOID: &[&str] = &["br", "img", "hr", "input"];
const BLOCK: &[&str] = &[
    "p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "blockquote", "ul", "ol", "li", "pre", "hr",
    "table", "thead", "tbody", "tr",
];

fn tree(html: &str) -> Vec<El> {
    // Each frame holds an open element's name, attributes and children; the root is the first.
    let mut stack: Vec<(String, Vec<(String, String)>, Vec<El>)> = vec![(String::new(), vec![], vec![])];
    for token in tokens(html) {
        match token {
            Token::Text(text) => stack.last_mut().expect("root").2.push(El::Text(text)),
            Token::Open { name, attrs, .. } if VOID.contains(&name.as_str()) => {
                stack.last_mut().expect("root").2.push(El::Tag { name, attrs, children: vec![] })
            }
            Token::Open { name, attrs, .. } => stack.push((name, attrs, vec![])),
            Token::Close(name) => {
                if !stack.iter().skip(1).any(|(n, ..)| *n == name) {
                    continue;
                }
                while let Some((open, attrs, children)) = stack.pop() {
                    stack.last_mut().expect("root").2.push(El::Tag { name: open.clone(), attrs, children });
                    if open == name {
                        break;
                    }
                }
            }
        }
    }
    while stack.len() > 1 {
        let (name, attrs, children) = stack.pop().expect("open element");
        stack.last_mut().expect("root").2.push(El::Tag { name, attrs, children });
    }
    stack.pop().expect("root").2
}

fn is_block(el: &El) -> bool {
    matches!(el, El::Tag { name, .. } if BLOCK.contains(&name.as_str()))
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    Token::attr(attrs, name)
}

/// Backslash-escapes the characters that would read as inline Markdown.
fn escape_inline(text: &str, in_table: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        let entity = c == '&' && chars.get(i + 1).is_some_and(|n| n.is_ascii_alphanumeric() || *n == '#');
        if matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '~') || entity || in_table && c == '|' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A paragraph line that would start a block keeps that character literal.
fn escape_line_start(line: &str) -> String {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && digits < 10 && line[digits..].starts_with(['.', ')']) {
        return format!("{}\\{}", &line[..digits], &line[digits..]);
    }
    match line.chars().next() {
        Some('#' | '>' | '-' | '+' | '=' | '|') => format!("\\{line}"),
        _ => line.to_string(),
    }
}

fn link_destination(url: &str) -> String {
    if url.contains([' ', '(', ')', '<', '>']) {
        format!("<{}>", url.replace('<', "%3C").replace('>', "%3E"))
    } else {
        url.to_string()
    }
}

fn title_part(attrs: &[(String, String)]) -> String {
    attr(attrs, "title").map_or(String::new(), |t| format!(" \"{}\"", t.replace('"', "\\\"")))
}

/// The text content of an element, unescaped (for code).
fn text_content(els: &[El], out: &mut String) {
    for el in els {
        match el {
            El::Text(t) => out.push_str(t),
            El::Tag { name, .. } if name == "br" => out.push('\n'),
            El::Tag { children, .. } => text_content(children, out),
        }
    }
}

/// A code span around `code`, with more backticks than any run inside it.
fn code_span(code: &str) -> String {
    let longest = code.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    let pad = if code.starts_with('`') || code.ends_with('`') || code.starts_with(' ') && code.ends_with(' ') && !code.trim().is_empty() {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{code}{pad}{fence}")
}

struct Inline {
    out: String,
    in_table: bool,
}

impl Inline {
    fn text(&mut self, text: &str) {
        let mut collapsed = String::new();
        for c in text.chars() {
            if c.is_whitespace() && c != '\u{a0}' {
                if !collapsed.ends_with(' ') {
                    collapsed.push(' ');
                }
            } else {
                collapsed.push(c);
            }
        }
        if self.out.is_empty() || self.out.ends_with([' ', '\n']) {
            collapsed = collapsed.trim_start().to_string();
        }
        self.out.push_str(&escape_inline(&collapsed, self.in_table));
    }

    /// `open`/`close` around the content, with outer spaces moved outside the markers.
    fn wrapped(&mut self, open: &str, close: &str, children: &[El]) {
        let mut inner = Inline { out: String::new(), in_table: self.in_table };
        inner.all(children);
        let body = inner.out;
        if body.trim().is_empty() {
            self.out.push_str(&body);
            return;
        }
        let lead = &body[..body.len() - body.trim_start().len()];
        let trail = &body[body.trim_end().len()..];
        if !(self.out.is_empty() || self.out.ends_with([' ', '\n'])) || lead.is_empty() {
            self.out.push_str(lead);
        }
        self.out.push_str(open);
        self.out.push_str(body.trim());
        self.out.push_str(close);
        self.out.push_str(trail);
    }

    fn all(&mut self, els: &[El]) {
        for el in els {
            self.el(el);
        }
    }

    fn el(&mut self, el: &El) {
        let El::Tag { name, attrs, children } = el else {
            if let El::Text(t) = el {
                self.text(t);
            }
            return;
        };
        match name.as_str() {
            "b" | "strong" => self.wrapped("**", "**", children),
            "i" | "em" => self.wrapped("*", "*", children),
            "s" | "del" => self.wrapped("~~", "~~", children),
            "u" => self.wrapped("<u>", "</u>", children),
            "code" => {
                let mut code = String::new();
                text_content(children, &mut code);
                self.out.push_str(&code_span(&code.replace('\n', " ")));
            }
            "br" if self.in_table => self.out.push_str("<br>"),
            "br" => {
                let trimmed = self.out.trim_end_matches(' ').len();
                self.out.truncate(trimmed);
                self.out.push_str("\\\n");
            }
            "a" => match attr(attrs, "href") {
                Some(href) => {
                    let mut label = Inline { out: String::new(), in_table: self.in_table };
                    label.all(children);
                    self.out.push_str(&format!("[{}]({}{})", label.out.trim(), link_destination(href), title_part(attrs)));
                }
                None => self.all(children),
            },
            "img" => {
                let alt = escape_inline(attr(attrs, "alt").unwrap_or_default(), self.in_table);
                let src = attr(attrs, "src").unwrap_or_default();
                self.out.push_str(&format!("![{alt}]({}{})", link_destination(src), title_part(attrs)));
            }
            // A task box only means something at the start of a list item (`list_item`).
            "input" => {}
            _ => self.all(children),
        }
    }

    /// The finished paragraph: no hard break at the ends, and each line safe from block syntax.
    fn finish(self) -> String {
        let mut text = self.out.trim().to_string();
        while let Some(rest) = text.strip_suffix('\\') {
            text = rest.trim_end().to_string();
        }
        text.split('\n').map(|line| escape_line_start(line.trim_start())).collect::<Vec<_>>().join("\n")
    }
}

fn inline(els: &[El]) -> String {
    let mut inline = Inline { out: String::new(), in_table: false };
    inline.all(els);
    inline.finish()
}

fn prefixed(block: &str, first: &str, rest: &str) -> String {
    block
        .split('\n')
        .enumerate()
        .map(|(i, line)| {
            let prefix = if i == 0 { first } else { rest };
            if line.is_empty() { prefix.trim_end().to_string() } else { format!("{prefix}{line}") }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The Markdown blocks of a run of elements; inline runs between blocks become paragraphs.
fn blocks(els: &[El]) -> Vec<String> {
    let mut out = vec![];
    let mut run: Vec<&El> = vec![];
    let flush = |run: &mut Vec<&El>, out: &mut Vec<String>| {
        if run.is_empty() {
            return;
        }
        let mut inline = Inline { out: String::new(), in_table: false };
        for el in run.drain(..) {
            inline.el(el);
        }
        let text = inline.finish();
        if !text.is_empty() {
            out.push(text);
        }
    };
    for el in els {
        if !is_block(el) {
            run.push(el);
            continue;
        }
        flush(&mut run, &mut out);
        let El::Tag { name, attrs, children } = el else { continue };
        match name.as_str() {
            "p" | "div" if children.iter().any(is_block) => out.extend(blocks(children)),
            "p" | "div" => {
                let text = inline(children);
                if !text.is_empty() {
                    out.push(text);
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level: usize = name[1..].parse().unwrap_or(1);
                let text = inline(children).replace("\\\n", " ").replace('\n', " ");
                out.push(format!("{} {text}", "#".repeat(level)).trim_end().to_string());
            }
            "blockquote" => {
                let inner = blocks(children).join("\n\n");
                out.push(prefixed(if inner.is_empty() { "" } else { &inner }, "> ", "> "));
            }
            "ul" | "ol" => out.push(list(name == "ol", attrs, children)),
            "li" => out.push(list(false, &[], std::slice::from_ref(el))),
            "pre" => {
                let mut code = String::new();
                text_content(children, &mut code);
                let language = children.iter().find_map(|c| match c {
                    El::Tag { name, attrs, .. } if name == "code" => {
                        attr(attrs, "class").and_then(|c| c.strip_prefix("language-")).map(str::to_string)
                    }
                    _ => None,
                });
                let longest = code.split(|c| c != '`').map(str::len).max().unwrap_or(0);
                let fence = "`".repeat(longest.max(2) + 1);
                let code = code.strip_suffix('\n').unwrap_or(&code);
                out.push(format!("{fence}{}\n{code}\n{fence}", language.unwrap_or_default()));
            }
            "hr" => out.push("---".into()),
            "table" => {
                if let Some(table) = table(children) {
                    out.push(table);
                }
            }
            // A stray row or section outside a table keeps its text.
            _ => out.extend(blocks(children)),
        }
    }
    flush(&mut run, &mut out);
    out
}

fn list(ordered: bool, attrs: &[(String, String)], items: &[El]) -> String {
    let start: usize = attr(attrs, "start").and_then(|s| s.parse().ok()).unwrap_or(1);
    let mut out: Vec<String> = vec![];
    let mut loose = false;
    let mut n = start;
    for item in items {
        let El::Tag { name, children, .. } = item else { continue };
        if name != "li" {
            continue;
        }
        let marker = if ordered { format!("{n}. ") } else { "- ".into() };
        n += 1;
        // A task box first in the item becomes `[ ]` or `[x]`.
        let mut children: &[El] = children;
        let mut task = "";
        let first_real = children.iter().position(|c| !matches!(c, El::Text(t) if t.trim().is_empty()));
        if let Some(at) = first_real {
            if let El::Tag { name, attrs, .. } = &children[at] {
                if name == "input" {
                    task = if attr(attrs, "checked").is_some() { "[x] " } else { "[ ] " };
                    children = &children[at + 1..];
                }
            }
        }
        // Nested lists sit tight under the item's text; other blocks are separated by a blank line.
        let mut parts: Vec<(String, bool)> = vec![];
        let mut run: Vec<El> = vec![];
        for child in children {
            match child {
                El::Tag { name, attrs, children } if name == "ul" || name == "ol" => {
                    parts.extend(blocks(&std::mem::take(&mut run)).into_iter().map(|b| (b, false)));
                    // An ordered list that does not start at 1 cannot interrupt a paragraph.
                    let tight = name == "ul" || attr(attrs, "start").is_none_or(|s| s == "1");
                    parts.push((list(name == "ol", attrs, children), tight));
                }
                other => run.push(other.clone()),
            }
        }
        parts.extend(blocks(&run).into_iter().map(|b| (b, false)));
        let mut body = String::new();
        let mut prose = 0;
        for (i, (part, nested)) in parts.iter().enumerate() {
            if i > 0 {
                body.push_str(if *nested { "\n" } else { "\n\n" });
            }
            if !nested {
                prose += 1;
            }
            body.push_str(part);
        }
        loose |= prose > 1;
        let indent = " ".repeat(marker.len());
        out.push(prefixed(&format!("{task}{body}"), &marker, &indent).trim_end().to_string());
    }
    out.join(if loose { "\n\n" } else { "\n" })
}

fn rows(els: &[El], out: &mut Vec<Vec<(String, Option<String>)>>) {
    for el in els {
        let El::Tag { name, children, .. } = el else { continue };
        match name.as_str() {
            "tr" => out.push(
                children
                    .iter()
                    .filter_map(|cell| match cell {
                        El::Tag { name, attrs, children } if name == "td" || name == "th" => {
                            let mut inline = Inline { out: String::new(), in_table: true };
                            inline.all(children);
                            Some((inline.out.trim().to_string(), attr(attrs, "align").map(str::to_string)))
                        }
                        _ => None,
                    })
                    .collect(),
            ),
            _ => rows(children, out),
        }
    }
}

fn table(children: &[El]) -> Option<String> {
    let mut all = vec![];
    rows(children, &mut all);
    let columns = all.iter().map(Vec::len).max().filter(|n| *n > 0)?;
    let line = |row: &[(String, Option<String>)]| {
        let cells: Vec<&str> = (0..columns).map(|i| row.get(i).map_or("", |c| c.0.as_str())).collect();
        format!("| {} |", cells.join(" | "))
    };
    let header = &all[0];
    let delimiter: Vec<&str> = (0..columns)
        .map(|i| match header.get(i).and_then(|c| c.1.as_deref()) {
            Some("left") => ":---",
            Some("center") => ":---:",
            Some("right") => "---:",
            _ => "---",
        })
        .collect();
    let mut out = vec![line(header), format!("| {} |", delimiter.join(" | "))];
    out.extend(all[1..].iter().map(|row| line(row)));
    Some(out.join("\n"))
}

/// The Markdown the editor shows for a stored rich-text value.
pub fn rich_text_markup(html: &str) -> String {
    blocks(&tree(&sanitize_rich_text(html))).join("\n\n")
}
