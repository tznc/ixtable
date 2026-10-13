//! Chart data and geometry for report charts: category and scatter series from rows,
//! nice axis ticks, bar rectangles, line and area paths, pie slices and scatter points.
//! Pure and deterministic (coordinates rounded to 0.01). A port of the dashboard chart
//! helpers (`src/dashboards/data.ts`, `src/dashboards/charts/`) that reports use.

use super::{PathOp, Row};
use serde_json::Value;

// ---- palette --------------------------------------------------------------------

/// Categorical series colors, assigned in this fixed order and never cycled (a ninth
/// series folds into "Other").
pub const SERIES_COLORS: [&str; 8] = [
    "#2a78d6", "#c8531f", "#0b9466", "#9a6600", "#c2406f", "#008300", "#4a3aa7", "#d03b3b",
];
/// Neutral for the folded "Other" series.
pub const OTHER_COLOR: &str = "#6b7079";
pub const OTHER_LABEL: &str = "Other";
pub const MAX_SERIES: usize = SERIES_COLORS.len();
/// Chart chrome: gridlines and the axis baseline.
pub const GRID: &str = "#e4e6e9";
pub const AXIS: &str = "#b9bec6";

pub fn series_color(index: usize, name: Option<&str>) -> &'static str {
    if name == Some(OTHER_LABEL) && index == MAX_SERIES - 1 {
        OTHER_COLOR
    } else {
        SERIES_COLORS[index.min(MAX_SERIES - 1)]
    }
}

// ---- scales ---------------------------------------------------------------------

/// JavaScript's `Number.prototype.toFixed` as a number: the value with `digits` decimals,
/// exact ties rounding up (away from zero for negatives).
pub fn to_fixed(x: f64, digits: usize) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let negative = x < 0.0;
    // Exact decimal expansion, long enough to show every tie.
    let text = format!("{:.*}", digits + 40, x.abs());
    let (whole, frac) = text.split_once('.').unwrap_or((&text, ""));
    let mut digits_vec: Vec<u8> = whole.bytes().chain(frac.bytes().take(digits)).map(|b| b - b'0').collect();
    let next = frac.as_bytes().get(digits).map_or(0, |b| b - b'0');
    if next >= 5 {
        let mut i = digits_vec.len();
        loop {
            if i == 0 {
                digits_vec.insert(0, 1);
                break;
            }
            i -= 1;
            if digits_vec[i] == 9 {
                digits_vec[i] = 0;
            } else {
                digits_vec[i] += 1;
                break;
            }
        }
    }
    let split = digits_vec.len() - digits;
    let mut out = String::new();
    for (i, d) in digits_vec.iter().enumerate() {
        if i == split {
            out.push('.');
        }
        out.push((b'0' + d) as char);
    }
    let value: f64 = out.parse().unwrap_or(0.0);
    if negative && value != 0.0 {
        -value
    } else {
        0.0 + value
    }
}

