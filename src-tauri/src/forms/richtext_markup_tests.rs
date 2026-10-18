use super::{Edit, markup_rich_text, rich_text_markup, toggle_inline, toggle_list};

fn html(markup: &str) -> String {
    markup_rich_text(markup).unwrap_or_default()
}

#[test]
fn turns_markup_into_the_html_subset() {
    assert_eq!(html("Hello"), "<p>Hello</p>");
    assert_eq!(html("**b** *i* ++u++ ~~s~~"), "<p><b>b</b> <i>i</i> <u>u</u> <s>s</s></p>");
    assert_eq!(html("- one\n- two\nafter"), "<ul><li>one</li><li>two</li></ul><p>after</p>");
    assert_eq!(html("1. one\n2. two"), "<ol><li>one</li><li>two</li></ol>");
    assert_eq!(html("# Title\n### Small"), "<h1>Title</h1><h3>Small</h3>");
    assert_eq!(html("> quoted\n> more"), "<blockquote><p>quoted</p><p>more</p></blockquote>");
    assert_eq!(html("a\n\nb"), "<p>a</p><p></p><p>b</p>");
    assert_eq!(markup_rich_text("  \n "), None);
}

#[test]
fn keeps_unpaired_markers_and_escapes_as_text() {
    assert_eq!(html("2 * 3 = 6"), "<p>2 * 3 = 6</p>");
    assert_eq!(html("C++ and **open"), "<p>C++ and **open</p>");
    assert_eq!(html(r"\*not italic\*"), "<p>*not italic*</p>");
    assert_eq!(html("a < b & c"), "<p>a &lt; b &amp; c</p>");
}

#[test]
fn nests_overlapping_markers() {
    assert_eq!(html("**a *b** c*"), "<p><b>a <i>b</i></b><i> c</i></p>");
    assert_eq!(html("***both***"), "<p><b><i>both</i></b></p>");
}

#[test]
fn makes_links_with_safe_targets_only() {
    assert_eq!(
        html("see [the **site**](https://example.com)"),
        "<p>see <a href=\"https://example.com\" rel=\"noopener noreferrer\" target=\"_blank\">the <b>site</b></a></p>"
    );
    assert_eq!(html("[x](javascript:alert(1))"), "<p><a>x</a>)</p>");
}

#[test]
fn shows_stored_html_as_markup() {
    assert_eq!(rich_text_markup("<p>Hello <b>you</b></p><p><i>there</i></p>"), "Hello **you**\n*there*");
    assert_eq!(rich_text_markup("<ul><li>a</li><li><p>b</p></li></ul><ol><li>c</li><li>d</li></ol>"), "- a\n- b\n1. c\n2. d");
    assert_eq!(rich_text_markup("<h2>T</h2><blockquote><p>q</p></blockquote>"), "## T\n> q");
    assert_eq!(rich_text_markup("<b>Bold</b><script>alert(1)</script>"), "**Bold**");
    assert_eq!(rich_text_markup("<p>a<br>b</p>"), "a\nb");
    assert_eq!(rich_text_markup("<p>\n  spaced   out \n</p>"), "spaced out");
    assert_eq!(rich_text_markup("<p><a href=\"https://x.io\">x</a> <a href=\"javascript:1\">y</a></p>"), "[x](https://x.io) y");
}

#[test]
fn escapes_text_that_would_read_as_markup() {
    assert_eq!(rich_text_markup("<p>2*3 [x] C++ a+b</p>"), r"2\*3 \[x\] C\+\+ a+b");
    assert_eq!(rich_text_markup("<p>- not a list</p><p>1. nor this</p><p># nor this</p>"), "\\- not a list\n1\\. nor this\n\\# nor this");
    for stored in [
        "<p>2*3 [x] C++ a+b</p>",
        "<p>- not a list</p><p>1. nor this</p><p># nor this</p>",
        "<p>a <b>b <i>c</i></b> <u>d</u> <s>e</s></p><ul><li>x</li></ul><ol><li>y</li></ol><h1>h</h1>",
    ] {
        assert_eq!(markup_rich_text(&rich_text_markup(stored)).as_deref(), Some(stored), "round trip of {stored}");
    }
}

fn edit(text: &str, selection: std::ops::Range<usize>) -> Edit {
    Edit { text: text.into(), selection }
}

#[test]
fn toolbar_wraps_and_unwraps_the_selection() {
    assert_eq!(toggle_inline("make bold", 5..9, "**"), edit("make **bold**", 7..11));
    assert_eq!(toggle_inline("make **bold**", 7..11, "**"), edit("make bold", 5..9));
    assert_eq!(toggle_inline("make **bold**", 5..13, "**"), edit("make bold", 5..9));
    // A double-clicked word's trailing space stays outside.
    assert_eq!(toggle_inline("one two", 0..4, "*"), edit("*one* two", 1..4));
    // Italic is not mistaken for half of bold, and the reverse.
    assert_eq!(toggle_inline("**a**", 2..3, "*"), edit("***a***", 3..4));
    assert_eq!(toggle_inline("***a***", 3..4, "*"), edit("**a**", 2..3));
    assert_eq!(toggle_inline("*a*", 1..2, "**"), edit("***a***", 3..4));
    // Nothing selected: a pair with the caret inside.
    assert_eq!(toggle_inline("x ", 2..2, "++"), edit("x ++++", 4..4));
    // Across lines each line is wrapped after its list prefix.
    assert_eq!(toggle_inline("- a\n- b", 2..7, "~~"), edit("- ~~a~~\n- ~~b~~", 0..15));
}

#[test]
fn toolbar_turns_lines_into_lists_and_back() {
    assert_eq!(toggle_list("a\nb\nc", 0..3, false), edit("- a\n- b\nc", 0..7));
    assert_eq!(toggle_list("- a\n- b\nc", 0..7, false), edit("a\nb\nc", 0..3));
    assert_eq!(toggle_list("- a\n- b", 0..7, true), edit("1. a\n2. b", 0..9));
    assert_eq!(toggle_list("# a", 3..3, false), edit("- a", 3..3));
    // The caret keeps its place in the line's text.
    assert_eq!(toggle_list("x\nhello", 4..4, true), edit("x\n1. hello", 7..7));
    // A selection ending at a line start leaves that line alone.
    assert_eq!(toggle_list("a\nb", 0..2, false), edit("- a\nb", 0..3));
}
