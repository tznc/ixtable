//! Page chart figures (port of `src/dashboards/charts/Chart.tsx`). The marks are the
//! report engine's chart (`report_engine::chart::chart_item`), so dashboards and reports
//! draw the same bars, lines, slices and points. What the web charts exposed as an SVG
//! title and description, a legend list and a hidden data table is returned as text so the
//! view can label the chart for assistive technology.

use crate::pages::{ChartType, PageComponent};
use crate::report_engine::{
    ChartItem, PathOp, Row,
    chart::chart_item,
    chart_geometry::{
        CategoryData, LinePoint, MAX_SERIES, OTHER_COLOR, OTHER_LABEL, SeriesSpec, chart_data, chart_r2, format_number,
        linear_scale, path_of, pie_categories, pie_geometry, scatter_data, series_color,
    },
    component::ChartSpec as ReportChartSpec,
    values::num_value,
};
use serde_json::Value;

/// What a dashboard chart reads from its component.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartSpec {
    pub chart_type: ChartType,
    pub x: Option<String>,
    pub y: Vec<String>,
    pub group_by: Option<String>,
    pub stacked: bool,
    pub format: Option<String>,
}

impl From<&PageComponent> for ChartSpec {
    fn from(c: &PageComponent) -> Self {
        Self {
            chart_type: c.chart_type.unwrap_or(ChartType::Bar),
            x: c.x.clone(),
            y: c.y.clone(),
            group_by: c.group_by.clone(),
            stacked: c.stacked,
            format: c.format.clone(),
        }
    }
}

pub fn type_name(chart: ChartType) -> &'static str {
    match chart {
        ChartType::Bar => "Bar chart",
        ChartType::Line => "Line chart",
        ChartType::Area => "Area chart",
        ChartType::Pie => "Pie chart",
        ChartType::Donut => "Donut chart",
        ChartType::Scatter => "Scatter plot",
        ChartType::Summary => "Summary",
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LegendEntry {
    pub name: String,
    pub color: String,
}

/// The chart's data as the hidden table of the web chart listed it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DataTable {
    pub caption: String,
    pub head: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl DataTable {
    /// Each row's cells run together (`["monthsalescost", "Jan106"]`).
    pub fn row_texts(&self) -> Vec<String> {
        std::iter::once(&self.head).chain(self.rows.iter()).map(|r| r.concat()).collect()
    }
}

/// A summary chart: the latest value, its change and a sparkline in a 160 x 40 box.
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryBody {
    pub big: String,
    /// (up, text) such as `(false, "▼ 10 vs previous")`.
    pub delta: Option<(bool, String)>,
    pub path: Vec<PathOp>,
    pub last: Option<(f64, f64)>,
}

pub const SPARKLINE: (f64, f64) = (160.0, 40.0);

#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// No rows: `No data for <name>.`
    Empty(String),
    Marks(ChartItem),
    Summary(SummaryBody),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    pub name: String,
    pub summary: String,
    /// `<name>. <summary>`: the accessible name of the chart.
    pub label: String,
    /// Shown for two or more entries only.
    pub legend: Vec<LegendEntry>,
    pub table: DataTable,
    pub body: Body,
}

fn fmt(value: Option<f64>, spec: &ChartSpec) -> String {
    format_number(&value.map_or(Value::Null, num_value), spec.format.as_deref())
}

/// `Number(x.toPrecision(15))`.
fn precision15(x: f64) -> f64 {
    format!("{x:.14e}").parse().unwrap_or(x)
}

/// A number as JavaScript prints it in a template (`50`, `16.7`).
fn js_number(x: f64) -> String {
    if x.fract() == 0.0 { format!("{}", x as i64) } else { format!("{x}") }
}

fn percent(fraction: f64) -> i64 {
    crate::report_engine::js_round(fraction * 100.0) as i64
}

fn series_legend(data: &CategoryData) -> Vec<LegendEntry> {
    data.series.iter().enumerate().map(|(i, s)| LegendEntry { name: s.name.clone(), color: series_color(i, Some(&s.name)).into() }).collect()
}

