//! Multi-cell paste from spreadsheets (port of `src/data/sheet/paste.ts`).

use super::rows::{original_values, typed_values, SheetFields};
use crate::data::Page;
use crate::ui::widgets::plural;
use crate::records::{RecordWrite, RecordWriteMeta};
use std::collections::BTreeMap;

/// Parses spreadsheet clipboard text (Excel, Numbers, Google Sheets: tab-separated
/// rows, CRLF or LF line ends, fields with tabs, newlines or quotes wrapped in double
/// quotes) into rows of cells. A trailing line end adds no empty row.
pub fn parse_clipboard_grid(text: &str) -> Vec<Vec<String>> {
    let chars: Vec<char> = text.chars().collect();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut i = 0;
    let mut quoted = false;
    while i < chars.len() {
        let ch = chars[i];
        if quoted {
            if ch == '"' && chars.get(i + 1) == Some(&'"') {
                cell.push('"');
                i += 2;
                continue;
            }
            if ch == '"' {
                quoted = false;
            } else {
                cell.push(ch);
            }
            i += 1;
            continue;
        }
        if ch == '"' && cell.is_empty() {
            quoted = true;
        } else if ch == '\t' {
            row.push(std::mem::take(&mut cell));
        } else if ch == '\n' || ch == '\r' {
            row.push(std::mem::take(&mut cell));
            rows.push(std::mem::take(&mut row));
            if ch == '\r' && chars.get(i + 1) == Some(&'\n') {
                i += 1;
            }
        } else {
            cell.push(ch);
        }
        i += 1;
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

/// True when pasted text spans more than one cell, so the datasheet should take it over.
pub fn is_multi_cell(text: &str) -> bool {
    let mut body = text;
    while let Some(rest) = body
        .strip_suffix("\r\n")
        .or_else(|| body.strip_suffix('\n'))
    {
        body = rest;
    }
    body.contains(['\t', '\n', '\r'])
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PastePlan {
    /// Existing rows (page row index) and the column-to-text cells pasted over them.
    pub updates: Vec<(usize, BTreeMap<usize, String>)>,
    /// New rows, each a column-to-text map.
    pub inserts: Vec<BTreeMap<usize, String>>,
}

/// Maps a parsed grid onto the datasheet: cell (0, 0) lands on `start`, later columns
/// follow `columns` (the visible, editable column order) and cells past the last one
/// are dropped. Rows past `row_count` become new records. `start.row` equal to
/// `row_count` means the paste began in the new-record row.
pub fn plan_paste(
    grid: &[Vec<String>],
    columns: &[usize],
    start: (usize, usize),
    row_count: usize,
) -> PastePlan {
    let (start_row, start_column) = start;
    let mut plan = PastePlan::default();
    let Some(first) = columns.iter().position(|c| *c == start_column) else {
        return plan;
    };
    for (offset, values) in grid.iter().enumerate() {
        let mut cells = BTreeMap::new();
        for (k, text) in values.iter().enumerate() {
            if let Some(column) = columns.get(first + k) {
                cells.insert(*column, text.clone());
            }
        }
        let row = start_row + offset;
        if row < row_count {
            plan.updates.push((row, cells));
        } else {
            plan.inserts.push(cells);
        }
    }
    plan
}

/// The record writes for `plan`: updates carry the row as read (`expected`), new rows
/// leave empty cells to their defaults. A cell that does not parse for its column
/// fails, naming the pasted row.
pub fn paste_writes(
    table: &str,
    page: &Page,
    plan: &PastePlan,
    fields: &SheetFields,
) -> Result<Vec<RecordWrite>, String> {
    let named = |k: usize, reason: String| format!("Pasted row {}: {reason}.", k + 1);
    let mut writes = Vec::new();
    for (k, (row, cells)) in plan.updates.iter().enumerate() {
        let values =
            typed_values(&page.columns, cells, true, fields).map_err(|e| named(k, e))?;
        writes.push(
            RecordWrite::update(table, values, page.identities[*row].clone()).with_meta(
                RecordWriteMeta {
                    expected: Some(original_values(&page.columns, &page.rows[*row])),
                    ..Default::default()
                },
            ),
        );
    }
    for (k, cells) in plan.inserts.iter().enumerate() {
        let values = typed_values(&page.columns, cells, false, fields)
            .map_err(|e| named(plan.updates.len() + k, e))?;
        writes.push(RecordWrite::insert(table, values));
    }
    Ok(writes)
}

/// Summary shown after a paste, e.g. "Pasted into 2 records and added 1 record."
pub fn pasted_message(updated: usize, added: usize) -> String {
    if added == 0 {
        format!("Pasted into {}.", plural(updated, "record"))
    } else if updated == 0 {
        format!("Added {}.", plural(added, "record"))
    } else {
        format!("Pasted into {} and added {}.", plural(updated, "record"), plural(added, "record"))
    }
}
