//! Report layout and PDF engine (PRD §15, docs/decisions/report-engine.md): a pure,
//! deterministic port of the former TypeScript engine (`src/reports/engine`,
//! `pdf.ts`). `layout_report` turns a report definition and its rows into laid-out
//! pages; `pdf::write_pdf` serializes them. Neither touches the UI, so previews, print
//! and exported PDFs all draw the same document.
//!
//! - `component`: typed view of the report definition (`crate::reports` keeps
//!   component settings untyped).
//! - `text`: Helvetica metrics, wrapping and the fallback fonts.
//! - `values`, `running`, `render`, `grow`, `split`, `subreport`, `layout`: the engine.
//! - `chart`, `chart_geometry`, `chart_paths`: report charts.
//! - `model`, `parameters`, `data`: designer model helpers, parameter prompts and the
//!   data loading around a run.
//! - `pdf`, `pdf_fonts`, `pdf_images`: the PDF 1.4 writer.

pub mod chart;
pub mod chart_geometry;
pub mod chart_paths;
pub mod component;
pub mod data;
pub mod grow;
pub mod layout;
mod metrics;
pub mod model;
pub mod parameters;
pub mod pdf;
pub mod pdf_fonts;
pub mod pdf_images;
pub mod render;
pub mod running;
pub mod split;
pub mod subreport;
pub mod text;
pub mod values;

#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_layout;
#[cfg(test)]
mod tests_more;
#[cfg(test)]
mod tests_model;
#[cfg(test)]
mod tests_pdf;
#[cfg(test)]
mod tests_phase6;
#[cfg(test)]
mod tests_resources;
#[cfg(test)]
mod tests_subreport;

pub use layout::layout_report;

use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashMap;

/// One query or dataset row: column name to plain value.
pub type Row = Map<String, Value>;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TextLine {
    pub text: String,
    /// Left edge of the line (alignment already applied).
    pub x: f64,
    /// Baseline.
    pub y: f64,
    pub width: f64,
}

/// A path operation in page points.
#[derive(Debug, Clone, PartialEq)]
pub enum PathOp {
    M(f64, f64),
    L(f64, f64),
    /// Cubic Bézier curve: two control points and the end point.
    C(f64, f64, f64, f64, f64, f64),
    Z,
}

impl Serialize for PathOp {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let (tag, nums): (&str, Vec<f64>) = match self {
            PathOp::M(x, y) => ("M", vec![*x, *y]),
            PathOp::L(x, y) => ("L", vec![*x, *y]),
            PathOp::C(a, b, c, d, e, f) => ("C", vec![*a, *b, *c, *d, *e, *f]),
            PathOp::Z => ("Z", vec![]),
        };
        let mut seq = s.serialize_seq(Some(nums.len() + 1))?;
        seq.serialize_element(tag)?;
        for n in nums {
            seq.serialize_element(&n)?;
        }
        seq.end()
    }
}

/// One chart mark. Colors are `#rrggbb`; `None` means no fill or no stroke.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathItem {
    pub ops: Vec<PathOp>,
    pub fill: Option<String>,
    pub stroke: Option<String>,
    pub line_width: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextItem {
    pub component_id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub text: String,
    pub lines: Vec<TextLine>,
    pub font_size: f64,
    pub bold: bool,
    pub gray: f64,
}

