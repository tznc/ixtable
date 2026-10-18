//! Rich text edited as light markup. GPUI has no content-editable box, so the form's rich-text
//! editor shows the stored HTML as Markdown-like text, turns it back into the HTML subset on
//! every change, and renders the result beside it. The toolbar edits the markup.
//!
//! Markup: `**bold**`, `*italic*`, `++underline++`, `~~strike~~` and `[text](https://…)` inside a
//! line. A line starting `- ` is a bullet, `1. ` a numbered item, `# ` to `### ` a heading and
//! `> ` a quote; any other line is a paragraph. `\` keeps the next marker character literal.

use super::richtext::{Token, sanitize_rich_text, tokens};
use std::ops::Range;

/// A toolbar edit's result: the new markup and the range to select in it.
#[derive(Debug, PartialEq)]
pub struct Edit {
    pub text: String,
    pub selection: Range<usize>,
}

const INLINE: &[(&str, &str)] = &[("**", "b"), ("++", "u"), ("~~", "s"), ("*", "i")];

fn inline_marker(tag: &str) -> Option<&'static str> {
    match tag {
        "b" | "strong" => Some("**"),
        "i" | "em" => Some("*"),
        "u" => Some("++"),
        "s" => Some("~~"),
        _ => None,
    }
}

// ---- HTML to markup -----------------------------------------------------------------------

