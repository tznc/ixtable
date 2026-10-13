//! Report charts (PRD §15): chart data and geometry become one `chart` item of paths and
//! text marks in page points, so preview, print and PDF draw the same chart.

use super::chart_geometry::{
    bar_geometry, chart_data, format_number, line_geometry, pie_categories, pie_geometry, plot_area,
    scatter_data, scatter_geometry, series_color, CategoryData, Frame, Margin, Plot, ScatterSeries,
    SeriesSpec, Ticks, AXIS, GRID, MAX_SERIES, OTHER_COLOR, OTHER_LABEL,
};
use super::chart_paths::{circle_path, line_path, offset_path, rect_path, tint, wedge_path};
use super::component::{ChartSpec, Comp, Style};
use super::render::{text_item, BoxSpec, RenderContext, TextOptions};
use super::text::{measure_text, LINE_HEIGHT};
use super::{r2, ChartItem, Item, Mark, PathItem, PathOp, Row};
use serde_json::Value;

pub const CHART_NO_DATA: &str = "No data";
pub const CHART_QUERY_NOT_LOADED: &str = "Chart query data is not loaded";

const LABEL_SIZE: f64 = 7.0;
const TITLE_SIZE: f64 = 9.0;
const LABEL_LINE: f64 = LABEL_SIZE * LINE_HEIGHT;
const TITLE_HEIGHT: f64 = TITLE_SIZE * LINE_HEIGHT + 4.0;
const LEGEND_ROW: f64 = LABEL_LINE + 3.0;
const SWATCH: f64 = 7.0;
/// Label and axis text gray, close to the dashboard's muted ink.
const LABEL_GRAY: f64 = 0.36;
const TITLE_GRAY: f64 = 0.18;

fn type_name(kind: &str) -> &'static str {
    match kind {
        "bar" => "Bar chart",
        "line" => "Line chart",
        "area" => "Area chart",
        "pie" => "Pie chart",
        "donut" => "Donut chart",
        "scatter" => "Scatter plot",
        _ => "Chart",
    }
}

type Legend = Vec<(String, String)>;

/// Collects marks for one chart in page coordinates.
struct Marks {
    id: String,
    ox: f64,
    oy: f64,
    marks: Vec<Mark>,
}

#[derive(Default, Clone, Copy)]
struct Label {
    align: Option<&'static str>,
    size: Option<f64>,
    bold: bool,
    gray: Option<f64>,
}

impl Marks {
    fn new(id: &str, ox: f64, oy: f64) -> Self {
        Self {
            id: id.into(),
            ox,
            oy,
            marks: vec![],
        }
    }

    fn path(&mut self, ops: Vec<PathOp>, fill: Option<&str>, stroke: Option<&str>, line_width: f64) {
        if !ops.is_empty() {
            self.marks.push(Mark::Path(PathItem {
                ops,
                fill: fill.map(String::from),
                stroke: stroke.map(String::from),
                line_width,
            }));
        }
    }

    /// One line of text in a box at chart-relative (x, y), clipped to the box width.
    fn text(&mut self, raw: &str, x: f64, y: f64, w: f64, label: Label) {
        let size = label.size.unwrap_or(LABEL_SIZE);
        let spec = BoxSpec {
            id: self.id.clone(),
            x,
            y,
            w: w.max(1.0),
            h: size * LINE_HEIGHT,
            style: Style {
                font_size: Some(size),
                bold: Some(label.bold),
                align: Some(label.align.unwrap_or("left").into()),
                gray: Some(label.gray.unwrap_or(LABEL_GRAY)),
                ..Style::default()
            },
        };
        let item = text_item(
            &spec,
            raw,
            self.ox,
            self.oy,
            TextOptions {
                pad_x: Some(0.0),
                ..TextOptions::default()
            },
        );
        if !item.lines.is_empty() {
            self.marks.push(Mark::Text(item));
        }
    }
}

/// Legend rows: swatch and name entries flowing left to right, wrapping at `width`.
fn legend_rows(legend: &Legend, width: f64) -> Vec<Legend> {
    let mut rows: Vec<Legend> = vec![];
    let mut used = f64::INFINITY;
    for entry in legend {
        let w = SWATCH + 3.0 + measure_text(&entry.0, LABEL_SIZE, false) + 10.0;
        if used + w > width && rows.len() < 3 {
            rows.push(vec![]);
            used = 0.0;
        }
        if used + w > width && !rows.last().map_or(true, Vec::is_empty) {
            continue;
        }
        if let Some(row) = rows.last_mut() {
            row.push(entry.clone());
        }
        used += w;
    }
    rows
}