/// From (x, y) to (x + w, y + h); one of w and h is 0.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineItem {
    pub component_id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub line_width: f64,
    pub gray: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RectItem {
    pub component_id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Stroke width; 0 = no stroke.
    pub line_width: f64,
    pub gray: f64,
    pub fill: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageItem {
    pub component_id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub asset_id: String,
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Mark {
    Path(PathItem),
    Text(TextItem),
}

/// A chart: paths and text marks in page coordinates, drawn in order. It never splits
/// across pages; `title` labels it for assistive technology.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartItem {
    pub component_id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub title: String,
    pub marks: Vec<Mark>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Item {
    Text(TextItem),
    Line(LineItem),
    Rect(RectItem),
    Image(ImageItem),
    Chart(ChartItem),
}

impl Item {
    pub fn component_id(&self) -> &str {
        match self {
            Item::Text(i) => &i.component_id,
            Item::Line(i) => &i.component_id,
            Item::Rect(i) => &i.component_id,
            Item::Image(i) => &i.component_id,
            Item::Chart(i) => &i.component_id,
        }
    }
    pub fn y(&self) -> f64 {
        match self {
            Item::Text(i) => i.y,
            Item::Line(i) => i.y,
            Item::Rect(i) => i.y,
            Item::Image(i) => i.y,
            Item::Chart(i) => i.y,
        }
    }
    pub fn h(&self) -> f64 {
        match self {
            Item::Text(i) => i.h,
            Item::Line(i) => i.h,
            Item::Rect(i) => i.h,
            Item::Image(i) => i.h,
            Item::Chart(i) => i.h,
        }
    }
    pub fn x(&self) -> f64 {
        match self {
            Item::Text(i) => i.x,
            Item::Line(i) => i.x,
            Item::Rect(i) => i.x,
            Item::Image(i) => i.x,
            Item::Chart(i) => i.x,
        }
    }
    pub fn w(&self) -> f64 {
        match self {
            Item::Text(i) => i.w,
            Item::Line(i) => i.w,
            Item::Rect(i) => i.w,
            Item::Image(i) => i.w,
            Item::Chart(i) => i.w,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Page {
    pub number: usize,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportDiagnostic {
    pub component_id: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReportDocument {
    pub width: f64,
    pub height: f64,
    pub pages: Vec<Page>,
    /// Expression and data problems, one per component and message.
    pub diagnostics: Vec<ReportDiagnostic>,
}

/// An application asset the layout knows about.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetInfo {
    pub media_type: String,
}

/// Subreport definition and all its rows (each instance filters them by its links).
#[derive(Debug, Clone)]
pub struct SubreportData {
    pub report: crate::reports::Report,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, Default)]
pub struct LayoutOptions {
    /// Parameter values exposed as `params`. Defaults to the report's `params`.
    pub params: Option<Map<String, Value>>,
    /// Clock for `today()` / `now()`. Pass a fixed value for reproducible output.
    pub now: Option<chrono::DateTime<chrono::Utc>>,
    /// Rows of saved queries used by table and chart components, by query id.
    pub tables: HashMap<String, Vec<Row>>,
    /// Known application assets by id (images). Missing assets render as a labelled box.
    pub assets: HashMap<String, AssetInfo>,
    /// Subreport definitions and rows by report id.
    pub subreports: HashMap<String, SubreportData>,
}

/// JavaScript's `Math.round`: halves round toward positive infinity.
pub fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// Rounds to 1/100 pt so golden layouts don't carry floating-point noise.
pub fn r2(n: f64) -> f64 {
    js_round(n * 100.0) / 100.0 + 0.0
}

fn move_path(ops: &[PathOp], dy: f64) -> Vec<PathOp> {
    ops.iter()
        .map(|op| match op {
            PathOp::M(x, y) => PathOp::M(*x, r2(y + dy)),
            PathOp::L(x, y) => PathOp::L(*x, r2(y + dy)),
            PathOp::C(a, b, c, d, e, f) => PathOp::C(*a, r2(b + dy), *c, r2(d + dy), *e, r2(f + dy)),
            PathOp::Z => PathOp::Z,
        })
        .collect()
}

/// A chart item moved down by `dy` with all of its marks.
pub fn move_chart(item: &ChartItem, dy: f64) -> Item {
    Item::Chart(ChartItem {
        y: r2(item.y + dy),
        marks: item
            .marks
            .iter()
            .map(|mark| match mark {
                Mark::Path(p) => Mark::Path(PathItem {
                    ops: move_path(&p.ops, dy),
                    ..p.clone()
                }),
                Mark::Text(t) => Mark::Text(TextItem {
                    y: r2(t.y + dy),
                    lines: t
                        .lines
                        .iter()
                        .map(|l| TextLine {
                            y: r2(l.y + dy),
                            ..l.clone()
                        })
                        .collect(),
                    ..t.clone()
                }),
            })
            .collect(),
        ..item.clone()
    })
}