fn escape_inline(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        let doubled = |c: char| chars.get(i + 1) == Some(&c) || i > 0 && chars[i - 1] == c;
        let escape = match c {
            '\\' | '*' | '[' | ']' => true,
            '+' | '~' => doubled(c),
            _ => false,
        };
        if escape {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// What the start of a plain line would be read as, if it is a block prefix.
fn block_prefix(line: &str) -> Option<(Block, usize)> {
    if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        return Some((Block::Bullet, line.len() - rest.len()));
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && line[digits..].starts_with(". ") {
        return Some((Block::Numbered, digits + 2));
    }
    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    if (1..=3).contains(&hashes) && line[hashes..].starts_with(' ') {
        return Some((Block::Heading(hashes), hashes + 1));
    }
    if let Some(rest) = line.strip_prefix("> ") {
        return Some((Block::Quote, line.len() - rest.len()));
    }
    if line == ">" {
        return Some((Block::Quote, 1));
    }
    None
}

struct Lines {
    out: Vec<String>,
    prefix: String,
    text: String,
    open: bool,
    quotes: usize,
    lists: Vec<Option<usize>>,
    links: Vec<Option<String>>,
}

impl Lines {
    fn plain_prefix(&self) -> String {
        if self.quotes > 0 { "> ".into() } else { String::new() }
    }

    fn flush(&mut self) {
        let text = self.text.trim_end().to_string();
        let mut line = std::mem::take(&mut self.prefix);
        // A paragraph that reads like a block prefix keeps its first character literal.
        if line.is_empty() && block_prefix(&text).is_some() {
            let first = text.chars().next().map_or(0, char::len_utf8);
            let digits = text.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 {
                line.push_str(&text[..digits]);
                line.push('\\');
                line.push_str(&text[digits..]);
            } else {
                line.push('\\');
                line.push_str(&text[..first]);
                line.push_str(&text[first..]);
            }
        } else {
            line.push_str(&text);
        }
        self.out.push(line.trim_end().to_string());
        self.text.clear();
        self.open = false;
    }

    fn ensure_open(&mut self) {
        if !self.open {
            self.prefix = self.plain_prefix();
            self.open = true;
        }
    }
}

/// The markup the editor shows for a stored rich-text value.
pub fn rich_text_markup(html: &str) -> String {
    let clean = sanitize_rich_text(html);
    let mut lines = Lines {
        out: vec![],
        prefix: String::new(),
        text: String::new(),
        open: false,
        quotes: 0,
        lists: vec![],
        links: vec![],
    };
    for token in tokens(&clean) {
        match token {
            Token::Text(text) => {
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
                if !lines.open && collapsed.trim().is_empty() {
                    continue;
                }
                lines.ensure_open();
                let collapsed = if lines.text.is_empty() || lines.text.ends_with(' ') {
                    collapsed.trim_start().to_string()
                } else {
                    collapsed
                };
                lines.text.push_str(&escape_inline(&collapsed));
            }
            Token::Open { name, href, .. } => match name.as_str() {
                "p" | "div" | "h1" | "h2" | "h3" | "blockquote" | "li" => {
                    if lines.open && !lines.text.is_empty() {
                        lines.flush();
                    }
                    if name == "blockquote" {
                        lines.quotes += 1;
                    }
                    // A paragraph inside an item or quote that has no text yet keeps its prefix.
                    if lines.open && matches!(name.as_str(), "p" | "div") {
                        continue;
                    }
                    lines.prefix = match name.as_str() {
                        "h1" => "# ".into(),
                        "h2" => "## ".into(),
                        "h3" => "### ".into(),
                        "li" => match lines.lists.last_mut() {
                            Some(Some(n)) => {
                                *n += 1;
                                format!("{n}. ")
                            }
                            _ => "- ".into(),
                        },
                        _ => lines.plain_prefix(),
                    };
                    lines.open = true;
                }
                "ul" | "ol" => {
                    if lines.open && !lines.text.is_empty() {
                        lines.flush();
                    }
                    lines.open = false;
                    lines.lists.push((name == "ol").then_some(0));
                }
                "br" => {
                    if lines.open {
                        lines.flush();
                    }
                    lines.ensure_open();
                }
                "a" => {
                    lines.ensure_open();
                    if href.is_some() {
                        lines.text.push('[');
                    }
                    lines.links.push(href);
                }
                tag => {
                    if let Some(marker) = inline_marker(tag) {
                        lines.ensure_open();
                        lines.text.push_str(marker);
                    }
                }
            },
            Token::Close(name) => match name.as_str() {
                "p" | "div" | "h1" | "h2" | "h3" | "blockquote" | "li" => {
                    if lines.open {
                        lines.flush();
                    }
                    if name == "blockquote" {
                        lines.quotes = lines.quotes.saturating_sub(1);
                    }
                }
                "ul" | "ol" => {
                    lines.lists.pop();
                }
                "a" => {
                    if let Some(Some(href)) = lines.links.pop() {
                        lines.text.push_str(&format!("]({href})"));
                    }
                }
                tag => {
                    if let Some(marker) = inline_marker(tag) {
                        lines.text.push_str(marker);
                    }
                }
            },
        }
    }
    if lines.open {
        lines.flush();
    }
    lines.out.join("\n")
}

// ---- markup to HTML -----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Block {
    Paragraph,
    Bullet,
    Numbered,
    Heading(usize),
    Quote,
}

enum Piece {
    Text(String),
    Marker(&'static str, &'static str),
    Link(String, String),
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// `[label](url)` at the start of `rest`: the label, the url and the bytes it spans.
fn link_at(rest: &str) -> Option<(&str, &str, usize)> {
    let body = rest.strip_prefix('[')?;
    let mut escaped = false;
    let mut close = None;
    for (i, c) in body.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '[' => return None,
            ']' => {
                close = Some(i);
                break;
            }
            _ => {}
        }
    }
    let close = close?;
    let url_part = body[close + 1..].strip_prefix('(')?;
    let end = url_part.find(')')?;
    let url = &url_part[..end];
    if url.is_empty() || url.contains(char::is_whitespace) {
        return None;
    }
    Some((&body[..close], url, 1 + close + 2 + end + 1))
}

fn pieces(line: &str) -> Vec<Piece> {
    let mut out = vec![];
    let mut text = String::new();
    let mut rest = line;
    'scan: while let Some(c) = rest.chars().next() {
        if c == '\\' {
            if let Some(next) = rest[1..].chars().next().filter(|n| n.is_ascii_punctuation()) {
                text.push(next);
                rest = &rest[1 + next.len_utf8()..];
                continue;
            }
        }
        if c == '[' {
            if let Some((label, url, len)) = link_at(rest) {
                out.push(Piece::Text(std::mem::take(&mut text)));
                out.push(Piece::Link(inline_html(label), url.to_string()));
                rest = &rest[len..];
                continue;
            }
        }
        for (marker, tag) in INLINE {
            if rest.starts_with(marker) {
                out.push(Piece::Text(std::mem::take(&mut text)));
                out.push(Piece::Marker(marker, tag));
                rest = &rest[marker.len()..];
                continue 'scan;
            }
        }
        text.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out.push(Piece::Text(text));
    out
}