/// Rounds to 2 decimals for output, so paths are short and deterministic.
pub fn chart_r2(value: f64) -> f64 {
    to_fixed(value, 2)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ticks {
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub ticks: Vec<f64>,
}

/// Decimal places needed to print multiples of `step` exactly.
pub fn step_decimals(step: f64) -> usize {
    if step >= 1.0 || step <= 0.0 {
        0
    } else {
        (-(step.log10() + 1e-9).floor()).max(0.0) as usize
    }
}

/// The 1, 2, 5 × 10^k step closest to splitting `span` into `count` intervals.
pub fn nice_step(span: f64, count: f64) -> f64 {
    let raw = span / count.round().max(1.0);
    if !(raw > 0.0) || !raw.is_finite() {
        return 1.0;
    }
    let magnitude = 10f64.powf(raw.log10().floor());
    // Tolerance absorbs float noise such as (1.04 - 1.01) / 3 = 0.010000000000000009.
    let normalized = raw / magnitude - 1e-9;
    let nice = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * magnitude
}

/// Round axis bounds and evenly spaced ticks covering [min, max] with about `count`
/// intervals. A zero-width range is widened to include zero (or [0, 1] when the value is
/// zero).
pub fn nice_ticks(min: f64, max: f64, count: f64) -> Ticks {
    let mut lo = if min.is_finite() { min } else { 0.0 };
    let mut hi = if max.is_finite() { max } else { lo };
    if lo > hi {
        std::mem::swap(&mut lo, &mut hi);
    }
    if lo == hi {
        if lo > 0.0 {
            lo = 0.0;
        } else if hi < 0.0 {
            hi = 0.0;
        } else {
            hi = 1.0;
        }
    }
    let step = nice_step(hi - lo, count);
    let decimals = step_decimals(step);
    let first = to_fixed(lo / step, 9).floor() * step;
    let last = to_fixed(hi / step, 9).ceil() * step;
    let n = ((last - first) / step).round() as usize;
    let ticks: Vec<f64> = (0..=n)
        .map(|i| {
            let v = to_fixed(first + i as f64 * step, decimals);
            if v == 0.0 {
                0.0
            } else {
                v
            }
        })
        .collect();
    Ticks {
        min: ticks[0],
        max: ticks[ticks.len() - 1],
        step,
        ticks,
    }
}

/// Maps a value in `domain` linearly onto `range`. A zero-width domain maps to the range start.
pub fn linear_scale(domain: (f64, f64), range: (f64, f64)) -> impl Fn(f64) -> f64 {
    move |value| {
        if domain.1 == domain.0 {
            range.0
        } else {
            range.0 + ((value - domain.0) / (domain.1 - domain.0)) * (range.1 - range.0)
        }
    }
}

pub struct BandScale {
    pub width: f64,
    start: f64,
}

impl BandScale {
    pub fn start(&self, index: usize) -> f64 {
        self.start + index as f64 * self.width
    }
    pub fn center(&self, index: usize) -> f64 {
        self.start + (index as f64 + 0.5) * self.width
    }
}

/// Evenly divides `[start, end]` into `count` bands.
pub fn band_scale(count: usize, range: (f64, f64)) -> BandScale {
    BandScale {
        width: if count > 0 {
            (range.1 - range.0) / count as f64
        } else {
            0.0
        },
        start: range.0,
    }
}

// ---- data -----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub name: String,
    pub values: Vec<Option<f64>>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CategoryData {
    pub categories: Vec<String>,
    pub series: Vec<Series>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScatterSeries {
    pub name: String,
    pub points: Vec<Point>,
}

/// The columns chart data reads.
pub struct SeriesSpec<'a> {
    pub x: &'a str,
    pub y: &'a [String],
    pub group_by: Option<&'a str>,
}

/// A finite number from a query value, else `None` (text that looks numeric counts).
pub fn to_number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64().filter(|v| v.is_finite()),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) if !s.trim().is_empty() => s.trim().parse::<f64>().ok().filter(|v| v.is_finite()),
        _ => None,
    }
}

/// `String(value)`.
pub fn js_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e21 => format!("{}", f as i64),
            Some(f) => format!("{f}"),
            None => n.to_string(),
        },
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".into(),
        other => other.to_string(),
    }
}

pub fn category_label(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "(blank)".into(),
        Some(Value::String(s)) if s.is_empty() => "(blank)".into(),
        Some(v) => js_string(v),
    }
}

/// Formats a value with an optional expression format pattern; bad patterns fall back to
/// plain text.
pub fn format_number(value: &Value, pattern: Option<&str>) -> String {
    if value.is_null() {
        return "—".into();
    }
    crate::expr::format_value(value, pattern.filter(|p| !p.is_empty())).unwrap_or_else(|_| js_string(value))
}

fn add(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match b {
        None => a,
        Some(b) => Some(a.unwrap_or(0.0) + b),
    }
}

/// Keeps the first `MAX_SERIES - 1` series and sums the rest into "Other".
pub fn fold_series(series: Vec<Series>) -> Vec<Series> {
    if series.len() <= MAX_SERIES {
        return series;
    }
    let mut kept = series;
    let rest = kept.split_off(MAX_SERIES - 1);
    let mut other: Vec<Option<f64>> = rest[0].values.iter().map(|_| None).collect();
    // Same as the reduce over `series.slice(MAX_SERIES - 1)` starting from all-null.
    for s in &rest {
        other = other
            .iter()
            .enumerate()
            .map(|(i, v)| add(*v, s.values.get(i).copied().flatten()))
            .collect();
    }
    kept.push(Series {
        name: OTHER_LABEL.into(),
        values: other,
    });
    kept
}

