//! Items of one component: text, shapes, images, tables and the evaluation context of a
//! band instance.

use super::chart::chart_items;
use super::component::{Comp, Kind, Style, TableColumn};
use super::text::{measure_text, normalize_text, wrap_text, BASELINE, LINE_HEIGHT};
use super::values::{styled_component, RunningValue, ERROR_TEXT};
use super::{r2, AssetInfo, Item, LineItem, RectItem, ReportDiagnostic, Row, TextItem, TextLine};
use crate::expr::{self, EvaluateOptions, ExprError, Scope};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub const DEFAULT_FONT_SIZE: f64 = 10.0;
pub const TABLE_FONT_SIZE: f64 = 9.0;
const TEXT_PAD: f64 = 2.0;
const CELL_PAD_X: f64 = 3.0;
const CELL_PAD_Y: f64 = 2.0;
const HEADER_FILL: f64 = 0.9;
const CELL_LINE: f64 = 0.5;

/// Names an expression sees for one band instance. Everything but `rows` is a plain
/// entry; `rows` is shared and only copied into an evaluation scope when the expression
/// mentions it, so a detail band over a large group stays cheap.
#[derive(Debug, Clone)]
pub struct ScopeData {
    pub entries: Scope,
    pub rows: Rc<Vec<Row>>,
}

impl ScopeData {
    /// The current record (`null` outside rows).
    pub fn record(&self) -> Value {
        self.entries.get("record").cloned().unwrap_or(Value::Null)
    }

    /// A copy with `overrides` replacing entries of the same name.
    pub fn with(&self, overrides: &[(&str, Value)]) -> ScopeData {
        let mut entries = self.entries.clone();
        for (key, value) in overrides {
            entries.insert((*key).to_string(), value.clone());
        }
        ScopeData {
            entries,
            rows: self.rows.clone(),
        }
    }

    /// The evaluation scope for `src` with `overrides`.
    pub fn scope_for(&self, src: &str, overrides: &[(&str, Value)]) -> Scope {
        let mut scope = self.entries.clone();
        for (key, value) in overrides {
            scope.insert((*key).to_string(), value.clone());
        }
        if src.contains("rows") && !scope.contains_key("rows") {
            scope.insert(
                "rows".into(),
                Value::Array(self.rows.iter().map(|r| Value::Object(r.clone())).collect()),
            );
        }
        scope
    }

    pub fn eval(
        &self,
        src: &str,
        overrides: &[(&str, Value)],
        now: Option<DateTime<Utc>>,
    ) -> Result<Value, ExprError> {
        expr::evaluate(src, &self.scope_for(src, overrides), EvaluateOptions { now })
    }

    pub fn eval_bool(
        &self,
        src: &str,
        overrides: &[(&str, Value)],
        now: Option<DateTime<Utc>>,
    ) -> Result<bool, ExprError> {
        expr::evaluate_boolean(src, &self.scope_for(src, overrides), EvaluateOptions { now })
    }
}

/// Expression and data problems of a run, one per component and message.
#[derive(Default)]
pub struct Diagnostics {
    list: RefCell<Vec<ReportDiagnostic>>,
    seen: RefCell<HashSet<(String, String)>>,
}

impl Diagnostics {
    pub fn add(&self, component_id: &str, message: &str) {
        let key = (component_id.to_string(), message.to_string());
        if self.seen.borrow_mut().insert(key) {
            self.list.borrow_mut().push(ReportDiagnostic {
                component_id: component_id.into(),
                message: message.into(),
            });
        }
    }
    pub fn into_vec(&self) -> Vec<ReportDiagnostic> {
        self.list.take()
    }
}

/// Everything layout reads besides a band instance's scope.
pub struct Env<'a> {
    pub now: Option<DateTime<Utc>>,
    pub tables: &'a HashMap<String, Vec<Row>>,
    pub assets: &'a HashMap<String, AssetInfo>,
    pub diagnostics: Diagnostics,
}

/// Evaluation context for one band instance.
pub struct RenderContext<'a> {
    pub env: &'a Env<'a>,
    pub scope: &'a ScopeData,
    /// Running-sum values of this band instance, by component id.
    pub running: Option<&'a HashMap<String, RunningValue>>,
}

