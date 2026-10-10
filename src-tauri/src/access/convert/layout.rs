//! Absolute Access layouts (twips) → ixtable grid placements.
//!
//! Controls are grouped into rows by vertical overlap, then each control's
//! left edge and width are scaled to a 12-column grid. Overlaps after rounding
//! push a control right, and a control that no longer fits starts a new row.
use serde_json::{json, Value};

pub const COLUMNS: i64 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub left: i64,
    pub top: i64,
    pub width: i64,
    pub height: i64,
}

impl Rect {
    pub fn right(&self) -> i64 {
        self.left + self.width
    }

    pub fn bottom(&self) -> i64 {
        self.top + self.height
    }

    pub fn union(&self, o: &Rect) -> Rect {
        let left = self.left.min(o.left);
        let top = self.top.min(o.top);
        Rect {
            left,
            top,
            width: self.right().max(o.right()) - left,
            height: self.bottom().max(o.bottom()) - top,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub column: i64,
    pub row: i64,
    pub span: i64,
}

impl Placement {
    pub fn json(&self) -> Value {
        json!({ "column": self.column, "row": self.row, "columnSpan": self.span })
    }
}

/// Places rectangles (twips, relative to `origin_left`) on a grid `width` twips wide,
/// starting at grid row `first_row`. Returns placements in input order and the next free row.
pub fn place(
    items: &[Rect],
    origin_left: i64,
    width: i64,
    first_row: i64,
) -> (Vec<Placement>, i64) {
    let width = width
        .max(
            items
                .iter()
                .map(|r| r.right() - origin_left)
                .max()
                .unwrap_or(0),
        )
        .max(1);
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|&i| (items[i].top, items[i].left));
    // Group into rows: an item joins the current row when it starts above the row's middle.
    let mut rows: Vec<Vec<usize>> = vec![];
    let mut band: Option<(i64, i64)> = None;
    for i in order {
        let r = items[i];
        match band {
            Some((top, bottom)) if r.top < top + (bottom - top).max(1) / 2 + 60 => {
                rows.last_mut().expect("a row exists").push(i);
                band = Some((top, bottom.max(r.bottom())));
            }
            _ => {
                rows.push(vec![i]);
                band = Some((r.top, r.bottom()));
            }
        }
    }
    let mut out = vec![
        Placement {
            column: 1,
            row: first_row,
            span: 1
        };
        items.len()
    ];
    let mut row = first_row;
    for mut members in rows {
        members.sort_by_key(|&i| items[i].left);
        let mut next_free = 1;
        for i in members {
            let r = items[i];
            let mut column =
                ((r.left - origin_left) as f64 / width as f64 * COLUMNS as f64).round() as i64 + 1;
            let mut span = ((r.width as f64 / width as f64 * COLUMNS as f64).round() as i64).max(1);
            column = column.clamp(1, COLUMNS);
            if column < next_free {
                column = next_free;
            }
            if column > COLUMNS {
                row += 1;
                column = 1;
            }
            span = span.min(COLUMNS - column + 1);
            out[i] = Placement { column, row, span };
            next_free = column + span;
        }
        row += 1;
    }
    (out, row)
}

/// The default 12-column grid (for tab pages and sections).
pub fn grid_layout() -> Value {
    let tracks: Vec<Value> = (0..COLUMNS)
        .map(|_| json!({ "kind": "fr", "value": 1 }))
        .collect();
    json!({ "columns": tracks, "rows": [], "columnGap": 16, "rowGap": 16, "padding": 0, "justifyItems": "stretch", "alignItems": "start", "namedRegions": [], "breakpoints": [] })
}
