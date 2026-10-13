//! One laid-out report page as GPUI elements, in points scaled by the zoom (port of
//! `PageView.tsx`). Text lines are positioned at the engine's baselines; chart paths are
//! painted on one canvas under the page's text.
//!
//! Test ids: the page is `report-page` (label "Page N of M"); each text line is
//! `page-text-<item>-<line>` (its text is the label, chart text marks are
//! `page-text-<item>-m<mark>`); a chart is `page-chart-<item>` (label: its title).

use crate::report_engine::{
    text::{BASELINE, LINE_HEIGHT},
    ChartItem, Item, Mark, Page, PathItem, PathOp, ReportDocument, TextItem,
};
use crate::ui::prelude::*;
use gpui_kit::{canvas, hsla, img, rgb, Image, ImageFormat, ObjectFit, PathBuilder};
use std::{collections::HashMap, sync::Arc};

/// Decoded image assets by asset id.
pub type PageImages = HashMap<String, Arc<Image>>;

/// An `Image` for GPUI from asset bytes, when the format is one the page can show.
pub fn page_image(media_type: &str, data: &[u8]) -> Option<Arc<Image>> {
    let format = match media_type {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        "image/webp" => ImageFormat::Webp,
        _ => return None,
    };
    Some(Arc::new(Image::from_bytes(format, data.to_vec())))
}

/// Gray level (0 black, 1 white) as a color.
fn gray(level: f64) -> Hsla {
    hsla(0.0, 0.0, level.clamp(0.0, 1.0) as f32, 1.0)
}

/// `#rrggbb` as a color.
fn hex(color: &str) -> Hsla {
    let value = u32::from_str_radix(color.trim_start_matches('#'), 16).unwrap_or(0);
    rgb(value).into()
}

fn at(v: f64, zoom: f32) -> Pixels {
    px(v as f32 * zoom)
}

fn text_lines(item: &TextItem, key: &str, zoom: f32) -> Vec<AnyElement> {
    let size = item.font_size;
    item.lines
        .iter()
        .enumerate()
        .map(|(j, line)| {
            let (left, top) = (at(line.x, zoom), at(line.y - size * BASELINE, zoom));
            let (font, height) = (at(size, zoom), at(size * LINE_HEIGHT, zoom));
            let (color, bold) = (gray(item.gray), item.bold);
            widgets::styled_text(format!("page-text-{key}-{j}"), line.text.clone(), move |d| {
                d.absolute()
                    .left(left)
                    .top(top)
                    .h(height)
                    .text_size(font)
                    .line_height(height)
                    .whitespace_nowrap()
                    .text_color(color)
                    .when(bold, |d| d.font_weight(FontWeight::BOLD))
            })
            .into_any_element()
        })
        .collect()
}

fn chart_paths(chart: &ChartItem) -> impl Iterator<Item = &PathItem> {
    chart.marks.iter().filter_map(|m| match m {
        Mark::Path(p) => Some(p),
        Mark::Text(_) => None,
    })
}

/// Paints the chart paths of a page; `origin` is the page's top-left corner.
fn paint_paths(paths: Vec<PathItem>, zoom: f32, bounds: Bounds<Pixels>, window: &mut Window) {
    let point_at = |x: f64, y: f64| point(bounds.origin.x + px(x as f32 * zoom), bounds.origin.y + px(y as f32 * zoom));
    let trace = |builder: &mut PathBuilder, ops: &[PathOp]| {
        for op in ops {
            match op {
                PathOp::M(x, y) => builder.move_to(point_at(*x, *y)),
                PathOp::L(x, y) => builder.line_to(point_at(*x, *y)),
                PathOp::C(a, b, c, d, e, f) => {
                    builder.cubic_bezier_to(point_at(*e, *f), point_at(*a, *b), point_at(*c, *d))
                }
                PathOp::Z => builder.close(),
            }
        }
    };
    for path in &paths {
        if let Some(fill) = &path.fill {
            let mut builder = PathBuilder::fill();
            trace(&mut builder, &path.ops);
            if let Ok(built) = builder.build() {
                window.paint_path(built, hex(fill));
            }
        }
        if let Some(stroke) = path.stroke.as_ref().filter(|_| path.line_width > 0.0) {
            let mut builder = PathBuilder::stroke(px(path.line_width as f32 * zoom));
            trace(&mut builder, &path.ops);
            if let Ok(built) = builder.build() {
                window.paint_path(built, hex(stroke));
            }
        }
    }
}