impl RenderContext<'_> {
    pub fn now(&self) -> Option<DateTime<Utc>> {
        self.env.now
    }
    pub fn diagnose(&self, component_id: &str, message: &str) {
        self.env.diagnostics.add(component_id, message);
    }
}

/// Evaluates `expression` and formats the result; errors become `#Error` plus a diagnostic.
pub fn expression_text(
    component_id: &str,
    expression: &str,
    format: Option<&str>,
    ctx: &RenderContext,
    overrides: &[(&str, Value)],
) -> String {
    if expression.trim().is_empty() {
        return String::new();
    }
    let result = ctx
        .scope
        .eval(expression, overrides, ctx.now())
        .map_err(|e| e.message)
        .and_then(|v| expr::format_value(&v, format.filter(|f| !f.is_empty())).map_err(|e| e.0));
    result.unwrap_or_else(|message| {
        ctx.diagnose(component_id, &message);
        ERROR_TEXT.to_string()
    })
}

/// A positioned box: a component, or a table cell.
#[derive(Debug, Clone)]
pub struct BoxSpec {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub style: Style,
}

impl BoxSpec {
    pub fn of(c: &Comp) -> BoxSpec {
        BoxSpec {
            id: c.id.clone(),
            x: c.x,
            y: c.y,
            w: c.w,
            h: c.h,
            style: c.style.clone(),
        }
    }
}

/// Optional background/border rectangle for a box.
pub fn frame(b: &BoxSpec, ox: f64, oy: f64) -> Vec<Item> {
    let line_width = b.style.border_width.unwrap_or(0.0);
    let fill = b.style.fill;
    if line_width == 0.0 && fill.is_none() {
        return vec![];
    }
    vec![Item::Rect(RectItem {
        component_id: b.id.clone(),
        x: r2(ox + b.x),
        y: r2(oy + b.y),
        w: r2(b.w),
        h: r2(b.h),
        line_width,
        gray: b.style.gray.unwrap_or(0.0),
        fill,
    })]
}

#[derive(Default, Clone, Copy)]
pub struct TextOptions {
    pub pad_x: Option<f64>,
    pub middle: bool,
    pub bold: Option<bool>,
    pub font_size: Option<f64>,
}

/// A text item: wrapped with the fixed metrics, clipped to the lines that fit the box (at
/// least one), aligned horizontally, and centered vertically when `middle` is set.
pub fn text_item(b: &BoxSpec, raw: &str, ox: f64, oy: f64, options: TextOptions) -> TextItem {
    let font_size = options
        .font_size
        .or(b.style.font_size)
        .unwrap_or(DEFAULT_FONT_SIZE);
    let bold = options.bold.or(b.style.bold).unwrap_or(false);
    let pad_x = options.pad_x.unwrap_or(TEXT_PAD);
    let align = b.style.align.as_deref().unwrap_or("left");
    let line_height = font_size * LINE_HEIGHT;
    let text = normalize_text(raw);
    let all = if text.is_empty() {
        vec![]
    } else {
        wrap_text(&text, (b.w - 2.0 * pad_x).max(1.0), font_size, bold)
    };
    let fit = (((b.h + 1e-6) / line_height).floor().max(0.0) as usize).max(1);
    let shown: Vec<String> = all.into_iter().take(fit).collect();
    let x = ox + b.x;
    let top = oy
        + b.y
        + if options.middle {
            (b.h - shown.len() as f64 * line_height) / 2.0
        } else {
            0.0
        };
    let lines = shown
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let width = measure_text(line, font_size, bold);
            let lx = match align {
                "center" => x + (b.w - width) / 2.0,
                "right" => x + b.w - pad_x - width,
                _ => x + pad_x,
            };
            TextLine {
                text: line.clone(),
                x: r2(lx),
                y: r2(top + i as f64 * line_height + font_size * BASELINE),
                width: r2(width),
            }
        })
        .collect();
    TextItem {
        component_id: b.id.clone(),
        x: r2(x),
        y: r2(oy + b.y),
        w: r2(b.w),
        h: r2(b.h),
        text,
        lines,
        font_size,
        bold,
        gray: b.style.gray.unwrap_or(0.0),
    }
}

/// Height a text box needs to show all of `raw` at its style, in points.
pub fn text_height(b: &BoxSpec, raw: &str) -> f64 {
    let font_size = b.style.font_size.unwrap_or(DEFAULT_FONT_SIZE);
    let text = normalize_text(raw);
    let width = (b.w - 2.0 * TEXT_PAD).max(1.0);
    let lines = if text.is_empty() {
        1
    } else {
        wrap_text(&text, width, font_size, b.style.bold.unwrap_or(false)).len()
    };
    lines as f64 * font_size * LINE_HEIGHT
}

