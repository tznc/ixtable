use super::{markup_rich_text, rich_text_markup};

fn html(markup: &str) -> String {
    markup_rich_text(markup).unwrap_or_default()
}

const LINK: &str = "rel=\"noopener noreferrer\" target=\"_blank\"";

#[test]
fn turns_markdown_into_the_html_subset() {
    assert_eq!(html("Hello"), "<p>Hello</p>");
    assert_eq!(html("**b** *i* <u>u</u> ~~s~~ `c`"), "<p><b>b</b> <i>i</i> <u>u</u> <s>s</s> <code>c</code></p>");
    assert_eq!(html("- one\n- two\n\nafter"), "<ul><li>one</li><li>two</li></ul><p>after</p>");
    assert_eq!(html("1. one\n2. two"), "<ol><li>one</li><li>two</li></ol>");
    assert_eq!(html("3. three"), "<ol start=\"3\"><li>three</li></ol>");
    assert_eq!(html("# Title\n###### Small"), "<h1>Title</h1><h6>Small</h6>");
    assert_eq!(html("> quoted\n> more"), "<blockquote><p>quoted<br>more</p></blockquote>");
    assert_eq!(html("a\n\nb"), "<p>a</p><p>b</p>");
    assert_eq!(html("---"), "<hr>");
    assert_eq!(markup_rich_text("  \n "), None);
}

#[test]
fn keeps_single_line_breaks() {
    assert_eq!(html("one\ntwo"), "<p>one<br>two</p>");
    assert_eq!(html("one\\\ntwo"), "<p>one<br>two</p>");
}

#[test]
fn nests_lists_and_task_lists() {
    assert_eq!(html("- a\n  - b\n- c"), "<ul><li>a<ul><li>b</li></ul></li><li>c</li></ul>");
    assert_eq!(
        html("- [ ] todo\n- [x] done"),
        "<ul><li><input type=\"checkbox\" disabled>todo</li><li><input type=\"checkbox\" disabled checked>done</li></ul>"
    );
    assert_eq!(html("- a\n\n- b"), "<ul><li><p>a</p></li><li><p>b</p></li></ul>");
}

#[test]
fn writes_code_blocks_and_tables() {
    assert_eq!(html("```rust\nlet a = 1 < 2;\n```"), "<pre><code class=\"language-rust\">let a = 1 &lt; 2;</code></pre>");
    assert_eq!(
        html("| A | B |\n| :-- | --: |\n| 1 | **2** |"),
        "<table><thead><tr><th align=\"left\">A</th><th align=\"right\">B</th></tr></thead>\
         <tbody><tr><td align=\"left\">1</td><td align=\"right\"><b>2</b></td></tr></tbody></table>"
    );
}

#[test]
fn keeps_unpaired_markers_and_escapes_as_text() {
    assert_eq!(html("2 * 3 = 6"), "<p>2 * 3 = 6</p>");
    assert_eq!(html("C++ and **open"), "<p>C++ and **open</p>");
    assert_eq!(html(r"\*not italic\*"), "<p>*not italic*</p>");
    assert_eq!(html("a < b & c"), "<p>a &lt; b &amp; c</p>");
    assert_eq!(html("~one~"), "<p>~one~</p>");
}

#[test]
fn makes_links_and_images_with_safe_targets_only() {
    assert_eq!(
        html("see [the **site**](https://example.com)"),
        format!("<p>see <a href=\"https://example.com\" {LINK}>the <b>site</b></a></p>")
    );
    assert_eq!(html("[x](javascript:alert(1))"), "<p><a>x</a></p>");
    assert_eq!(html("www.example.com"), format!("<p><a href=\"http://www.example.com\" {LINK}>www.example.com</a></p>"));
    assert_eq!(html("[ref]\n\n[ref]: https://x.io"), format!("<p><a href=\"https://x.io\" {LINK}>ref</a></p>"));
    assert_eq!(
        html("![logo](https://x.io/a.png \"Our logo\")"),
        "<p><img src=\"https://x.io/a.png\" alt=\"logo\" title=\"Our logo\"></p>"
    );
    assert_eq!(html("![chart](attachment:chart.png)"), "<p><img src=\"attachment:chart.png\" alt=\"chart\"></p>");
    assert_eq!(html("![x](file:///etc/passwd) ![y](data:image/png;base64,AA)"), "<p> </p>");
    assert_eq!(html("<img src=x onerror=alert(1)><script>alert(1)</script>"), "<p></p>");
}