/// Category chart data. Without `group_by`, each y column is a series and rows with the
/// same x are summed. With `group_by`, the first y column is split into one series per
/// group value. Categories and series keep first-appearance order.
pub fn chart_data(spec: &SeriesSpec, rows: &[Row]) -> CategoryData {
    let ys: Vec<&String> = spec.y.iter().filter(|y| !y.is_empty()).collect();
    let mut categories: Vec<String> = vec![];
    let mut category_of = |row: &Row| -> usize {
        let label = category_label(row.get(spec.x));
        match categories.iter().position(|c| *c == label) {
            Some(i) => i,
            None => {
                categories.push(label);
                categories.len() - 1
            }
        }
    };
    if let (Some(group_by), false) = (spec.group_by.filter(|g| !g.is_empty()), ys.is_empty()) {
        let cells: Vec<(usize, String, Option<f64>)> = rows
            .iter()
            .map(|row| {
                (
                    category_of(row),
                    category_label(row.get(group_by)),
                    to_number(row.get(ys[0].as_str())),
                )
            })
            .collect();
        let mut groups: Vec<(String, Vec<Option<f64>>)> = vec![];
        for (_, group, _) in &cells {
            if !groups.iter().any(|(g, _)| g == group) {
                groups.push((group.clone(), vec![]));
            }
        }
        for (_, values) in groups.iter_mut() {
            values.extend(std::iter::repeat(None).take(categories.len()));
        }
        for (category, group, value) in cells {
            if let Some((_, values)) = groups.iter_mut().find(|(g, _)| *g == group) {
                values[category] = add(values[category], value);
            }
        }
        return CategoryData {
            categories,
            series: fold_series(
                groups
                    .into_iter()
                    .map(|(name, values)| Series { name, values })
                    .collect(),
            ),
        };
    }
    let positions: Vec<usize> = rows.iter().map(&mut category_of).collect();
    let series = ys
        .iter()
        .map(|name| {
            let mut values: Vec<Option<f64>> = vec![None; categories.len()];
            for (i, row) in rows.iter().enumerate() {
                values[positions[i]] = add(values[positions[i]], to_number(row.get(name.as_str())));
            }
            Series {
                name: (*name).clone(),
                values,
            }
        })
        .collect();
    CategoryData {
        categories,
        series: fold_series(series),
    }
}

/// Scatter data: numeric x against each y column, or the first y split by `group_by`.
pub fn scatter_data(spec: &SeriesSpec, rows: &[Row]) -> Vec<ScatterSeries> {
    let ys: Vec<&String> = spec.y.iter().filter(|y| !y.is_empty()).collect();
    let point = |row: &Row, y: &str| -> Option<Point> {
        let px = to_number(row.get(spec.x))?;
        let py = to_number(row.get(y))?;
        Some(Point {
            x: px,
            y: py,
            label: category_label(row.get(spec.x)),
        })
    };
    if let (Some(group_by), false) = (spec.group_by.filter(|g| !g.is_empty()), ys.is_empty()) {
        let mut groups: Vec<ScatterSeries> = vec![];
        for row in rows {
            let name = category_label(row.get(group_by));
            let p = point(row, ys[0]);
            let at = match groups.iter().position(|g| g.name == name) {
                Some(i) => i,
                None => {
                    groups.push(ScatterSeries {
                        name,
                        points: vec![],
                    });
                    groups.len() - 1
                }
            };
            if let Some(p) = p {
                groups[at].points.push(p);
            }
        }
        if groups.len() <= MAX_SERIES {
            return groups;
        }
        let rest = groups.split_off(MAX_SERIES - 1);
        groups.push(ScatterSeries {
            name: OTHER_LABEL.into(),
            points: rest.into_iter().flat_map(|s| s.points).collect(),
        });
        return groups;
    }
    ys.iter()
        .take(MAX_SERIES)
        .map(|name| ScatterSeries {
            name: (*name).clone(),
            points: rows.iter().filter_map(|row| point(row, name)).collect(),
        })
        .collect()
}

// ---- geometry -------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct Margin {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub width: f64,
    pub height: f64,
    pub margin: Margin,
}

#[derive(Debug, Clone, Copy)]
pub struct Plot {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

pub fn plot_area(frame: &Frame) -> Plot {
    Plot {
        left: frame.margin.left,
        right: frame.width - frame.margin.right,
        top: frame.margin.top,
        bottom: frame.height - frame.margin.bottom,
    }
}

fn finite(values: impl IntoIterator<Item = Option<f64>>) -> Vec<f64> {
    values.into_iter().flatten().filter(|v| v.is_finite()).collect()
}

#[derive(Debug, Clone)]
pub struct Bar {
    pub series: usize,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone)]
pub struct AxisLabel {
    pub text: String,
    pub x: f64,
}

pub struct BarGeometry {
    pub ticks: Ticks,
    pub zero_y: f64,
    pub bars: Vec<Bar>,
    pub x_labels: Vec<AxisLabel>,
}

fn x_labels(categories: &[String], band: &BandScale) -> Vec<AxisLabel> {
    categories
        .iter()
        .enumerate()
        .map(|(i, text)| AxisLabel {
            text: text.clone(),
            x: chart_r2(band.center(i)),
        })
        .collect()
}