fn placeholder(b: &BoxSpec, label: &str, ox: f64, oy: f64) -> Vec<Item> {
    let cell = BoxSpec {
        style: Style {
            font_size: Some(8.0),
            align: Some("center".into()),
            gray: Some(0.4),
            ..Style::default()
        },
        ..b.clone()
    };
    vec![
        Item::Rect(RectItem {
            component_id: b.id.clone(),
            x: r2(ox + b.x),
            y: r2(oy + b.y),
            w: r2(b.w),
            h: r2(b.h),
            line_width: CELL_LINE,
            gray: 0.5,
            fill: None,
        }),
        Item::Text(text_item(
            &cell,
            label,
            ox,
            oy,
            TextOptions {
                middle: true,
                ..TextOptions::default()
            },
        )),
    ]
}

/// Items for one component other than a table or subreport, placed with its band origin
/// at (ox, oy).
pub fn component_items(c: &Comp, ox: f64, oy: f64, ctx: &RenderContext) -> Vec<Item> {
    match &c.kind {
        Kind::StaticText { .. } | Kind::Field { .. } | Kind::Calculated { .. } => {
            let (component, text) = styled_component(c, ctx);
            let b = BoxSpec::of(&component);
            let mut items = frame(&b, ox, oy);
            items.push(Item::Text(text_item(&b, &text, ox, oy, TextOptions::default())));
            items
        }
        Kind::Chart(spec) => {
            let mut items = frame(&BoxSpec::of(c), ox, oy);
            items.extend(chart_items(c, spec, ox, oy, ctx));
            items
        }
        Kind::Line { vertical } => {
            let line_width = c.style.border_width.unwrap_or(1.0);
            if line_width <= 0.0 {
                return vec![];
            }
            let horizontal = !vertical;
            vec![Item::Line(LineItem {
                component_id: c.id.clone(),
                x: r2(ox + c.x + if horizontal { 0.0 } else { c.w / 2.0 }),
                y: r2(oy + c.y + if horizontal { c.h / 2.0 } else { 0.0 }),
                w: if horizontal { r2(c.w) } else { 0.0 },
                h: if horizontal { 0.0 } else { r2(c.h) },
                line_width,
                gray: c.style.gray.unwrap_or(0.0),
            })]
        }
        Kind::Rectangle => vec![Item::Rect(RectItem {
            component_id: c.id.clone(),
            x: r2(ox + c.x),
            y: r2(oy + c.y),
            w: r2(c.w),
            h: r2(c.h),
            line_width: c.style.border_width.unwrap_or(1.0),
            gray: c.style.gray.unwrap_or(0.0),
            fill: c.style.fill,
        })],
        Kind::Image { asset_id } => {
            let b = BoxSpec::of(c);
            if asset_id.is_empty() {
                return placeholder(&b, "Image", ox, oy);
            }
            let Some(asset) = ctx.env.assets.get(asset_id) else {
                ctx.diagnose(&c.id, &format!("Image asset {asset_id} not found"));
                return placeholder(&b, "Missing image", ox, oy);
            };
            if asset.media_type != "image/jpeg" && asset.media_type != "image/png" {
                return placeholder(&b, "Unsupported image", ox, oy);
            }
            let mut items = vec![Item::Image(super::ImageItem {
                component_id: c.id.clone(),
                x: r2(ox + c.x),
                y: r2(oy + c.y),
                w: r2(c.w),
                h: r2(c.h),
                asset_id: asset_id.clone(),
                media_type: asset.media_type.clone(),
            })];
            let unfilled = BoxSpec {
                style: Style {
                    fill: None,
                    ..b.style.clone()
                },
                ..b
            };
            items.extend(frame(&unfilled, ox, oy));
            items
        }
        Kind::Table { .. } | Kind::Subreport { .. } => vec![],
    }
}

/// Measured table: column positions, cell texts, and row heights.
#[derive(Debug, Clone)]
pub struct TableGeometry {
    pub columns: Vec<(f64, f64)>,
    pub header: Vec<String>,
    pub cells: Vec<Vec<String>>,
    pub header_height: f64,
    pub row_heights: Vec<f64>,
    pub height: f64,
}

