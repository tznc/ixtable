//! Splitting a band's items across pages: where the cut falls and which items go to
//! which piece.

use super::text::{BASELINE, LINE_HEIGHT};
use super::{move_chart, r2, Item, TextItem, TextLine};

/// Coordinates are rounded to 0.01 pt, so comparisons allow that much.
const TOL: f64 = 0.01;

fn line_top(item: &TextItem, line: &TextLine) -> f64 {
    line.y - item.font_size * BASELINE
}

fn line_bottom(item: &TextItem, line: &TextLine) -> f64 {
    line_top(item, line) + item.font_size * LINE_HEIGHT
}

/// Where a piece of a band that starts at `from` and may reach `limit` ends (coordinates
/// relative to the band top). The cut falls between text lines; an image, chart or line
/// that crosses it moves it above that item. Rectangles and text frames don't move it:
/// they are clipped. Returns `from` when nothing fits.
pub fn cut_at(items: &[Item], from: f64, limit: f64) -> f64 {
    let mut cut = limit;
    let mut changed = true;
    while changed {
        changed = false;
        for item in items {
            let top = item.y();
            let bottom = item.y() + item.h();
            if top >= cut - TOL || bottom <= cut + TOL || matches!(item, Item::Rect(_)) {
                continue;
            }
            let mut next = cut;
            if let Item::Text(text) = item {
                if let Some(crossing) = text.lines.iter().find(|l| line_bottom(text, l) > cut + TOL) {
                    next = line_top(text, crossing);
                }
            } else {
                next = top;
            }
            next = next.max(from);
            if next < cut - TOL {
                cut = next;
                changed = true;
            }
        }
    }
    cut
}

/// The items of one piece of a band, from `from` (inclusive) to `to` (exclusive), moved
/// down by `dy`. Text keeps the lines that start in the piece; rectangles and vertical
/// lines are clipped to it; images and horizontal lines go to the piece their top is in.
pub fn slice_items(items: &[Item], from: f64, to: f64, dy: f64) -> Vec<Item> {
    let inside = |y: f64| y >= from - TOL && y < to - TOL;
    let mut out = vec![];
    for item in items {
        let top = item.y().max(from);
        let h = (item.y() + item.h()).min(to) - top;
        match item {
            Item::Text(text) => {
                let lines: Vec<TextLine> = text
                    .lines
                    .iter()
                    .filter(|l| inside(line_top(text, l)))
                    .cloned()
                    .collect();
                if lines.is_empty() && !(text.lines.is_empty() && inside(text.y)) {
                    continue;
                }
                out.push(Item::Text(TextItem {
                    y: r2(top + dy),
                    h: r2(h),
                    lines: lines
                        .into_iter()
                        .map(|l| TextLine {
                            y: r2(l.y + dy),
                            ..l
                        })
                        .collect(),
                    ..text.clone()
                }));
            }
            Item::Chart(chart) => {
                if inside(chart.y) {
                    out.push(move_chart(chart, dy));
                }
            }
            Item::Image(image) => {
                if inside(image.y) {
                    let mut moved = image.clone();
                    moved.y = r2(image.y + dy);
                    out.push(Item::Image(moved));
                }
            }
            Item::Line(line) if line.h == 0.0 => {
                if inside(line.y) {
                    let mut moved = line.clone();
                    moved.y = r2(line.y + dy);
                    out.push(Item::Line(moved));
                }
            }
            Item::Line(line) => {
                if h > TOL {
                    let mut clipped = line.clone();
                    clipped.y = r2(top + dy);
                    clipped.h = r2(h);
                    out.push(Item::Line(clipped));
                }
            }
            Item::Rect(rect) => {
                if h > TOL {
                    let mut clipped = rect.clone();
                    clipped.y = r2(top + dy);
                    clipped.h = r2(h);
                    out.push(Item::Rect(clipped));
                }
            }
        }
    }
    out
}