/// Grouped (side by side) or stacked bars. The value axis always includes zero.
pub fn bar_geometry(data: &CategoryData, stacked: bool, frame: &Frame) -> BarGeometry {
    let area = plot_area(frame);
    let n = data.categories.len();
    let (mut lo, mut hi): (f64, f64) = (0.0, 0.0);
    if stacked {
        for c in 0..n {
            let values = finite(data.series.iter().map(|s| s.values.get(c).copied().flatten()));
            hi = hi.max(values.iter().filter(|v| **v > 0.0).sum());
            lo = lo.min(values.iter().filter(|v| **v < 0.0).sum());
        }
    } else {
        let all = finite(data.series.iter().flat_map(|s| s.values.iter().copied()));
        hi = all.iter().copied().fold(0.0, f64::max);
        lo = all.iter().copied().fold(0.0, f64::min);
    }
    let ticks = nice_ticks(lo, hi, 5.0);
    let y = linear_scale((ticks.min, ticks.max), (area.bottom, area.top));
    let band = band_scale(n, (area.left, area.right));
    let inner = band.width * 0.8;
    let series_count = data.series.len().max(1) as f64;
    let gap = if !stacked && series_count > 1.0 { 2.0 } else { 0.0 };
    let bar_width = if stacked { inner } else { inner / series_count };
    let mut bars = vec![];
    for c in 0..n {
        let (mut up, mut down) = (0.0, 0.0);
        for (s, series) in data.series.iter().enumerate() {
            let Some(value) = series.values.get(c).copied().flatten().filter(|v| v.is_finite()) else {
                continue;
            };
            let x = band.start(c) + band.width * 0.1 + if stacked { 0.0 } else { s as f64 * bar_width };
            let (mut from, mut to) = (0.0, value);
            if stacked {
                from = if value >= 0.0 { up } else { down };
                to = from + value;
                if value >= 0.0 {
                    up = to;
                } else {
                    down = to;
                }
            }
            let (y0, y1) = (y(from), y(to));
            bars.push(Bar {
                series: s,
                x: chart_r2(x),
                y: chart_r2(y0.min(y1)),
                width: chart_r2((bar_width - gap).max(1.0)),
                height: chart_r2((y1 - y0).abs()),
            });
        }
    }
    BarGeometry {
        zero_y: chart_r2(y(0.0)),
        ticks,
        bars,
        x_labels: x_labels(&data.categories, &band),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LinePoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone)]
pub struct LineSeries {
    pub points: Vec<LinePoint>,
    pub path: Vec<PathOp>,
    pub area: Option<Vec<PathOp>>,
}

pub struct LineGeometry {
    pub ticks: Ticks,
    pub zero_y: f64,
    pub series: Vec<LineSeries>,
    pub x_labels: Vec<AxisLabel>,
}

/// Joins points into a path; `None` gaps start a new subpath.
pub fn path_of(points: &[Option<LinePoint>]) -> Vec<PathOp> {
    let mut out = vec![];
    let mut pen = false;
    for p in points {
        match p {
            None => pen = false,
            Some(p) => {
                let (x, y) = (chart_r2(p.x), chart_r2(p.y));
                out.push(if pen { PathOp::L(x, y) } else { PathOp::M(x, y) });
                pen = true;
            }
        }
    }
    out
}