/// Evaluates and measures every cell of a table component.
pub fn measure_table(
    t: &Comp,
    query_id: Option<&str>,
    table_columns: &[TableColumn],
    ctx: &RenderContext,
) -> TableGeometry {
    let font_size = t.style.font_size.unwrap_or(TABLE_FONT_SIZE);
    let line_height = font_size * LINE_HEIGHT;
    let rows: Vec<Row> = match query_id {
        Some(q) => match ctx.env.tables.get(q) {
            Some(rows) => rows.clone(),
            None => {
                ctx.diagnose(&t.id, "Table query data is not loaded");
                vec![]
            }
        },
        None => ctx.scope.rows.as_ref().clone(),
    };
    let total: f64 = table_columns.iter().map(|c| c.width.max(0.0)).sum();
    let total = if total == 0.0 { 1.0 } else { total };
    let mut cx = 0.0;
    let columns: Vec<(f64, f64)> = table_columns
        .iter()
        .map(|col| {
            let w = col.width.max(0.0) / total * t.w;
            let out = (cx, w);
            cx += w;
            out
        })
        .collect();
    let lines_in = |text: &str, i: usize, bold: bool| -> usize {
        if text.is_empty() {
            1
        } else {
            wrap_text(text, (columns[i].1 - 2.0 * CELL_PAD_X).max(1.0), font_size, bold).len()
        }
    };
    let height_of = |texts: &[String], bold: bool| -> f64 {
        let lines = texts
            .iter()
            .enumerate()
            .map(|(i, s)| lines_in(s, i, bold))
            .max()
            .unwrap_or(1)
            .max(1);
        lines as f64 * line_height + 2.0 * CELL_PAD_Y
    };
    let header: Vec<String> = table_columns.iter().map(|c| normalize_text(&c.header)).collect();
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|record| {
            table_columns
                .iter()
                .map(|col| {
                    expression_text(
                        &t.id,
                        &col.expression,
                        col.format.as_deref(),
                        ctx,
                        &[("record", Value::Object(record.clone()))],
                    )
                })
                .collect()
        })
        .collect();
    let header_height = if table_columns.is_empty() {
        0.0
    } else {
        height_of(&header, true)
    };
    let row_heights: Vec<f64> = if table_columns.is_empty() {
        vec![]
    } else {
        cells.iter().map(|row| height_of(row, false)).collect()
    };
    let height = header_height + row_heights.iter().sum::<f64>();
    TableGeometry {
        columns,
        header,
        cells,
        header_height,
        row_heights,
        height,
    }
}

/// Items for one table row (`index` None is the header) whose top edge is at `top`.
pub fn table_row_items(
    t: &Comp,
    columns: &[TableColumn],
    geo: &TableGeometry,
    index: Option<usize>,
    left: f64,
    top: f64,
) -> Vec<Item> {
    let header = index.is_none();
    let (texts, h) = match index {
        None => (&geo.header, geo.header_height),
        Some(i) => (&geo.cells[i], geo.row_heights[i]),
    };
    let font_size = t.style.font_size.unwrap_or(TABLE_FONT_SIZE);
    let mut out = vec![];
    for (i, col) in columns.iter().enumerate() {
        let align = col
            .align
            .clone()
            .or_else(|| if header { Some("left".to_string()) } else { t.style.align.clone() });
        let cell = BoxSpec {
            id: t.id.clone(),
            x: geo.columns[i].0,
            y: 0.0,
            w: geo.columns[i].1,
            h,
            style: Style {
                align,
                gray: t.style.gray,
                ..Style::default()
            },
        };
        out.push(Item::Rect(RectItem {
            component_id: t.id.clone(),
            x: r2(left + cell.x),
            y: r2(top),
            w: r2(cell.w),
            h: r2(h),
            line_width: t.style.border_width.unwrap_or(CELL_LINE),
            gray: t.style.gray.unwrap_or(0.0),
            fill: if header { Some(HEADER_FILL) } else { None },
        }));
        out.push(Item::Text(text_item(
            &cell,
            &texts[i],
            left,
            top,
            TextOptions {
                pad_x: Some(CELL_PAD_X),
                middle: true,
                bold: Some(header),
                font_size: Some(font_size),
            },
        )));
    }
    out
}