fn category_table(name: &str, spec: &ChartSpec, data: &CategoryData) -> DataTable {
    let x = spec.x.as_deref().filter(|x| !x.is_empty()).unwrap_or("Category");
    DataTable {
        caption: format!("{name} data"),
        head: std::iter::once(x.to_string()).chain(data.series.iter().map(|s| s.name.clone())).collect(),
        rows: data
            .categories
            .iter()
            .enumerate()
            .map(|(i, c)| {
                std::iter::once(c.clone()).chain(data.series.iter().map(|s| fmt(s.values.get(i).copied().flatten(), spec))).collect()
            })
            .collect(),
    }
}

fn describe(spec: &ChartSpec, data: &CategoryData) -> String {
    let values: Vec<f64> = data.series.iter().flat_map(|s| s.values.iter().flatten().copied()).collect();
    let range = if values.is_empty() {
        String::new()
    } else {
        let lo = values.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        format!(" Values from {} to {}.", fmt(Some(lo), spec), fmt(Some(hi), spec))
    };
    let series = if data.series.len() == 1 { "1 series".to_string() } else { format!("{} series", data.series.len()) };
    format!(
        "{}{}: {} over {} {}.{}",
        type_name(spec.chart_type),
        if spec.stacked { ", stacked" } else { "" },
        series,
        data.categories.len(),
        spec.x.as_deref().unwrap_or("categories"),
        range
    )
}

/// A sparkline path through the non-null values, scaled to fill `width` x `height`.
pub fn sparkline(values: &[Option<f64>], width: f64, height: f64, pad: f64) -> (Vec<PathOp>, Option<(f64, f64)>) {
    let nums: Vec<f64> = values.iter().flatten().copied().filter(|v| v.is_finite()).collect();
    if nums.is_empty() {
        return (vec![], None);
    }
    let lo = nums.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = nums.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let sx = linear_scale((0.0, (values.len() as f64 - 1.0).max(1.0)), (pad, width - pad));
    let sy = linear_scale((lo, hi), (height - pad, pad));
    let points: Vec<Option<LinePoint>> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            v.filter(|v| v.is_finite()).map(|v| LinePoint {
                x: chart_r2(sx(i as f64)),
                y: chart_r2(if lo == hi { height / 2.0 } else { sy(v) }),
            })
        })
        .collect();
    let last = points.iter().rev().flatten().next().map(|p| (p.x, p.y));
    (path_of(&points), last)
}

