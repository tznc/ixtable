//! Rich-text fields store a sanitized HTML subset (port of `src/fields/richtext.ts`), widened to
//! everything Markdown writes: headings, quotes, lists and task lists, code, rules, tables,
//! links and images (`docs/decisions/field-formats.md`). Everything is sanitized on the way in
//! (editor output) and again before display, because a value can also arrive from an import, an
//! action, or another PostgreSQL client. The scanner below reads tags and text without a DOM.

const ALLOWED: &[&str] = &[
    "p", "br", "div", "b", "strong", "i", "em", "u", "s", "del", "code", "ul", "ol", "li", "a",
    "h1", "h2", "h3", "h4", "h5", "h6", "blockquote", "pre", "hr", "img", "input", "table",
    "thead", "tbody", "tr", "th", "td",
];
/// Elements whose content is dropped with them, not kept as text.
const DROPPED: &[&str] = &[
    "script", "style", "template", "iframe", "object", "embed", "noscript",
];
const BLOCKS: &[&str] = &[
    "p", "div", "li", "h1", "h2", "h3", "h4", "h5", "h6", "blockquote", "br", "pre", "hr", "tr",
];
const VOID: &[&str] = &["br", "img", "hr", "input", "meta", "link", "wbr"];
/// Image sources a value may use: web images and files of the record's attachment fields.
pub const ATTACHMENT_SCHEME: &str = "attachment:";

#[derive(Debug)]
pub(super) enum Token {
    Text(String),
    Open {
        name: String,
        attrs: Vec<(String, String)>,
        self_closing: bool,
    },
    Close(String),
}

impl Token {
    /// An attribute of an opening tag (names are lowercase).
    pub(super) fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
        attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
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

/// The attributes in a tag's source text (between the name and `>`), names lowercased.
fn attributes_of(source: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = source;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
        let name_len = rest
            .find(|c: char| c.is_whitespace() || matches!(c, '=' | '/' | '>'))
            .unwrap_or(rest.len());
        if name_len == 0 {
            break;
        }
        let name = rest[..name_len].to_ascii_lowercase();
        rest = rest[name_len..].trim_start();
        let mut value = String::new();
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            match after.chars().next() {
                Some(q @ ('"' | '\'')) => {
                    let body = &after[1..];
                    let end = body.find(q).unwrap_or(body.len());
                    value = body[..end].to_string();
                    rest = body.get(end + 1..).unwrap_or("");
                }
                _ => {
                    let end = after.find(|c: char| c.is_whitespace() || c == '>').unwrap_or(after.len());
                    value = after[..end].to_string();
                    rest = &after[end..];
                }
            }
        }
        if !out.iter().any(|(n, _): &(String, String)| *n == name) {
            out.push((name, decode_entities(&value)));
        }
    }
    out
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
                attrs: attributes_of(attributes),
                self_closing: attributes.trim_end().ends_with('/') || VOID.contains(&name.as_str()),
                name,
            });
        }
    }
    out
}

fn safe_link(href: &str) -> bool {
    let lower = href.to_ascii_lowercase();
    lower.starts_with("http:") || lower.starts_with("https:") || lower.starts_with("mailto:")
}

fn safe_image(src: &str) -> bool {
    let lower = src.to_ascii_lowercase();
    lower.starts_with("http:") || lower.starts_with("https:") || lower.starts_with(ATTACHMENT_SCHEME)
}

/// The allowed attributes of an allowed element, as source text; None drops the element.
fn kept_attributes(name: &str, attrs: &[(String, String)]) -> Option<String> {
    let attr = |n: &str| Token::attr(attrs, n).map(str::trim).filter(|v| !v.is_empty());
    let mut out = String::new();
    let mut push = |n: &str, v: &str| out.push_str(&format!(" {n}=\"{}\"", escape_attribute(v)));
    match name {
        "a" => {
            if let Some(href) = attr("href").filter(|h| safe_link(h)) {
                push("href", href);
                if let Some(title) = attr("title") {
                    push("title", title);
                }
                push("rel", "noopener noreferrer");
                push("target", "_blank");
            }
        }
        "img" => {
            push("src", attr("src").filter(|s| safe_image(s))?);
            for n in ["alt", "title"] {
                if let Some(v) = attr(n) {
                    push(n, v);
                }
            }
        }
        "input" => {
            if !attr("type").is_some_and(|t| t.eq_ignore_ascii_case("checkbox")) {
                return None;
            }
            out.push_str(" type=\"checkbox\" disabled");
            if Token::attr(attrs, "checked").is_some() {
                out.push_str(" checked");
            }
        }
        "th" | "td" => {
            if let Some(align) = attr("align").map(str::to_ascii_lowercase) {
                if matches!(align.as_str(), "left" | "center" | "right") {
                    push("align", &align);
                }
            }
        }
        "ol" => {
            if let Some(start) = attr("start").filter(|s| s.len() < 10 && s.bytes().all(|b| b.is_ascii_digit())) {
                push("start", start);
            }
        }
        "code" => {
            let language = attr("class").and_then(|c| c.split_whitespace().find_map(|c| c.strip_prefix("language-")));
            if let Some(language) = language.filter(|l| {
                l.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '#' | '-' | '_' | '.'))
            }) {
                push("class", &format!("language-{language}"));
            }
        }
        _ => {}
    }
    Some(out)
}

/// The allowed subset of `html`: formatting tags only, links limited to http(s) and mailto,
/// images to http(s) and the record's attachments.
pub fn sanitize_rich_text(html: &str) -> String {
    sanitize_with(html, None)
}

/// Maps an allowed image source to the one to show; None shows the image's description instead.
type ImageHook<'a> = &'a dyn Fn(&str) -> Option<String>;

fn sanitize_with(html: &str, images: Option<ImageHook>) -> String {
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
                attrs,
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
                let Some(mut kept) = kept_attributes(&name, &attrs) else {
                    continue;
                };
                if let (Some(hook), "img") = (images, name.as_str()) {
                    let src = Token::attr(&attrs, "src").map(str::trim).unwrap_or_default();
                    match hook(src) {
                        Some(shown) => {
                            let old = format!(" src=\"{}\"", escape_attribute(src));
                            kept = kept.replacen(&old, &format!(" src=\"{}\"", escape_attribute(&shown)), 1);
                        }
                        None => {
                            out.push_str(&escape_text(Token::attr(&attrs, "alt").unwrap_or_default()));
                            continue;
                        }
                    }
                }
                out.push('<');
                out.push_str(&name);
                out.push_str(&kept);
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

/// Sanitized HTML ready for the text view, which draws no form inputs: task-list boxes become
/// ballot-box characters, and `image` gives each image's source to show (a data URL once the
/// image is loaded) or None to show its description.
pub fn rich_text_display(html: &str, image: &dyn Fn(&str) -> Option<String>) -> String {
    sanitize_with(html, Some(image))
        .replace("<input type=\"checkbox\" disabled checked>", "\u{2611} ")
        .replace("<input type=\"checkbox\" disabled>", "\u{2610} ")
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
                name,
                attrs,
                self_closing,
            } => {
                if DROPPED.contains(&name.as_str()) {
                    if !self_closing {
                        dropped.push(name);
                    }
                } else if BLOCKS.contains(&name.as_str()) && !parts.is_empty() {
                    parts.push('\n');
                } else if matches!(name.as_str(), "td" | "th") && !parts.is_empty() && !parts.ends_with('\n') {
                    parts.push(' ');
                } else if name == "img" {
                    parts.push_str(Token::attr(&attrs, "alt").unwrap_or_default());
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