/// One line's inline markup as HTML. A marker pairs with the nearest open one of its kind; one
/// without a partner is literal text, and tags that would overlap are closed and reopened so the HTML nests.
fn inline_html(line: &str) -> String {
    let pieces = pieces(line);
    let mut paired = vec![false; pieces.len()];
    let mut open: Vec<(&str, usize)> = vec![];
    for (i, piece) in pieces.iter().enumerate() {
        if let Piece::Marker(marker, _) = piece {
            match open.iter().rposition(|(m, _)| m == marker) {
                Some(at) => {
                    paired[open[at].1] = true;
                    paired[i] = true;
                    open.remove(at);
                }
                None => open.push((marker, i)),
            }
        }
    }
    let mut html = String::new();
    let mut stack: Vec<&str> = vec![];
    for (i, piece) in pieces.iter().enumerate() {
        match piece {
            Piece::Text(text) => html.push_str(&escape_html(text)),
            Piece::Link(label, url) => {
                html.push_str(&format!("<a href=\"{}\">{label}</a>", escape_html(url).replace('"', "&quot;")))
            }
            Piece::Marker(marker, _) if !paired[i] => html.push_str(marker),
            Piece::Marker(_, tag) => match stack.iter().rposition(|t| t == tag) {
                Some(at) => {
                    let reopen: Vec<&str> = stack.drain(at + 1..).collect();
                    for t in reopen.iter().rev() {
                        html.push_str(&format!("</{t}>"));
                    }
                    stack.pop();
                    html.push_str(&format!("</{tag}>"));
                    for t in &reopen {
                        html.push_str(&format!("<{t}>"));
                    }
                    stack.extend(reopen);
                }
                None => {
                    html.push_str(&format!("<{tag}>"));
                    stack.push(tag);
                }
            },
        }
    }
    // Closing and reopening can leave an empty pair behind.
    for (_, tag) in INLINE {
        html = html.replace(&format!("<{tag}></{tag}>"), "");
    }
    html
}

/// The stored HTML for editor markup; None when there is no text.
pub fn markup_rich_text(markup: &str) -> Option<String> {
    if markup.trim().is_empty() {
        return None;
    }
    let mut html = String::new();
    let mut group: Option<Block> = None;
    let close = |html: &mut String, group: Option<Block>| match group {
        Some(Block::Bullet) => html.push_str("</ul>"),
        Some(Block::Numbered) => html.push_str("</ol>"),
        Some(Block::Quote) => html.push_str("</blockquote>"),
        _ => {}
    };
    for line in markup.lines() {
        let (block, at) = block_prefix(line).unwrap_or((Block::Paragraph, 0));
        let inner = inline_html(&line[at..]);
        let grouped = matches!(block, Block::Bullet | Block::Numbered | Block::Quote);
        if group != Some(block) {
            close(&mut html, group);
            group = None;
            if grouped {
                html.push_str(match block {
                    Block::Bullet => "<ul>",
                    Block::Numbered => "<ol>",
                    _ => "<blockquote>",
                });
                group = Some(block);
            }
        }
        match block {
            Block::Bullet | Block::Numbered => html.push_str(&format!("<li>{inner}</li>")),
            Block::Heading(n) => html.push_str(&format!("<h{n}>{inner}</h{n}>")),
            Block::Quote | Block::Paragraph => html.push_str(&format!("<p>{inner}</p>")),
        }
    }
    close(&mut html, group);
    Some(sanitize_rich_text(&html))
}

// ---- toolbar edits ------------------------------------------------------------------------

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |i| i + 1)
}

fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |i| at + i)
}

fn stars_mark(before: usize, after: usize, marker: &str) -> bool {
    before == after && if marker == "*" { before % 2 == 1 } else { before >= 2 }
}

/// Whether `marker` sits right outside `range`. Stars count together: `*a*` and `***a***` are
/// italic, `**a**` is not.
fn wrapped(text: &str, range: &Range<usize>, marker: &str) -> bool {
    if marker.starts_with('*') {
        let before = text[..range.start].bytes().rev().take_while(|b| *b == b'*').count();
        let after = text[range.end..].bytes().take_while(|b| *b == b'*').count();
        return stars_mark(before, after, marker);
    }
    text[..range.start].ends_with(marker) && text[range.end..].starts_with(marker)
}