fn draw_legend(m: &mut Marks, rows: &[Legend], top: f64, width: f64) {
    for (r, row) in rows.iter().enumerate() {
        let widths: Vec<f64> = row
            .iter()
            .map(|e| SWATCH + 3.0 + measure_text(&e.0, LABEL_SIZE, false))
            .collect();
        let total: f64 = widths.iter().sum::<f64>() + 10.0 * (row.len() as f64 - 1.0);
        let mut x = ((width - total) / 2.0).max(0.0);
        let y = top + r as f64 * LEGEND_ROW;
        for (i, entry) in row.iter().enumerate() {
            m.path(rect_path(m.ox + x, m.oy + y + 1.0, SWATCH, SWATCH), Some(&entry.1), None, 0.0);
            m.text(&entry.0, x + SWATCH + 3.0, y, widths[i] - SWATCH - 3.0 + 1.0, Label::default());
            x += widths[i] + 10.0;
        }
    }
}

/// Gridlines and labels for a value axis on the left of the plot.
fn value_axis(m: &mut Marks, ticks: &Ticks, plot: &Plot, format: Option<&str>) {
    let y = super::chart_geometry::linear_scale((ticks.min, ticks.max), (plot.bottom, plot.top));
    for tick in &ticks.ticks {
        let ty = y(*tick);
        m.path(
            line_path(m.ox + plot.left, m.oy + ty, m.ox + plot.right, m.oy + ty),
            None,
            Some(GRID),
            0.5,
        );
        m.text(
            &format_number(&number_value(*tick), format),
            0.0,
            ty - LABEL_LINE / 2.0,
            plot.left - 4.0,
            Label {
                align: Some("right"),
                ..Label::default()
            },
        );
    }
}

fn number_value(n: f64) -> Value {
    super::values::num_value(n)
}

/// Category labels under the plot, thinned so they don't overlap.
fn category_axis(m: &mut Marks, labels: &[(String, f64)], plot: &Plot) {
    if labels.is_empty() {
        return;
    }
    let band = (plot.right - plot.left) / labels.len() as f64;
    let widest = labels
        .iter()
        .map(|l| measure_text(&l.0, LABEL_SIZE, false))
        .fold(f64::NEG_INFINITY, f64::max);
    let step = (((widest + 4.0) / band.max(1.0)).ceil().max(1.0)) as usize;
    for (i, label) in labels.iter().enumerate() {
        if i % step != 0 {
            continue;
        }
        let w = band * step as f64;
        m.text(
            &label.0,
            label.1 - w / 2.0,
            plot.bottom + 3.0,
            w,
            Label {
                align: Some("center"),
                ..Label::default()
            },
        );
    }
}

fn value_ticks_width(ticks: &Ticks, format: Option<&str>) -> f64 {
    ticks
        .ticks
        .iter()
        .map(|t| measure_text(&format_number(&number_value(*t), format), LABEL_SIZE, false))
        .fold(f64::NEG_INFINITY, f64::max)
}

fn frame_for(w: f64, h: f64, left: f64) -> Frame {
    Frame {
        width: w,
        height: h,
        margin: Margin {
            top: LABEL_LINE / 2.0 + 2.0,
            right: 8.0,
            bottom: LABEL_LINE + 5.0,
            left,
        },
    }
}

fn has_values(data: &CategoryData) -> bool {
    !data.categories.is_empty()
        && data
            .series
            .iter()
            .any(|s| s.values.iter().any(|v| v.map_or(false, f64::is_finite)))
}

