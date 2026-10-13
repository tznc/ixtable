//! Find and replace over datasheet cells (port of `src/data/sheet/find.ts`).

use crate::data::DataValue;
use crate::recordstore::FieldSettings;
use crate::ui::fields::datasheet::datasheet_text;
use crate::ui::schema::logical::show_value;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FindOptions {
    pub find: String,
    pub match_case: bool,
    /// Match the whole cell text rather than any part of it.
    pub whole_field: bool,
}

fn same(a: char, b: char, match_case: bool) -> bool {
    a == b || (!match_case && a.to_lowercase().eq(b.to_lowercase()))
}

/// Start of the first match at or after char index `from`, as (start, length).
fn find_from(text: &[char], options: &FindOptions, from: usize) -> Option<(usize, usize)> {
    let needle: Vec<char> = options.find.chars().collect();
    if options.whole_field {
        let whole = from == 0
            && text.len() == needle.len()
            && text.iter().zip(&needle).all(|(a, b)| same(*a, *b, options.match_case));
        return whole.then_some((0, text.len()));
    }
    if needle.is_empty() {
        return (from <= text.len()).then_some((from, 0));
    }
    (from..text.len().saturating_sub(needle.len() - 1)).find_map(|start| {
        let end = start + needle.len();
        (end <= text.len()
            && text[start..end]
                .iter()
                .zip(&needle)
                .all(|(a, b)| same(*a, *b, options.match_case)))
        .then_some((start, needle.len()))
    })
}

/// The text a datasheet cell shows (masked or formatted by its field), or None for
/// cells find skips (nulls and blobs).
pub fn cell_text(value: &DataValue, field: Option<&FieldSettings>) -> Option<String> {
    match value {
        DataValue::Null | DataValue::Blob(_) => None,
        value => Some(datasheet_text(value, field).unwrap_or_else(|| show_value(value))),
    }
}

pub fn cell_matches(value: &DataValue, options: &FindOptions, field: Option<&FieldSettings>) -> bool {
    if options.find.is_empty() {
        return false;
    }
    cell_text(value, field).is_some_and(|text| {
        let chars: Vec<char> = text.chars().collect();
        find_from(&chars, options, 0).is_some()
    })
}

/// `text` with every match replaced (the whole text when matching whole fields).
pub fn replace_text(text: &str, replacement: &str, options: &FindOptions) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut at = 0;
    while let Some((start, len)) = find_from(&chars, options, at) {
        out.extend(&chars[at..start]);
        out.push_str(replacement);
        at = start + len;
        if len == 0 {
            // An empty match advances one character, as a global regex does.
            if at < chars.len() {
                out.push(chars[at]);
            }
            at += 1;
        }
        if at > chars.len() {
            break;
        }
    }
    if at <= chars.len() {
        out.extend(&chars[at..]);
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellAt {
    pub row: usize,
    pub column: usize,
}

/// The first matching cell after `after` in reading order (row by row, through
/// `columns` only), or None. Without `after` the search starts at the first cell.
pub fn next_match(
    rows: &[Vec<DataValue>],
    columns: &[usize],
    options: &FindOptions,
    after: Option<CellAt>,
    fields: &[Option<FieldSettings>],
) -> Option<CellAt> {
    let start_row = after.map_or(0, |a| a.row);
    for row in start_row..rows.len() {
        for &column in columns {
            if let Some(after) = after {
                let here = columns.iter().position(|c| *c == column);
                let there = columns.iter().position(|c| *c == after.column);
                if row == start_row && here <= there {
                    continue;
                }
            }
            let field = fields.get(column).and_then(Option::as_ref);
            if cell_matches(&rows[row][column], options, field) {
                return Some(CellAt { row, column });
            }
        }
    }
    None
}