/// `d` placed at a rectangle of the page.
fn place<E: Styled>(d: E, rect: (f64, f64, f64, f64), zoom: f32) -> E {
    let (x, y, w, h) = rect;
    d.absolute().left(at(x, zoom)).top(at(y, zoom)).w(at(w, zoom)).h(at(h, zoom))
}

/// `d` with a border of `width` on every side.
fn bordered<E: Styled>(mut d: E, width: Pixels, color: Hsla) -> E {
    let edges = &mut d.style().border_widths;
    for edge in [&mut edges.top, &mut edges.right, &mut edges.bottom, &mut edges.left] {
        *edge = Some(width.into());
    }
    d.border_color(color)
}

fn item_element(i: usize, item: &Item, zoom: f32, images: &PageImages) -> Vec<AnyElement> {
    match item {
        Item::Rect(r) => {
            let mut d = place(div(), (r.x, r.y, r.w, r.h), zoom);
            if let Some(fill) = r.fill {
                d = d.bg(gray(fill));
            }
            if r.line_width > 0.0 {
                d = bordered(d, at(r.line_width, zoom), gray(r.gray));
            }
            vec![d.into_any_element()]
        }
        Item::Line(l) => {
            let (w, h) = if l.h == 0.0 { (l.w, l.line_width) } else { (l.line_width, l.h) };
            let (x, y) = if l.h == 0.0 { (l.x, l.y - l.line_width / 2.0) } else { (l.x - l.line_width / 2.0, l.y) };
            vec![place(div(), (x, y, w, h), zoom).bg(gray(l.gray)).into_any_element()]
        }
        Item::Text(t) => text_lines(t, &i.to_string(), zoom),
        Item::Image(m) => match images.get(&m.asset_id) {
            Some(image) => vec![place(div(), (m.x, m.y, m.w, m.h), zoom)
                .child(img(image.clone()).size_full().object_fit(ObjectFit::Fill))
                .into_any_element()],
            None => vec![bordered(place(div(), (m.x, m.y, m.w, m.h), zoom), px(1.0), gray(0.55)).into_any_element()],
        },
        Item::Chart(c) => {
            let frame = place(div().id(format!("page-chart-{i}")).aria_label(c.title.clone()), (c.x, c.y, c.w, c.h), zoom);
            let mut out = vec![widgets::observed(frame).into_any_element()];
            for (k, mark) in c.marks.iter().enumerate() {
                if let Mark::Text(t) = mark {
                    out.extend(text_lines(t, &format!("{i}-m{k}"), zoom));
                }
            }
            out
        }
    }
}

/// The page at `zoom` (1 = one point per pixel). `label` names it for tests and
/// assistive technology.
pub fn page_view(doc: &ReportDocument, page: &Page, zoom: f32, label: &str, images: &PageImages) -> impl IntoElement {
    let paths: Vec<PathItem> = page
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Chart(c) => Some(chart_paths(c).cloned().collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect();
    let charts = canvas(|_, _, _| (), move |bounds, _, window, _| paint_paths(paths, zoom, bounds, window))
        .absolute()
        .size_full();
    let items: Vec<AnyElement> = page
        .items
        .iter()
        .enumerate()
        .flat_map(|(i, item)| item_element(i, item, zoom, images))
        .collect();
    div()
        .id("report-page")
        .aria_label(label.to_string())
        .relative()
        .flex_none()
        .w(at(doc.width, zoom))
        .h(at(doc.height, zoom))
        .bg(gray(1.0))
        .overflow_hidden()
        .child(charts)
        .children(items)
        .test_support()
}