fn category_chart(m: &mut Marks, c: &ChartSpec, data: &CategoryData, w: f64, h: f64, top: f64) {
    let kind = c.chart_type.as_str();
    let stacked = c.stacked;
    let format = c.format.as_deref();
    let ticks_of = |frame: &Frame| {
        if kind == "bar" {
            bar_geometry(data, stacked, frame).ticks
        } else {
            line_geometry(data, kind == "area", stacked, frame).ticks
        }
    };
    let first = ticks_of(&frame_for(w, h, 40.0));
    let frame = frame_for(w, h, (w / 3.0).min(value_ticks_width(&first, format) + 6.0));
    let plot = plot_area(&frame);
    let dy = m.oy + top;
    let mut shifted = Marks::new(&m.id, m.ox, dy);
    value_axis(&mut shifted, &first, &plot, format);
    let color = |s: usize| series_color(s, data.series.get(s).map(|s| s.name.as_str()));
    if kind == "bar" {
        let g = bar_geometry(data, stacked, &frame);
        for bar in &g.bars {
            shifted.path(rect_path(m.ox + bar.x, dy + bar.y, bar.width, bar.height), Some(color(bar.series)), None, 0.0);
        }
        shifted.path(
            line_path(m.ox + plot.left, dy + g.zero_y, m.ox + plot.right, dy + g.zero_y),
            None,
            Some(AXIS),
            0.75,
        );
        let labels: Vec<(String, f64)> = g.x_labels.iter().map(|l| (l.text.clone(), l.x)).collect();
        category_axis(&mut shifted, &labels, &plot);
    } else {
        let g = line_geometry(data, kind == "area", stacked, &frame);
        for (i, s) in g.series.iter().enumerate() {
            if let Some(area) = &s.area {
                shifted.path(offset_path(area, m.ox, dy), Some(&tint(color(i), 0.6)), None, 0.0);
            }
        }
        for (i, s) in g.series.iter().enumerate() {
            shifted.path(offset_path(&s.path, m.ox, dy), None, Some(color(i)), 1.5);
            if kind == "line" {
                for p in &s.points {
                    shifted.path(circle_path(m.ox + p.x, dy + p.y, 1.75), Some(color(i)), None, 0.0);
                }
            }
        }
        shifted.path(
            line_path(m.ox + plot.left, dy + g.zero_y, m.ox + plot.right, dy + g.zero_y),
            None,
            Some(AXIS),
            0.75,
        );
        let labels: Vec<(String, f64)> = g.x_labels.iter().map(|l| (l.text.clone(), l.x)).collect();
        category_axis(&mut shifted, &labels, &plot);
    }
    m.marks.append(&mut shifted.marks);
}

fn scatter_chart(
    m: &mut Marks,
    c: &ChartSpec,
    data: &[ScatterSeries],
    w: f64,
    h: f64,
    top: f64,
) -> Option<Legend> {
    if !data.iter().any(|s| !s.points.is_empty()) {
        return None;
    }
    let format = c.format.as_deref();
    let first = scatter_geometry(data, &frame_for(w, h, 40.0));
    let frame = frame_for(w, h, (w / 3.0).min(value_ticks_width(&first.y_ticks, format) + 6.0));
    let plot = plot_area(&frame);
    let g = scatter_geometry(data, &frame);
    let dy = m.oy + top;
    let mut shifted = Marks::new(&m.id, m.ox, dy);
    value_axis(&mut shifted, &g.y_ticks, &plot, format);
    let sx = super::chart_geometry::linear_scale((g.x_ticks.min, g.x_ticks.max), (plot.left, plot.right));
    let labels: Vec<(String, f64)> = g
        .x_ticks
        .ticks
        .iter()
        .map(|t| (format_number(&number_value(*t), None), sx(*t)))
        .collect();
    category_axis(&mut shifted, &labels, &plot);
    shifted.path(
        line_path(m.ox + plot.left, dy + plot.bottom, m.ox + plot.right, dy + plot.bottom),
        None,
        Some(AXIS),
        0.75,
    );
    for (i, points) in g.series.iter().enumerate() {
        for p in points {
            shifted.path(
                circle_path(m.ox + p.cx, dy + p.cy, 2.25),
                Some(series_color(i, Some(&data[i].name))),
                None,
                0.0,
            );
        }
    }
    m.marks.append(&mut shifted.marks);
    Some(
        data.iter()
            .enumerate()
            .map(|(i, s)| (s.name.clone(), series_color(i, Some(&s.name)).to_string()))
            .collect(),
    )
}

/// Pie or donut on the left, legend with shares on the right.
fn pie_chart(m: &mut Marks, c: &ChartSpec, data: &CategoryData, w: f64, h: f64, top: f64) -> bool {
    let (labels, values) = pie_categories(data);
    let slices = pie_geometry(&labels, &values);
    if slices.is_empty() {
        return false;
    }
    let radius = (h.min(w / 2.0) / 2.0 - 4.0).max(4.0);
    let cx = m.ox + radius + 4.0;
    let cy = m.oy + top + h / 2.0;
    let inner = if c.chart_type == "donut" { radius * 0.58 } else { 0.0 };
    let color = |index: usize, label: &str| {
        if label == OTHER_LABEL && index == MAX_SERIES - 1 {
            OTHER_COLOR
        } else {
            series_color(index, None)
        }
    };
    let mut angle = -std::f64::consts::FRAC_PI_2;
    for s in &slices {
        let end = angle + s.fraction * 2.0 * std::f64::consts::PI;
        m.path(
            wedge_path(cx, cy, radius, inner, angle, end),
            Some(color(s.index, &s.label)),
            Some("#ffffff"),
            if slices.len() > 1 { 0.75 } else { 0.0 },
        );
        angle = end;
    }
    let lx = 2.0 * radius + 16.0;
    let rows = ((h / LEGEND_ROW).floor() as usize).max(1);
    let shown: Vec<_> = slices.iter().take(rows).collect();
    let ly = top + (h - shown.len() as f64 * LEGEND_ROW) / 2.0;
    for (i, s) in shown.iter().enumerate() {
        let y = ly + i as f64 * LEGEND_ROW;
        m.path(rect_path(m.ox + lx, m.oy + y + 1.0, SWATCH, SWATCH), Some(color(s.index, &s.label)), None, 0.0);
        m.text(
            &format!("{} ({}%)", s.label, super::js_round(s.fraction * 100.0)),
            lx + SWATCH + 3.0,
            y,
            w - lx - SWATCH - 3.0,
            Label::default(),
        );
    }
    true
}