/// Whether the selected text itself starts and ends with `marker`.
fn inside(inner: &str, marker: &str) -> bool {
    if marker.starts_with('*') {
        let lead = inner.bytes().take_while(|b| *b == b'*').count();
        let trail = inner.bytes().rev().take_while(|b| *b == b'*').count();
        return lead + trail < inner.len() && stars_mark(lead, trail, marker);
    }
    inner.len() >= 2 * marker.len() && inner.starts_with(marker) && inner.ends_with(marker)
}

/// Bold, italic, underline or strike (`marker`) on the selection: wraps it, or unwraps it when
/// it is already wrapped. With nothing selected it inserts a pair and puts the caret inside.
pub fn toggle_inline(text: &str, selection: Range<usize>, marker: &str) -> Edit {
    let m = marker.len();
    let selected = &text[selection.clone()];
    if selected.contains('\n') {
        // Markers do not span lines: wrap each line of the selection on its own.
        let (start, end) = (line_start(text, selection.start), line_end(text, selection.end));
        let mut out = String::new();
        for (i, line) in text[start..end].split('\n').enumerate() {
            if i > 0 {
                out.push('\n');
            }
            let at = block_prefix(line).map_or(0, |(_, at)| at);
            let body = &line[at..];
            out.push_str(&line[..at]);
            if body.trim().is_empty() {
                out.push_str(body);
            } else {
                out.push_str(&format!("{marker}{body}{marker}"));
            }
        }
        let text = format!("{}{out}{}", &text[..start], &text[end..]);
        return Edit { selection: start..start + out.len(), text };
    }
    // Leave surrounding spaces (a double-clicked word brings one) outside the markers.
    let lead = selected.len() - selected.trim_start().len();
    let trail = selected.len() - selected.trim_end().len();
    let range = if selected.trim().is_empty() {
        selection.clone()
    } else {
        selection.start + lead..selection.end - trail
    };
    if wrapped(text, &range, marker) {
        let text = format!("{}{}{}", &text[..range.start - m], &text[range.clone()], &text[range.end + m..]);
        return Edit { text, selection: range.start - m..range.end - m };
    }
    let inner = &text[range.clone()];
    if inside(inner, marker) {
        let text = format!("{}{}{}", &text[..range.start], &inner[m..inner.len() - m], &text[range.end..]);
        return Edit { text, selection: range.start..range.end - 2 * m };
    }
    let text = format!("{}{marker}{inner}{marker}{}", &text[..range.start], &text[range.end..]);
    Edit { text, selection: range.start + m..range.end + m }
}

/// Bulleted (or numbered) list on the selected lines; removes it when every line already is one.
pub fn toggle_list(text: &str, selection: Range<usize>, numbered: bool) -> Edit {
    let start = line_start(text, selection.start);
    // A selection that ends at the start of a line does not take that line.
    let last = if selection.end > selection.start && selection.end == line_start(text, selection.end) {
        selection.end - 1
    } else {
        selection.end
    };
    let end = line_end(text, last.max(start));
    let want = if numbered { Block::Numbered } else { Block::Bullet };
    let lines: Vec<&str> = text[start..end].split('\n').collect();
    let all = lines.iter().all(|l| block_prefix(l).is_some_and(|(b, _)| b == want));
    let mut out = String::new();
    let mut caret = None;
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let at = block_prefix(line).map_or(0, |(_, at)| at);
        let prefix = match (all, numbered) {
            (true, _) => String::new(),
            (false, true) => format!("{}. ", i + 1),
            (false, false) => "- ".into(),
        };
        let line_at = start + lines[..i].iter().map(|l| l.len() + 1).sum::<usize>();
        if selection.is_empty() && (line_at..=line_at + line.len()).contains(&selection.start) {
            let offset = selection.start.saturating_sub(line_at + at);
            caret = Some(start + out.len() + prefix.len() + offset);
        }
        out.push_str(&prefix);
        out.push_str(&line[at..]);
    }
    let text = format!("{}{out}{}", &text[..start], &text[end..]);
    let selection = match caret {
        Some(c) => c..c,
        None => start..start + out.len(),
    };
    Edit { text, selection }
}

#[cfg(test)]
#[path = "richtext_markup_tests.rs"]
mod tests;
