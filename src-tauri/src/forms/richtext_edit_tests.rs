use super::{Edit, LineKind, code_block, divider, image, link, table, toggle_inline, toggle_lines};

fn edit(text: &str, selection: std::ops::Range<usize>) -> Edit {
    Edit { text: text.into(), selection }
}

#[test]
fn toolbar_wraps_and_unwraps_the_selection() {
    assert_eq!(toggle_inline("make bold", 5..9, "**", "**"), edit("make **bold**", 7..11));
    assert_eq!(toggle_inline("make **bold**", 7..11, "**", "**"), edit("make bold", 5..9));
    assert_eq!(toggle_inline("make **bold**", 5..13, "**", "**"), edit("make bold", 5..9));
    // A double-clicked word's trailing space stays outside.
    assert_eq!(toggle_inline("one two", 0..4, "*", "*"), edit("*one* two", 1..4));
    // Italic is not mistaken for half of bold, and the reverse.
    assert_eq!(toggle_inline("**a**", 2..3, "*", "*"), edit("***a***", 3..4));
    assert_eq!(toggle_inline("***a***", 3..4, "*", "*"), edit("**a**", 2..3));
    assert_eq!(toggle_inline("*a*", 1..2, "**", "**"), edit("***a***", 3..4));
    // Nothing selected: a pair with the caret inside.
    assert_eq!(toggle_inline("x ", 2..2, "`", "`"), edit("x ``", 3..3));
    // Underline has different open and close tags.
    assert_eq!(toggle_inline("a b", 2..3, "<u>", "</u>"), edit("a <u>b</u>", 5..6));
    assert_eq!(toggle_inline("a <u>b</u>", 5..6, "<u>", "</u>"), edit("a b", 2..3));
    assert_eq!(toggle_inline("a <u>b</u>", 2..10, "<u>", "</u>"), edit("a b", 2..3));
    // Across lines each line is wrapped after its list prefix.
    assert_eq!(toggle_inline("- a\n- b", 2..7, "~~", "~~"), edit("- ~~a~~\n- ~~b~~", 0..15));
}

#[test]
fn toolbar_turns_lines_into_lists_headings_and_quotes() {
    assert_eq!(toggle_lines("a\nb\nc", 0..3, LineKind::Bullet), edit("- a\n- b\nc", 0..7));
    assert_eq!(toggle_lines("- a\n- b\nc", 0..7, LineKind::Bullet), edit("a\nb\nc", 0..3));
    assert_eq!(toggle_lines("- a\n- b", 0..7, LineKind::Numbered), edit("1. a\n2. b", 0..9));
    assert_eq!(toggle_lines("# a", 3..3, LineKind::Bullet), edit("- a", 3..3));
    assert_eq!(toggle_lines("- a", 3..3, LineKind::Task), edit("- [ ] a", 7..7));
    assert_eq!(toggle_lines("- [x] a", 7..7, LineKind::Task), edit("a", 1..1));
    assert_eq!(toggle_lines("a", 0..0, LineKind::Heading(2)), edit("## a", 3..3));
    assert_eq!(toggle_lines("# a", 0..0, LineKind::Heading(2)), edit("## a", 3..3));
    assert_eq!(toggle_lines("## a", 0..0, LineKind::Heading(2)), edit("a", 0..0));
    assert_eq!(toggle_lines("a\nb", 0..3, LineKind::Quote), edit("> a\n> b", 0..7));
    // The caret keeps its place in the line's text.
    assert_eq!(toggle_lines("x\nhello", 4..4, LineKind::Numbered), edit("x\n1. hello", 7..7));
    // A selection ending at a line start leaves that line alone.
    assert_eq!(toggle_lines("a\nb", 0..2, LineKind::Bullet), edit("- a\nb", 0..3));
}

#[test]
fn toolbar_inserts_blocks_on_their_own_lines() {
    assert_eq!(code_block("", 0..0), edit("```\n\n```", 4..4));
    assert_eq!(code_block("see x = 1 here", 4..9), edit("see\n\n```\nx = 1\n```\n\nhere", 9..14));
    assert_eq!(divider("a\n", 2..2), edit("a\n\n---", 6..6));
    let t = table("", 0..0);
    assert_eq!(&t.text[t.selection.clone()], "Column 1");
    assert!(t.text.starts_with("| Column 1 | Column 2 |\n| --- | --- |"));
}

#[test]
fn toolbar_inserts_links_and_images() {
    assert_eq!(link("go here", 3..7), edit("go [here](https://)", 10..18));
    assert_eq!(link("", 0..0), edit("[text](https://)", 1..5));
    assert_eq!(image("a ", 2..2, "photo", "attachment:photo.png"), edit("a ![photo](attachment:photo.png)", 32..32));
    assert_eq!(image("x", 0..1, "p", "attachment:a b.png"), edit("![x](<attachment:a b.png>)", 26..26));
}
