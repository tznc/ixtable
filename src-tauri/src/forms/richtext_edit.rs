//! The rich-text toolbar's edits on the editor's Markdown (`richtext_markup.rs`). Each takes the
//! text and the selection and returns the new text and the range to select in it.

use std::ops::Range;

/// A toolbar edit's result: the new Markdown and the range to select in it.
#[derive(Debug, PartialEq)]
pub struct Edit {
    pub text: String,
    pub selection: Range<usize>,
}

/// What a line starts with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineKind {
    Bullet,
    Numbered,
    Task,
    Heading(usize),
    Quote,
}

impl LineKind {
    fn prefix(self, index: usize) -> String {
        match self {
            LineKind::Bullet => "- ".into(),
            LineKind::Numbered => format!("{}. ", index + 1),
            LineKind::Task => "- [ ] ".into(),
            LineKind::Heading(n) => format!("{} ", "#".repeat(n)),
            LineKind::Quote => "> ".into(),
        }
    }
}

/// The block prefix a line starts with and its length.
fn line_prefix(line: &str) -> Option<(LineKind, usize)> {
    for task in ["- [ ] ", "- [x] ", "- [X] ", "* [ ] ", "* [x] "] {
        if line.starts_with(task) {
            return Some((LineKind::Task, task.len()));
        }
    }
    if line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ") {
        return Some((LineKind::Bullet, 2));
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && (line[digits..].starts_with(". ") || line[digits..].starts_with(") ")) {
        return Some((LineKind::Numbered, digits + 2));
    }
    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
        return Some((LineKind::Heading(hashes), hashes + 1));
    }
    if line.starts_with("> ") {
        return Some((LineKind::Quote, 2));
    }
    (line == ">").then_some((LineKind::Quote, 1))
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |i| i + 1)
}

fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |i| at + i)
}

fn stars_mark(before: usize, after: usize, marker: &str) -> bool {
    before == after && if marker == "*" { before % 2 == 1 } else { before >= 2 }
}

/// Whether `open` and `close` sit right outside `range`. Stars count together: `*a*` and
/// `***a***` are italic, `**a**` is not.
fn wrapped(text: &str, range: &Range<usize>, open: &str, close: &str) -> bool {
    if open.starts_with('*') {
        let before = text[..range.start].bytes().rev().take_while(|b| *b == b'*').count();
        let after = text[range.end..].bytes().take_while(|b| *b == b'*').count();
        return stars_mark(before, after, open);
    }
    text[..range.start].ends_with(open) && text[range.end..].starts_with(close)
}

/// Whether the selected text itself starts with `open` and ends with `close`.
fn inside(inner: &str, open: &str, close: &str) -> bool {
    if open.starts_with('*') {
        let lead = inner.bytes().take_while(|b| *b == b'*').count();
        let trail = inner.bytes().rev().take_while(|b| *b == b'*').count();
        return lead + trail < inner.len() && stars_mark(lead, trail, open);
    }
    inner.len() >= open.len() + close.len() && inner.starts_with(open) && inner.ends_with(close)
}