/// The figure for `rows`; `size` is the box of the drawn marks in points.
pub fn figure(spec: &ChartSpec, title: &str, rows: &[Row], size: (f64, f64)) -> Figure {
    let name = if title.is_empty() { type_name(spec.chart_type).to_string() } else { title.to_string() };
    if rows.is_empty() {
        return Figure {
            summary: String::new(),
            label: name.clone(),
            legend: vec![],
            table: DataTable::default(),
            body: Body::Empty(format!("No data for {name}.")),
            name,
        };
    }
    let x = spec.x.clone().unwrap_or_default();
    let series_spec = SeriesSpec { x: &x, y: &spec.y, group_by: spec.group_by.as_deref().filter(|g| !g.is_empty()) };
    let marks = || {
        let report = ReportChartSpec {
            chart_type: super::model::chart_key(spec.chart_type).into(),
            query_id: None,
            x_field: x.clone(),
            y_fields: spec.y.clone(),
            group_by: spec.group_by.clone(),
            stacked: spec.stacked,
            format: spec.format.clone(),
            title: None,
        };
        Body::Marks(chart_item("chart", (0.0, 0.0), size, &report, rows))
    };
    let shown = |legend: Vec<LegendEntry>| if legend.len() > 1 { legend } else { vec![] };
    let (summary, legend, table, body) = match spec.chart_type {
        ChartType::Scatter => {
            let data = scatter_data(&series_spec, rows);
            let count: usize = data.iter().map(|s| s.points.len()).sum();
            let y = spec.y.join(", ");
            let summary = format!(
                "Scatter plot of {count} points: {} against {}.",
                if y.is_empty() { "y" } else { &y },
                spec.x.as_deref().unwrap_or("x")
            );
            let legend = data
                .iter()
                .enumerate()
                .map(|(i, s)| LegendEntry { name: s.name.clone(), color: series_color(i, Some(&s.name)).into() })
                .collect();
            let table = DataTable {
                caption: format!("{name} data"),
                head: vec![
                    "Series".into(),
                    spec.x.clone().filter(|x| !x.is_empty()).unwrap_or_else(|| "x".into()),
                    spec.y.first().filter(|y| !y.is_empty()).cloned().unwrap_or_else(|| "y".into()),
                ],
                rows: data
                    .iter()
                    .flat_map(|s| s.points.iter().map(|p| vec![s.name.clone(), fmt(Some(p.x), spec), fmt(Some(p.y), spec)]))
                    .collect(),
            };
            (summary, shown(legend), table, marks())
        }
        ChartType::Pie | ChartType::Donut => {
            let data = chart_data(&series_spec, rows);
            let series_name = data.series.first().map(|s| s.name.clone()).unwrap_or_default();
            let (labels, values) = pie_categories(&data);
            let slices = pie_geometry(&labels, &values);
            let value_of = |index: usize| values.get(index).copied().flatten().unwrap_or(0.0);
            let total: f64 = slices.iter().map(|s| value_of(s.index)).sum();
            let color = |index: usize, label: &str| {
                if label == OTHER_LABEL && index == MAX_SERIES - 1 { OTHER_COLOR } else { series_color(index, None) }
            };
            let summary = format!(
                "{} of {} across {} {}. Total {}.",
                type_name(spec.chart_type),
                if series_name.is_empty() { "values" } else { &series_name },
                slices.len(),
                spec.x.as_deref().unwrap_or("categories"),
                fmt(Some(total), spec)
            );
            let legend = slices
                .iter()
                .map(|s| LegendEntry { name: format!("{} ({}%)", s.label, percent(s.fraction)), color: color(s.index, &s.label).into() })
                .collect();
            let table = DataTable {
                caption: format!("{name} data"),
                head: vec![
                    spec.x.clone().filter(|x| !x.is_empty()).unwrap_or_else(|| "Category".into()),
                    if series_name.is_empty() { "Value".into() } else { series_name },
                    "Share".into(),
                ],
                rows: slices
                    .iter()
                    .map(|s| {
                        vec![
                            s.label.clone(),
                            fmt(Some(value_of(s.index)), spec),
                            format!("{}%", js_number(crate::report_engine::js_round(s.fraction * 1000.0) / 10.0)),
                        ]
                    })
                    .collect(),
            };
            (summary, legend, table, marks())
        }
        ChartType::Summary => {
            let data = chart_data(&series_spec, rows);
            let first = data.series.first().cloned().unwrap_or(crate::report_engine::chart_geometry::Series { name: String::new(), values: vec![] });
            let present: Vec<f64> = first.values.iter().flatten().copied().collect();
            let last = present.last().copied();
            let previous = present.len().checked_sub(2).and_then(|i| present.get(i)).copied();
            let delta = last.zip(previous).map(|(a, b)| precision15(a - b));
            let mut summary = format!("Latest {} {}", if first.name.is_empty() { "value" } else { &first.name }, fmt(last, spec));
            if let Some(d) = delta {
                summary += &format!(", {} {} from the previous value", if d >= 0.0 { "up" } else { "down" }, fmt(Some(d.abs()), spec));
            }
            summary.push('.');
            if !present.is_empty() {
                let lo = present.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = present.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                summary += &format!(" {} values from {} to {}.", present.len(), fmt(Some(lo), spec), fmt(Some(hi), spec));
            }
            let (path, last_point) = sparkline(&first.values, SPARKLINE.0, SPARKLINE.1, 3.0);
            let body = Body::Summary(SummaryBody {
                big: fmt(last, spec),
                delta: delta.map(|d| (d >= 0.0, format!("{} {} vs previous", if d >= 0.0 { "▲" } else { "▼" }, fmt(Some(d.abs()), spec)))),
                path,
                last: last_point,
            });
            let table = category_table(&name, spec, &CategoryData { categories: data.categories.clone(), series: vec![first] });
            (summary, vec![], table, body)
        }
        ChartType::Bar | ChartType::Line | ChartType::Area => {
            let data = chart_data(&series_spec, rows);
            (describe(spec, &data), shown(series_legend(&data)), category_table(&name, spec, &data), marks())
        }
    };
    Figure { label: format!("{name}. {summary}"), name, summary, legend, table, body }
}