/// Rows a chart draws: its saved query's rows, else the band's `rows`.
fn chart_rows(c: &Comp, spec: &ChartSpec, ctx: &RenderContext) -> Vec<Row> {
    match &spec.query_id {
        Some(q) => match ctx.env.tables.get(q) {
            Some(rows) => rows.clone(),
            None => {
                ctx.diagnose(&c.id, CHART_QUERY_NOT_LOADED);
                vec![]
            }
        },
        None => ctx.scope.rows.as_ref().clone(),
    }
}

/// The chart item of one chart component placed with its band origin at (ox, oy).
pub fn chart_items(c: &Comp, spec: &ChartSpec, ox: f64, oy: f64, ctx: &RenderContext) -> Vec<Item> {
    let rows = chart_rows(c, spec, ctx);
    vec![Item::Chart(chart_item(&c.id, (ox + c.x, oy + c.y), (c.w, c.h), spec, &rows))]
}

/// One chart drawn from `rows` into a `w` x `h` box at `origin` (page coordinates). Reports
/// and dashboards share it; `spec.query_id` is not read here.
pub fn chart_item(id: &str, origin: (f64, f64), size: (f64, f64), spec: &ChartSpec, rows: &[Row]) -> ChartItem {
    let (w, h) = size;
    let mut m = Marks::new(id, origin.0, origin.1);
    let series_spec = SeriesSpec {
        x: &spec.x_field,
        y: &spec.y_fields,
        group_by: spec.group_by.as_deref(),
    };
    let title_text = spec.title.as_deref().map_or("", str::trim).to_string();
    let mut top = 0.0;
    if !title_text.is_empty() {
        m.text(
            &title_text,
            0.0,
            2.0,
            w,
            Label {
                align: Some("center"),
                size: Some(TITLE_SIZE),
                bold: true,
                gray: Some(TITLE_GRAY),
            },
        );
        top = TITLE_HEIGHT;
    }
    let kind = spec.chart_type.as_str();
    let mut drawn = false;
    if kind == "scatter" {
        let data = scatter_data(&series_spec, rows);
        let legend: Legend = data
            .iter()
            .enumerate()
            .map(|(i, s)| (s.name.clone(), series_color(i, Some(&s.name)).to_string()))
            .collect();
        let rows_of = if legend.len() > 1 { legend_rows(&legend, w) } else { vec![] };
        let plot_h = h - top - rows_of.len() as f64 * LEGEND_ROW;
        drawn = scatter_chart(&mut m, spec, &data, w, plot_h, top).is_some();
        if drawn {
            draw_legend(&mut m, &rows_of, top + plot_h, w);
        }
    } else {
        let data = chart_data(&series_spec, rows);
        if has_values(&data) {
            drawn = true;
            if kind == "pie" || kind == "donut" {
                drawn = pie_chart(&mut m, spec, &data, w, h - top, top);
            } else {
                let legend: Legend = data
                    .series
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (s.name.clone(), series_color(i, Some(&s.name)).to_string()))
                    .collect();
                let rows_of = if legend.len() > 1 { legend_rows(&legend, w) } else { vec![] };
                let plot_h = h - top - rows_of.len() as f64 * LEGEND_ROW;
                category_chart(&mut m, spec, &data, w, plot_h, top);
                draw_legend(&mut m, &rows_of, top + plot_h, w);
            }
        }
    }
    if !drawn {
        m.text(
            CHART_NO_DATA,
            0.0,
            top + (h - top - LABEL_LINE) / 2.0,
            w,
            Label {
                align: Some("center"),
                ..Label::default()
            },
        );
    }
    let title = if title_text.is_empty() {
        type_name(kind).to_string()
    } else {
        title_text
    };
    ChartItem {
        component_id: id.to_string(),
        x: r2(m.ox),
        y: r2(m.oy),
        w: r2(w),
        h: r2(h),
        title,
        marks: m.marks,
    }
}