#[test]
fn shows_stored_html_as_markdown() {
    assert_eq!(rich_text_markup("<p>Hello <b>you</b></p><p><i>there</i></p>"), "Hello **you**\n\n*there*");
    assert_eq!(rich_text_markup("<ul><li>a</li><li><p>b</p></li></ul><ol><li>c</li><li>d</li></ol>"), "- a\n- b\n\n1. c\n2. d");
    assert_eq!(rich_text_markup("<h2>T</h2><blockquote><p>q</p></blockquote>"), "## T\n\n> q");
    assert_eq!(rich_text_markup("<b>Bold</b><script>alert(1)</script>"), "**Bold**");
    assert_eq!(rich_text_markup("<p>a<br>b</p>"), "a\\\nb");
    assert_eq!(rich_text_markup("<p>\n  spaced   out \n</p>"), "spaced out");
    assert_eq!(rich_text_markup("<p><a href=\"https://x.io\">x</a> <a href=\"javascript:1\">y</a></p>"), "[x](https://x.io) y");
    assert_eq!(rich_text_markup("<p><b>bold </b>next</p>"), "**bold** next");
    assert_eq!(rich_text_markup("<p><u>u</u></p>"), "<u>u</u>");
    assert_eq!(rich_text_markup("<pre><code>a ``` b\n</code></pre>"), "````\na ``` b\n````");
    assert_eq!(rich_text_markup("<p>use <code>a`b</code></p>"), "use ``a`b``");
}

#[test]
fn shows_tables_lists_and_images_as_markdown() {
    assert_eq!(
        rich_text_markup("<table><tr><th>A</th><th align=\"center\">B|C</th></tr><tr><td>1</td></tr></table>"),
        "| A | B\\|C |\n| --- | :---: |\n| 1 |  |"
    );
    assert_eq!(
        rich_text_markup("<ul><li><input type=\"checkbox\" checked> done<ul><li>sub</li></ul></li></ul>"),
        "- [x] done\n  - sub"
    );
    assert_eq!(rich_text_markup("<p><img src=\"attachment:my photo.png\" alt=\"me\"></p>"), "![me](<attachment:my photo.png>)");
}

#[test]
fn escapes_text_that_would_read_as_markdown() {
    assert_eq!(rich_text_markup("<p>2*3 [x] a_b &amp;copy;</p>"), r"2\*3 \[x\] a\_b \&copy;");
    assert_eq!(
        rich_text_markup("<p>- not a list</p><p>1. nor this</p><p># nor this</p>"),
        "\\- not a list\n\n1\\. nor this\n\n\\# nor this"
    );
}

#[test]
fn round_trips_stored_html() {
    for stored in [
        "<p>2*3 [x] C++ a+b</p>",
        "<p>- not a list</p><p>1. nor this</p><p># nor this</p><p>| pipe</p>",
        "<p>a <b>b <i>c</i></b> <u>d</u> <s>e</s> <code>f</code></p><ul><li>x</li></ul><ol><li>y</li></ol><h1>h</h1>",
        "<p>line one<br>line two</p>",
        "<h4>four</h4><h5>five</h5><h6>six</h6><hr>",
        "<blockquote><p>a</p><ul><li>b</li></ul></blockquote>",
        "<ul><li>a<ul><li>b<ol><li>c</li></ol></li></ul></li><li>d</li></ul>",
        "<ul><li><input type=\"checkbox\" disabled>open</li><li><input type=\"checkbox\" disabled checked>done</li></ul>",
        "<pre><code class=\"language-sql\">select *\nfrom t;</code></pre>",
        "<table><thead><tr><th>A</th><th align=\"right\">B</th></tr></thead><tbody><tr><td><b>1</b></td><td align=\"right\">a<br>b</td></tr></tbody></table>",
        "<p><img src=\"https://x.io/a.png\" alt=\"pic\" title=\"T\"> and <img src=\"attachment:p%20q.png\" alt=\"q\"></p>",
        "<p><a href=\"https://x.io/a_(b)\" title=\"t\" rel=\"noopener noreferrer\" target=\"_blank\">x</a></p>",
    ] {
        let markup = rich_text_markup(stored);
        assert_eq!(markup_rich_text(&markup).as_deref(), Some(super::sanitize_rich_text(stored).as_str()), "round trip of {stored} via {markup:?}");
    }
}

#[test]
fn round_trips_markdown() {
    let markup = "# Notes\n\nSome **bold**, *italic*, <u>under</u> and ~~gone~~ text with `code`.\\\nNext line.\n\n\
                  > A quote\n\n- [ ] task\n- [x] done\n  - nested\n\n1. one\n2. two\n\n\
                  ```js\nlet x = 1;\n```\n\n| Item | Qty |\n| --- | ---: |\n| Pen | 2 |\n\n---\n\n\
                  ![chart](attachment:chart.png) and [site](https://example.com)";
    let stored = html(markup);
    assert_eq!(rich_text_markup(&stored), markup);
}
