//! Ports of tests/unit/dashboard-charts.test.ts (the geometry reports and dashboards share,
//! `report_engine::chart_geometry`) and tests/unit/dashboard-chart-render.test.tsx (the
//! figure's name, legend, data table and marks). The web test read tooltips off SVG
//! elements; the native chart has none, so the tooltip assertion is not ported. Imports are
//! explicit.

use super::chart::{Body, ChartSpec, figure, sparkline};
use crate::dashboards::ChartType;
use crate::report_engine::{
    Mark, PathOp, Row,
    chart_geometry::{
        CategoryData, Frame, LinePoint, Margin, Point, SERIES_COLORS, ScatterSeries, Series, band_scale, bar_geometry,
        line_geometry, linear_scale, nice_step, nice_ticks, path_of, pie_geometry, scatter_geometry,
    },
    chart_paths::wedge_path,
};
use serde_json::json;
use std::f64::consts::{FRAC_PI_2, PI};

const FRAME: Frame = Frame { width: 100.0, height: 100.0, margin: Margin { top: 0.0, right: 0.0, bottom: 0.0, left: 0.0 } };

#[test]
fn nice_ticks_round_bounds_out_to_1_2_5_steps() {
    let t = nice_ticks(0.0, 97.0, 5.0);
    assert_eq!((t.min, t.max, t.step), (0.0, 100.0, 20.0));
    assert_eq!(t.ticks, [0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
    let t = nice_ticks(-37.0, 112.0, 5.0);
    assert_eq!((t.min, t.max, t.step), (-50.0, 150.0, 50.0));
    assert_eq!(t.ticks, [-50.0, 0.0, 50.0, 100.0, 150.0]);
    assert_eq!(nice_ticks(3.0, 7.0, 4.0).ticks, [3.0, 4.0, 5.0, 6.0, 7.0]);
    assert_eq!(nice_ticks(0.0, 1000.0, 4.0).ticks, [0.0, 500.0, 1000.0]);
}

#[test]
fn nice_ticks_print_decimal_steps_without_float_noise() {
    assert_eq!(nice_ticks(0.1, 0.87, 4.0).ticks, [0.0, 0.2, 0.4, 0.6, 0.8, 1.0]);
    assert_eq!(nice_ticks(0.0, 0.3, 3.0).ticks, [0.0, 0.1, 0.2, 0.3]);
    let t = nice_ticks(1.01, 1.04, 3.0);
    assert_eq!((t.min, t.max, t.step), (1.01, 1.04, 0.01));
}

#[test]
fn nice_ticks_handle_flat_reversed_and_non_finite_input() {
    assert_eq!(nice_ticks(5.0, 5.0, 5.0).ticks, [0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
    let t = nice_ticks(-4.0, -4.0, 5.0);
    assert_eq!((t.min, t.max), (-4.0, 0.0));
    assert_eq!(nice_ticks(0.0, 0.0, 5.0).ticks, [0.0, 0.2, 0.4, 0.6, 0.8, 1.0]);
    assert_eq!(nice_ticks(10.0, 0.0, 5.0), nice_ticks(0.0, 10.0, 5.0));
    assert_eq!(nice_ticks(f64::NAN, f64::INFINITY, 5.0).ticks, [0.0, 0.2, 0.4, 0.6, 0.8, 1.0]);
}

#[test]
fn chooses_steps_and_scales() {
    assert_eq!([nice_step(10.0, 5.0), nice_step(14.0, 5.0), nice_step(40.0, 5.0), nice_step(70.0, 5.0)], [2.0, 5.0, 10.0, 20.0]);
    assert_eq!(linear_scale((0.0, 10.0), (100.0, 0.0))(2.5), 75.0);
    assert_eq!(linear_scale((3.0, 3.0), (10.0, 20.0))(3.0), 10.0);
    let band = band_scale(4, (0.0, 100.0));
    assert_eq!([band.width, band.start(1), band.center(3)], [25.0, 25.0, 87.5]);
}

fn sales() -> CategoryData {
    CategoryData {
        categories: vec!["East".into(), "West".into()],
        series: vec![
            Series { name: "2025".into(), values: vec![Some(40.0), Some(10.0)] },
            Series { name: "2026".into(), values: vec![Some(20.0), Some(-10.0)] },
        ],
    }
}

#[test]
fn places_grouped_bars_side_by_side_from_the_zero_line() {
    let g = bar_geometry(&sales(), false, &FRAME);
    assert_eq!(g.ticks.ticks, [-10.0, 0.0, 10.0, 20.0, 30.0, 40.0]);
    assert_eq!(g.zero_y, 80.0);
    let bars: Vec<[f64; 5]> = g.bars.iter().map(|b| [b.series as f64, b.x, b.y, b.width, b.height]).collect();
    assert_eq!(
        bars,
        [[0.0, 5.0, 0.0, 18.0, 80.0], [1.0, 25.0, 40.0, 18.0, 40.0], [0.0, 55.0, 60.0, 18.0, 20.0], [1.0, 75.0, 80.0, 18.0, 20.0]]
    );
    let labels: Vec<(&str, f64)> = g.x_labels.iter().map(|l| (l.text.as_str(), l.x)).collect();
    assert_eq!(labels, [("East", 25.0), ("West", 75.0)]);
}

#[test]
fn stacks_positives_up_and_negatives_down() {
    let g = bar_geometry(&sales(), true, &FRAME);
    assert_eq!(g.ticks.ticks, [-20.0, 0.0, 20.0, 40.0, 60.0]);
    let bars: Vec<[f64; 4]> = g.bars.iter().map(|b| [b.x, b.y, b.width, b.height]).collect();
    assert_eq!(bars, [[5.0, 25.0, 40.0, 50.0], [5.0, 0.0, 40.0, 25.0], [55.0, 62.5, 40.0, 12.5], [55.0, 75.0, 40.0, 12.5]]);
}

#[test]
fn bars_skip_null_values() {
    let data = CategoryData { categories: vec!["A".into()], series: vec![Series { name: "v".into(), values: vec![None] }] };
    let g = bar_geometry(&data, false, &FRAME);
    assert!(g.bars.is_empty());
    assert_eq!(g.ticks.ticks, [0.0, 0.2, 0.4, 0.6, 0.8, 1.0]);
}

fn quarters() -> CategoryData {
    CategoryData {
        categories: ["Q1", "Q2", "Q3", "Q4"].map(String::from).to_vec(),
        series: vec![Series { name: "v".into(), values: vec![Some(10.0), Some(30.0), None, Some(20.0)] }],
    }
}

#[test]
fn draws_lines_through_category_centers_and_breaks_at_nulls() {
    let g = line_geometry(&quarters(), false, false, &FRAME);
    assert_eq!(g.ticks.ticks, [10.0, 15.0, 20.0, 25.0, 30.0]);
    assert_eq!(g.series[0].path, [PathOp::M(12.5, 100.0), PathOp::L(37.5, 0.0), PathOp::M(87.5, 50.0)]);
    assert_eq!(g.series[0].points.len(), 3);
    assert!(g.series[0].area.is_none());
}

#[test]
fn fills_areas_down_to_zero_one_shape_per_run() {
    let g = line_geometry(&quarters(), true, false, &FRAME);
    assert_eq!(g.ticks.ticks, [0.0, 10.0, 20.0, 30.0]);
    assert_eq!(g.zero_y, 100.0);
    assert_eq!(
        g.series[0].area.clone().expect("an area"),
        [
            PathOp::M(12.5, 66.67),
            PathOp::L(37.5, 0.0),
            PathOp::L(37.5, 100.0),
            PathOp::L(12.5, 100.0),
            PathOp::Z,
            PathOp::M(87.5, 33.33),
            PathOp::L(87.5, 100.0),
            PathOp::Z,
        ]
    );
}

#[test]
fn stacks_areas_on_the_series_below() {
    let data = CategoryData {
        categories: vec!["a".into(), "b".into()],
        series: vec![
            Series { name: "x".into(), values: vec![Some(10.0), Some(20.0)] },
            Series { name: "y".into(), values: vec![Some(10.0), Some(20.0)] },
        ],
    };
    let g = line_geometry(&data, true, true, &FRAME);
    assert_eq!(g.ticks.max, 40.0);
    assert_eq!(g.series[1].path, [PathOp::M(25.0, 50.0), PathOp::L(75.0, 0.0)]);
    assert_eq!(
        g.series[1].area.clone().expect("an area"),
        [PathOp::M(25.0, 50.0), PathOp::L(75.0, 0.0), PathOp::L(75.0, 50.0), PathOp::L(25.0, 75.0), PathOp::Z]
    );
}

#[test]
fn joins_paths() {
    let points = [
        Some(LinePoint { x: 1.0, y: 2.0 }),
        None,
        Some(LinePoint { x: 3.333, y: 4.0 }),
        Some(LinePoint { x: 5.0, y: 6.0 }),
    ];
    assert_eq!(path_of(&points), [PathOp::M(1.0, 2.0), PathOp::M(3.33, 4.0), PathOp::L(5.0, 6.0)]);
}

fn end_of(ops: &[PathOp]) -> Vec<(f64, f64)> {
    ops.iter()
        .filter_map(|op| match op {
            PathOp::M(x, y) | PathOp::L(x, y) | PathOp::C(_, _, _, _, x, y) => Some((*x, *y)),
            PathOp::Z => None,
        })
        .collect()
}

#[test]
fn cuts_clockwise_slices_from_12_oclock_and_drops_non_positive_values() {
    let labels: Vec<String> = ["A", "B", "C", "D"].map(String::from).to_vec();
    let slices = pie_geometry(&labels, &[Some(25.0), Some(75.0), Some(0.0), None]);
    let shown: Vec<(&str, f64)> = slices.iter().map(|s| (s.label.as_str(), s.fraction)).collect();
    assert_eq!(shown, [("A", 0.25), ("B", 0.75)]);
    // The first quarter runs from 12 o'clock to 3 o'clock around the center.
    let quarter = wedge_path(50.0, 50.0, 50.0, 0.0, -FRAC_PI_2, 0.0);
    let points = end_of(&quarter);
    assert_eq!(points[..2], [(50.0, 50.0), (50.0, 0.0)]);
    assert_eq!(points.last(), Some(&(100.0, 50.0)));
    assert_eq!(quarter.last(), Some(&PathOp::Z));
}

#[test]
fn cuts_a_hole_for_donuts() {
    let half = wedge_path(50.0, 50.0, 50.0, 25.0, -FRAC_PI_2, FRAC_PI_2);
    let points = end_of(&half);
    assert_eq!(points[0], (50.0, 0.0));
    assert!(points.contains(&(50.0, 100.0)));
    assert!(half.contains(&PathOp::L(50.0, 75.0)));
    assert_eq!(points.last(), Some(&(50.0, 25.0)));
}

#[test]
fn draws_a_single_full_slice_as_a_closed_circle() {
    let labels = vec!["A".to_string()];
    let slices = pie_geometry(&labels, &[Some(3.0)]);
    assert_eq!(slices[0].fraction, 1.0);
    let full = wedge_path(50.0, 50.0, 50.0, 0.0, -FRAC_PI_2, -FRAC_PI_2 + 2.0 * PI);
    assert_eq!(full[0], PathOp::M(50.0, 0.0));
    assert_eq!(full.last(), Some(&PathOp::Z));
    assert_eq!(end_of(&full).last(), Some(&(50.0, 0.0)));
}

#[test]
fn maps_scatter_points_onto_nice_x_and_y_axes() {
    let data = vec![ScatterSeries {
        name: "s".into(),
        points: vec![Point { x: 0.0, y: 0.0, label: "0".into() }, Point { x: 8.0, y: 45.0, label: "8".into() }],
    }];
    let g = scatter_geometry(&data, &FRAME);
    assert_eq!(g.x_ticks.ticks, [0.0, 2.0, 4.0, 6.0, 8.0]);
    assert_eq!(g.y_ticks.ticks, [0.0, 10.0, 20.0, 30.0, 40.0, 50.0]);
    let points: Vec<(f64, f64)> = g.series[0].iter().map(|p| (p.cx, p.cy)).collect();
    assert_eq!(points, [(0.0, 100.0), (100.0, 10.0)]);
}

#[test]
fn draws_a_sparkline_across_the_full_width() {
    let (path, last) = sparkline(&[Some(1.0), Some(3.0), None, Some(2.0)], 100.0, 20.0, 0.0);
    assert_eq!(path, [PathOp::M(0.0, 20.0), PathOp::L(33.33, 0.0), PathOp::M(100.0, 10.0)]);
    assert_eq!(last, Some((100.0, 10.0)));
    assert_eq!(sparkline(&[Some(5.0), Some(5.0)], 10.0, 10.0, 0.0).0, [PathOp::M(0.0, 5.0), PathOp::L(10.0, 5.0)]);
    assert_eq!(sparkline(&[None], 160.0, 40.0, 3.0).1, None);
}

#[test]
fn the_palette_has_eight_distinct_colors_with_at_least_3_to_1_contrast_on_white() {
    let luminance = |hex: &str| {
        let channel = |i: usize| {
            let c = f64::from(u8::from_str_radix(&hex[1 + i * 2..3 + i * 2], 16).expect("hex")) / 255.0;
            if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(0) + 0.7152 * channel(1) + 0.0722 * channel(2)
    };
    let distinct: std::collections::HashSet<_> = SERIES_COLORS.iter().collect();
    assert_eq!(distinct.len(), 8);
    for color in SERIES_COLORS {
        assert!(1.05 / (luminance(color) + 0.05) >= 3.0, "{color}");
    }
}

// ---- Figures ------------------------------------------------------------------------

fn months() -> Vec<Row> {
    [
        json!({"month": "Jan", "sales": 10, "cost": 6}),
        json!({"month": "Feb", "sales": 30, "cost": 12}),
        json!({"month": "Mar", "sales": 20, "cost": 8}),
    ]
    .into_iter()
    .map(|v| v.as_object().cloned().expect("object"))
    .collect()
}

fn spec(chart_type: ChartType, y: &[&str]) -> ChartSpec {
    ChartSpec {
        chart_type,
        x: Some("month".into()),
        y: y.iter().map(|s| s.to_string()).collect(),
        group_by: None,
        stacked: false,
        format: None,
    }
}

const SIZE: (f64, f64) = (300.0, 160.0);

#[test]
fn bar_line_and_area_charts_expose_their_name_legend_and_data() {
    for chart_type in [ChartType::Bar, ChartType::Line, ChartType::Area] {
        let f = figure(&spec(chart_type, &["sales", "cost"]), "Monthly", &months(), SIZE);
        assert!(f.label.starts_with("Monthly."), "{}", f.label);
        assert!(f.label.contains("2 series over 3 month. Values from 6 to 30."), "{}", f.label);
        let legend: Vec<&str> = f.legend.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(legend, ["sales", "cost"]);
        assert_eq!(f.table.caption, "Monthly data");
        assert_eq!(f.table.row_texts(), ["monthsalescost", "Jan106", "Feb3012", "Mar208"]);
    }
}

#[test]
fn bar_marks_use_the_palette_in_series_order() {
    let f = figure(&spec(ChartType::Bar, &["sales", "cost"]), "Monthly", &months(), SIZE);
    let Body::Marks(chart) = f.body else { panic!("marks") };
    let fills: Vec<&str> = chart
        .marks
        .iter()
        .filter_map(|m| match m {
            Mark::Path(p) => p.fill.as_deref(),
            Mark::Text(_) => None,
        })
        .collect();
    let (a, b) = (SERIES_COLORS[0], SERIES_COLORS[1]);
    // Six bars (three months of two series), then the legend swatches.
    assert_eq!(fills[..6], [a, b, a, b, a, b]);
    assert_eq!(fills[6..], [a, b]);
    assert_eq!(f.legend[0].color, a);
}

#[test]
fn pie_and_donut_list_shares() {
    let f = figure(&spec(ChartType::Donut, &["sales"]), "Mix", &months(), SIZE);
    assert!(f.label.contains("Donut chart of sales across 3 month. Total 60."), "{}", f.label);
    assert_eq!(f.table.row_texts(), ["monthsalesShare", "Jan1016.7%", "Feb3050%", "Mar2033.3%"]);
    let legend: String = f.legend.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(legend, "Jan (17%)Feb (50%)Mar (33%)");
}

#[test]
fn scatter_plots_numeric_pairs() {
    let mut s = spec(ChartType::Scatter, &["cost"]);
    s.x = Some("sales".into());
    let f = figure(&s, "Margin", &months(), SIZE);
    assert!(f.label.contains("Scatter plot of 3 points: cost against sales"), "{}", f.label);
    assert!(f.legend.is_empty());
}

#[test]
fn summary_shows_the_latest_value_change_and_sparkline() {
    let mut s = spec(ChartType::Summary, &["sales"]);
    s.format = Some("#,##0".into());
    let f = figure(&s, "Sales", &months(), SIZE);
    assert!(f.label.contains("Latest sales 20, down 10 from the previous value. 3 values from 10 to 30."), "{}", f.label);
    let Body::Summary(summary) = f.body else { panic!("summary") };
    assert_eq!(summary.big, "20");
    assert_eq!(summary.delta, Some((false, "▼ 10 vs previous".to_string())));
    assert!(!summary.path.is_empty());
    assert!(summary.last.is_some());
}

#[test]
fn renders_deterministically_and_handles_empty_results() {
    let line = spec(ChartType::Line, &["sales", "cost"]);
    assert_eq!(figure(&line, "Monthly", &months(), SIZE), figure(&line, "Monthly", &months(), SIZE));
    let empty = figure(&spec(ChartType::Bar, &["sales"]), "Nothing", &[], SIZE);
    assert_eq!(empty.body, Body::Empty("No data for Nothing.".into()));
}
