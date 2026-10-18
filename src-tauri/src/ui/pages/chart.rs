//! A dashboard chart as GPUI elements: the report engine's chart marks drawn by
//! `reports::chart_element`, plus the figure's accessible text.
//!
//! GPUI has no SVG `<title>` or tooltips, so the accessible name, the legend and the data
//! table are elements with labels instead.
//!
//! Test ids: `chart:<name>` (label: `<name>. <summary>`), `chart-legend:<name>:<n>`,
//! `chart-data:<name>` (label: the table's caption) with rows `chart-row:<name>:<n>` (the
//! row's cells run together), `chart-empty:<name>`, and for summary charts
//! `chart-big:<name>` and `chart-delta:<name>`.

use crate::{
    page_engine::chart::{figure, Body, ChartSpec, Figure, SummaryBody},
    pages::PageComponent,
    report_engine::{ChartItem, Mark, PathItem},
    ui::{prelude::*, reports::chart_element},
};
use serde_json::{Map, Value};

/// The drawn box in points and the points-to-pixels zoom: 7 pt labels read as 10 px.
const BOX: (f64, f64) = (250.0, 140.0);
const ZOOM: f32 = 1.5;

pub fn chart_figure(component: &PageComponent, rows: &[Map<String, Value>]) -> Figure {
    figure(&ChartSpec::from(component), &component.title, rows, BOX)
}

fn sparkline(big: &SummaryBody) -> ChartItem {
    let (w, h) = crate::page_engine::chart::SPARKLINE;
    let mut marks = vec![Mark::Path(PathItem {
        ops: big.path.clone(),
        fill: None,
        stroke: Some("#2563eb".into()),
        line_width: 1.5,
    })];
    if let Some((x, y)) = big.last {
        let r = 2.5;
        marks.push(Mark::Path(PathItem {
            ops: vec![
                crate::report_engine::PathOp::M(x - r, y),
                crate::report_engine::PathOp::C(x - r, y - r * 0.55, x - r * 0.55, y - r, x, y - r),
                crate::report_engine::PathOp::C(x + r * 0.55, y - r, x + r, y - r * 0.55, x + r, y),
                crate::report_engine::PathOp::C(x + r, y + r * 0.55, x + r * 0.55, y + r, x, y + r),
                crate::report_engine::PathOp::C(x - r * 0.55, y + r, x - r, y + r * 0.55, x - r, y),
                crate::report_engine::PathOp::Z,
            ],
            fill: Some("#2563eb".into()),
            stroke: None,
            line_width: 0.0,
        }));
    }
    ChartItem { component_id: "sparkline".into(), x: 0.0, y: 0.0, w, h, title: String::new(), marks }
}

fn hidden(id: String, label: String) -> impl IntoElement {
    // Present for assistive technology and tests, not on screen.
    widgets::styled_text(id, label, |d| d.h(px(0.)).overflow_hidden())
}

/// The chart body for `figure`.
pub fn chart_body(figure: &Figure, cx: &App) -> AnyElement {
    let name = figure.name.clone();
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    if let Body::Empty(text) = &figure.body {
        return widgets::styled_text(format!("chart-empty:{name}"), text.clone(), move |d| {
            d.text_sm().text_color(muted)
        })
        .into_any_element();
    }
    let drawn = match &figure.body {
        Body::Marks(item) => chart_element(item, &format!("chart-{name}"), ZOOM).into_any_element(),
        Body::Summary(summary) => {
            let up = summary.delta.as_ref().is_some_and(|(up, _)| *up);
            let (success, danger) = (theme.success, theme.danger);
            v_flex()
                .gap_1()
                .child(widgets::styled_text(format!("chart-big:{name}"), summary.big.clone(), |d| {
                    d.text_3xl().font_weight(FontWeight::SEMIBOLD)
                }))
                .children(summary.delta.as_ref().map(|(_, text)| {
                    widgets::styled_text(format!("chart-delta:{name}"), text.clone(), move |d| {
                        d.text_sm().text_color(if up { success } else { danger })
                    })
                }))
                .child(chart_element(&sparkline(summary), &format!("chart-{name}"), 1.0))
                .into_any_element()
        }
        Body::Empty(_) => unreachable!(),
    };
    let legend = h_flex().gap_3().flex_wrap().children(figure.legend.iter().enumerate().map(|(i, entry)| {
        let color = crate::ui::reports::page_color(&entry.color);
        h_flex()
            .gap_1()
            .items_center()
            .child(div().size(px(10.)).rounded_sm().bg(color))
            .child(widgets::styled_text(format!("chart-legend:{name}:{i}"), entry.name.clone(), |d| d.text_xs()))
    }));
    let table = v_flex()
        .id(SharedString::from(format!("chart-data:{name}")))
        .aria_label(figure.table.caption.clone())
        .h(px(0.))
        .overflow_hidden()
        .children(
            figure
                .table
                .row_texts()
                .into_iter()
                .enumerate()
                .map(|(i, text)| hidden(format!("chart-row:{name}:{i}"), text)),
        )
        .test_support();
    v_flex()
        .id(SharedString::from(format!("chart:{name}")))
        .aria_label(figure.label.clone())
        .gap_2()
        .w_full()
        .min_w_0()
        // A chart wider than its cell scrolls inside it instead of spilling over its neighbour.
        .child(
            div()
                .id(SharedString::from(format!("chart-scroll:{name}")))
                .w_full()
                .overflow_x_scroll()
                .child(drawn),
        )
        .child(legend)
        .child(table)
        .test_support()
        .into_any_element()
}