/// Line or area series at category centers. Areas fill down to zero (or to the series
/// below when stacked); a null value breaks the line and the fill.
pub fn line_geometry(data: &CategoryData, area: bool, stacked: bool, frame: &Frame) -> LineGeometry {
    let plot = plot_area(frame);
    let n = data.categories.len();
    let stack = stacked && area;
    let mut tops: Vec<Vec<Option<f64>>> = vec![];
    let mut bottoms: Vec<Vec<f64>> = vec![];
    let mut running = vec![0.0; n];
    for series in &data.series {
        if stack {
            bottoms.push(running.clone());
            tops.push(
                series
                    .values
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        running[i] += v.unwrap_or(0.0);
                        Some(running[i])
                    })
                    .collect(),
            );
        } else {
            bottoms.push(series.values.iter().map(|_| 0.0).collect());
            tops.push(series.values.clone());
        }
    }
    let values = finite(tops.iter().flat_map(|t| t.iter().copied()));
    let lo = values.iter().copied().fold(if area { 0.0 } else { f64::INFINITY }, f64::min);
    let hi = values.iter().copied().fold(if area { 0.0 } else { f64::NEG_INFINITY }, f64::max);
    let ticks = if values.is_empty() {
        nice_ticks(0.0, 1.0, 5.0)
    } else {
        nice_ticks(lo, hi, 5.0)
    };
    let y = linear_scale((ticks.min, ticks.max), (plot.bottom, plot.top));
    let band = band_scale(n, (plot.left, plot.right));
    let series = data
        .series
        .iter()
        .enumerate()
        .map(|(si, s)| {
            let points: Vec<Option<(usize, LinePoint)>> = s
                .values
                .iter()
                .enumerate()
                .map(|(c, value)| {
                    let top = tops[si].get(c).copied().flatten();
                    match (value, top) {
                        (Some(_), Some(top)) => Some((
                            c,
                            LinePoint {
                                x: chart_r2(band.center(c)),
                                y: chart_r2(y(top)),
                            },
                        )),
                        _ => None,
                    }
                })
                .collect();
            let plain: Vec<Option<LinePoint>> = points.iter().map(|p| p.map(|(_, p)| p)).collect();
            let mut line = LineSeries {
                points: plain.iter().flatten().copied().collect(),
                path: path_of(&plain),
                area: None,
            };
            if area {
                // One closed shape per run of non-null points.
                let mut runs: Vec<Vec<(usize, LinePoint)>> = vec![];
                let mut current: Vec<(usize, LinePoint)> = vec![];
                for p in &points {
                    match p {
                        Some(p) => current.push(*p),
                        None if !current.is_empty() => runs.push(std::mem::take(&mut current)),
                        None => {}
                    }
                }
                if !current.is_empty() {
                    runs.push(current);
                }
                let mut ops = vec![];
                for run in runs {
                    let base = run.iter().rev().map(|(c, p)| {
                        Some(LinePoint {
                            x: p.x,
                            y: y(bottoms[si].get(*c).copied().unwrap_or(0.0)),
                        })
                    });
                    let shape: Vec<Option<LinePoint>> =
                        run.iter().map(|(_, p)| Some(*p)).chain(base).collect();
                    ops.extend(path_of(&shape));
                    ops.push(PathOp::Z);
                }
                line.area = Some(ops);
            }
            line
        })
        .collect();
    LineGeometry {
        zero_y: chart_r2(y(0.0f64.max(ticks.min).min(ticks.max))),
        ticks,
        series,
        x_labels: x_labels(&data.categories, &band),
    }
}

#[derive(Debug, Clone)]
pub struct Slice {
    pub index: usize,
    pub label: String,
    pub fraction: f64,
}

/// Pie or donut slices for positive values, in order.
pub fn pie_geometry(labels: &[String], values: &[Option<f64>]) -> Vec<Slice> {
    let kept: Vec<(usize, String, f64)> = values
        .iter()
        .enumerate()
        .map(|(i, v)| (i, labels.get(i).cloned().unwrap_or_default(), v.unwrap_or(0.0)))
        .filter(|(_, _, v)| v.is_finite() && *v > 0.0)
        .collect();
    let total: f64 = kept.iter().map(|(_, _, v)| *v).sum();
    kept.into_iter()
        .map(|(index, label, value)| Slice {
            index,
            label,
            fraction: value / total,
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
pub struct ScatterPoint {
    pub cx: f64,
    pub cy: f64,
}

pub struct ScatterGeometry {
    pub x_ticks: Ticks,
    pub y_ticks: Ticks,
    pub series: Vec<Vec<ScatterPoint>>,
}

/// Scatter points on nice linear x and y axes.
pub fn scatter_geometry(data: &[ScatterSeries], frame: &Frame) -> ScatterGeometry {
    let plot = plot_area(frame);
    let all: Vec<&Point> = data.iter().flat_map(|s| s.points.iter()).collect();
    let (xs, ys): (Vec<f64>, Vec<f64>) = all.iter().map(|p| (p.x, p.y)).unzip();
    let ticks_of = |v: &[f64]| {
        if v.is_empty() {
            nice_ticks(0.0, 1.0, 5.0)
        } else {
            nice_ticks(
                v.iter().copied().fold(f64::INFINITY, f64::min),
                v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                5.0,
            )
        }
    };
    let (x_ticks, y_ticks) = (ticks_of(&xs), ticks_of(&ys));
    let sx = linear_scale((x_ticks.min, x_ticks.max), (plot.left, plot.right));
    let sy = linear_scale((y_ticks.min, y_ticks.max), (plot.bottom, plot.top));
    ScatterGeometry {
        series: data
            .iter()
            .map(|s| {
                s.points
                    .iter()
                    .map(|p| ScatterPoint {
                        cx: chart_r2(sx(p.x)),
                        cy: chart_r2(sy(p.y)),
                    })
                    .collect()
            })
            .collect(),
        x_ticks,
        y_ticks,
    }
}