/// Bold, italic, underline, strike or code (`open`/`close`) on the selection: wraps it, or
/// unwraps it when it is already wrapped. With nothing selected it inserts a pair and puts the
/// caret inside.
pub fn toggle_inline(text: &str, selection: Range<usize>, open: &str, close: &str) -> Edit {
    let (o, c) = (open.len(), close.len());
    let selected = &text[selection.clone()];
    if selected.contains('\n') {
        // Markers do not span lines: wrap each line of the selection on its own.
        let (start, end) = (line_start(text, selection.start), line_end(text, selection.end));
        let mut out = String::new();
        for (i, line) in text[start..end].split('\n').enumerate() {
            if i > 0 {
                out.push('\n');
            }
            let at = line_prefix(line).map_or(0, |(_, at)| at);
            let body = &line[at..];
            out.push_str(&line[..at]);
            if body.trim().is_empty() {
                out.push_str(body);
            } else {
                out.push_str(&format!("{open}{body}{close}"));
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
    if wrapped(text, &range, open, close) {
        let text = format!("{}{}{}", &text[..range.start - o], &text[range.clone()], &text[range.end + c..]);
        return Edit { text, selection: range.start - o..range.end - o };
    }
    let inner = &text[range.clone()];
    if inside(inner, open, close) {
        let text = format!("{}{}{}", &text[..range.start], &inner[o..inner.len() - c], &text[range.end..]);
        return Edit { text, selection: range.start..range.end - o - c };
    }
    let text = format!("{}{open}{inner}{close}{}", &text[..range.start], &text[range.end..]);
    Edit { text, selection: range.start + o..range.end + o }
}

/// A list, task list, heading or quote on the selected lines; removes it when every line
/// already is one.
pub fn toggle_lines(text: &str, selection: Range<usize>, kind: LineKind) -> Edit {
    let start = line_start(text, selection.start);
    // A selection that ends at the start of a line does not take that line.
    let last = if selection.end > selection.start && selection.end == line_start(text, selection.end) {
        selection.end - 1
    } else {
        selection.end
    };
    let end = line_end(text, last.max(start));
    let lines: Vec<&str> = text[start..end].split('\n').collect();
    let all = lines.iter().all(|l| line_prefix(l).is_some_and(|(k, _)| k == kind));
    let mut out = String::new();
    let mut caret = None;
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let at = line_prefix(line).map_or(0, |(_, at)| at);
        let prefix = if all { String::new() } else { kind.prefix(i) };
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

/// Replaces the selection with `block` on lines of its own (a blank line before and after) and
/// selects `select`, a range within `block`.
fn insert_block(text: &str, selection: Range<usize>, block: &str, select: Range<usize>) -> Edit {
    let before = text[..selection.start].trim_end_matches([' ', '\t']);
    let after = text[selection.end..].trim_start_matches([' ', '\t']);
    let lead = match before {
        "" => "",
        b if b.ends_with("\n\n") => "",
        b if b.ends_with('\n') => "\n",
        _ => "\n\n",
    };
    let tail = match after {
        "" => "",
        a if a.starts_with("\n\n") => "",
        a if a.starts_with('\n') => "\n",
        _ => "\n\n",
    };
    let at = before.len() + lead.len();
    let text = format!("{before}{lead}{block}{tail}{after}");
    Edit { text, selection: at + select.start..at + select.end }
}

/// A fenced code block around the selected text, or an empty one with the caret inside.
pub fn code_block(text: &str, selection: Range<usize>) -> Edit {
    let code = text[selection.clone()].trim_matches('\n');
    let block = format!("```\n{code}\n```");
    insert_block(text, selection, &block, 4..4 + code.len())
}

/// A two-column table with the first heading selected.
pub fn table(text: &str, selection: Range<usize>) -> Edit {
    let block = "| Column 1 | Column 2 |\n| --- | --- |\n|  |  |";
    insert_block(text, selection, block, 2..10)
}

/// A horizontal rule.
pub fn divider(text: &str, selection: Range<usize>) -> Edit {
    insert_block(text, selection, "---", 3..3)
}

/// The selection as a link's text, with the address selected to type over.
pub fn link(text: &str, selection: Range<usize>) -> Edit {
    let label = match &text[selection.clone()] {
        "" => "text",
        selected => selected.trim(),
    };
    let inserted = format!("[{label}](https://)");
    let at = selection.start + label.len() + 3;
    let selection_out = if text[selection.clone()].is_empty() {
        selection.start + 1..selection.start + 1 + label.len()
    } else {
        at..at + "https://".len()
    };
    let text = format!("{}{inserted}{}", &text[..selection.start], &text[selection.end..]);
    Edit { text, selection: selection_out }
}

/// An image of `src` in place of the selection (its text becomes the description), with the
/// caret after it.
pub fn image(text: &str, selection: Range<usize>, alt: &str, src: &str) -> Edit {
    let selected = text[selection.clone()].trim();
    let alt = if selected.is_empty() { alt } else { selected };
    let alt = alt.replace(['[', ']'], "");
    let src = if src.contains([' ', '(', ')']) { format!("<{src}>") } else { src.to_string() };
    let inserted = format!("![{alt}]({src})");
    let end = selection.start + inserted.len();
    let text = format!("{}{inserted}{}", &text[..selection.start], &text[selection.end..]);
    Edit { text, selection: end..end }
}

#[cfg(test)]
#[path = "richtext_edit_tests.rs"]
mod tests;
