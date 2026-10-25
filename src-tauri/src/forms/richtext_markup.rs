//! Rich text edited as Markdown. GPUI has no content-editable box, so the form's rich-text
//! editor shows the stored HTML as Markdown, turns it back into the HTML subset on every change,
//! and renders the result beside it. The toolbar edits the Markdown (`richtext_edit.rs`).
//!
//! The Markdown is CommonMark with the GitHub extensions (tables, task lists, `~~strike~~`,
//! bare links), read by the `markdown` crate. Underline has no Markdown syntax and is written
//! as `<u>…</u>`, which Markdown passes through as inline HTML. A single line break inside a
//! paragraph is kept as a line break, as people expect in a text box. Images are web images or
//! `attachment:<file name>` (`docs/decisions/field-formats.md`).

use super::richtext::sanitize_rich_text;
pub use super::richtext_to_markdown::rich_text_markup;
use markdown::mdast::{AlignKind, Node};
use std::collections::HashMap;

// ---- Markdown to HTML ---------------------------------------------------------------------

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn normalize_label(label: &str) -> String {
    label.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

struct Html {
    out: String,
    definitions: HashMap<String, (String, Option<String>)>,
}

fn collect_definitions(node: &Node, into: &mut HashMap<String, (String, Option<String>)>) {
    if let Node::Definition(d) = node {
        into.entry(normalize_label(&d.identifier)).or_insert((d.url.clone(), d.title.clone()));
    }
    for child in node.children().into_iter().flatten() {
        collect_definitions(child, into);
    }
}

impl Html {
    fn children(&mut self, node: &Node, tight: bool) {
        for child in node.children().into_iter().flatten() {
            self.node(child, tight);
        }
    }

    fn wrap(&mut self, tag: &str, node: &Node) {
        self.out.push_str(&format!("<{tag}>"));
        self.children(node, false);
        self.out.push_str(&format!("</{tag}>"));
    }

    fn link(&mut self, url: &str, title: Option<&str>, node: &Node) {
        self.out.push_str(&format!("<a href=\"{}\"", escape_html(url)));
        if let Some(title) = title {
            self.out.push_str(&format!(" title=\"{}\"", escape_html(title)));
        }
        self.out.push('>');
        self.children(node, false);
        self.out.push_str("</a>");
    }

    fn image(&mut self, url: &str, alt: &str, title: Option<&str>) {
        self.out.push_str(&format!("<img src=\"{}\" alt=\"{}\"", escape_html(url), escape_html(alt)));
        if let Some(title) = title {
            self.out.push_str(&format!(" title=\"{}\"", escape_html(title)));
        }
        self.out.push('>');
    }

    /// `tight` is true for the children of a tight list's item, whose paragraphs have no `<p>`.
    fn node(&mut self, node: &Node, tight: bool) {
        match node {
            Node::Paragraph(_) if tight => self.children(node, false),
            Node::Paragraph(_) => self.wrap("p", node),
            Node::Heading(h) => self.wrap(&format!("h{}", h.depth.clamp(1, 6)), node),
            Node::Blockquote(_) => self.wrap("blockquote", node),
            Node::List(list) => {
                let tight = !list.spread
                    && list.children.iter().all(|item| !matches!(item, Node::ListItem(i) if i.spread));
                let tag = if list.ordered { "ol" } else { "ul" };
                match list.start.filter(|s| list.ordered && *s != 1) {
                    Some(start) => self.out.push_str(&format!("<ol start=\"{start}\">")),
                    None => self.out.push_str(&format!("<{tag}>")),
                }
                self.children(node, tight);
                self.out.push_str(&format!("</{tag}>"));
            }
            Node::ListItem(item) => {
                self.out.push_str("<li>");
                match item.checked {
                    Some(true) => self.out.push_str("<input type=\"checkbox\" disabled checked>"),
                    Some(false) => self.out.push_str("<input type=\"checkbox\" disabled>"),
                    None => {}
                }
                self.children(node, tight);
                self.out.push_str("</li>");
            }
            Node::Code(code) => {
                self.out.push_str("<pre><code");
                if let Some(lang) = &code.lang {
                    self.out.push_str(&format!(" class=\"language-{}\"", escape_html(lang)));
                }
                self.out.push_str(&format!(">{}</code></pre>", escape_html(&code.value)));
            }
            Node::ThematicBreak(_) => self.out.push_str("<hr>"),
            Node::Table(table) => {
                self.out.push_str("<table>");
                for (r, row) in table.children.iter().enumerate() {
                    if r == 0 {
                        self.out.push_str("<thead>");
                    } else if r == 1 {
                        self.out.push_str("<tbody>");
                    }
                    let cell = if r == 0 { "th" } else { "td" };
                    self.out.push_str("<tr>");
                    for (c, data) in row.children().into_iter().flatten().enumerate() {
                        let align = match table.align.get(c) {
                            Some(AlignKind::Left) => " align=\"left\"",
                            Some(AlignKind::Center) => " align=\"center\"",
                            Some(AlignKind::Right) => " align=\"right\"",
                            _ => "",
                        };
                        self.out.push_str(&format!("<{cell}{align}>"));
                        self.children(data, false);
                        self.out.push_str(&format!("</{cell}>"));
                    }
                    self.out.push_str("</tr>");
                    if r == 0 {
                        self.out.push_str("</thead>");
                    }
                }
                if table.children.len() > 1 {
                    self.out.push_str("</tbody>");
                }
                self.out.push_str("</table>");
            }
            // Raw HTML (`<u>`, `<br>`) passes through; the sanitizer keeps only the subset.
            Node::Html(html) => self.out.push_str(&html.value),
            Node::Text(text) => self.out.push_str(&escape_html(&text.value).replace('\n', "<br>")),
            Node::Break(_) => self.out.push_str("<br>"),
            Node::InlineCode(code) => self.out.push_str(&format!("<code>{}</code>", escape_html(&code.value))),
            Node::Emphasis(_) => self.wrap("i", node),
            Node::Strong(_) => self.wrap("b", node),
            Node::Delete(_) => self.wrap("s", node),
            Node::Link(link) => self.link(&link.url, link.title.as_deref(), node),
            Node::Image(image) => self.image(&image.url, &image.alt, image.title.as_deref()),
            Node::LinkReference(r) => match self.definitions.get(&normalize_label(&r.identifier)).cloned() {
                Some((url, title)) => self.link(&url, title.as_deref(), node),
                None => self.children(node, false),
            },
            Node::ImageReference(r) => {
                if let Some((url, title)) = self.definitions.get(&normalize_label(&r.identifier)).cloned() {
                    self.image(&url, &r.alt, title.as_deref());
                }
            }
            Node::Definition(_) => {}
            _ => self.children(node, tight),
        }
    }
}

fn parse_options() -> markdown::ParseOptions {
    let mut options = markdown::ParseOptions::gfm();
    // Footnotes need a footer the subset has no place for; `~one~` is too easy to type by accident.
    options.constructs.gfm_footnote_definition = false;
    options.constructs.gfm_label_start_footnote = false;
    options.gfm_strikethrough_single_tilde = false;
    options
}

/// The stored HTML for editor Markdown; None when there is no text.
pub fn markup_rich_text(markup: &str) -> Option<String> {
    if markup.trim().is_empty() {
        return None;
    }
    // Without MDX the parser has no failure mode; an error keeps the text as one paragraph.
    let Ok(tree) = markdown::to_mdast(markup, &parse_options()) else {
        return Some(format!("<p>{}</p>", escape_html(markup)));
    };
    let mut definitions = HashMap::new();
    collect_definitions(&tree, &mut definitions);
    let mut html = Html { out: String::new(), definitions };
    html.node(&tree, false);
    Some(sanitize_rich_text(&html.out))
}

#[cfg(test)]
#[path = "richtext_markup_tests.rs"]
mod tests;
