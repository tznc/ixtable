//! Read-only results table for query runs (port of src/data/ResultGrid.tsx).
//!
//! Test ids: `col-<c>` for headers, `cell-<r>-<c>` for visible cells, `results-empty`
//! and `results-none` for the two empty states.

use crate::data::DataValue;
use crate::ui::{prelude::*, widgets};
use std::ops::Range;

const COLUMN_WIDTH: f32 = 170.;
const ROW_HEIGHT: f32 = 28.;

/// A value as the grid shows it (port of `showValue`).
pub fn show_value(value: &DataValue) -> String {
    match value {
        DataValue::Null => "NULL".into(),
        DataValue::Blob(b) => format!("Blob ({} base64 chars)", b.len()),
        DataValue::Integer(i) => i.to_string(),
        DataValue::Real(r) => r.to_string(),
        DataValue::Boolean(b) => b.to_string(),
        DataValue::Text(s)
        | DataValue::Date(s)
        | DataValue::Timestamp(s)
        | DataValue::Decimal(s)
        | DataValue::Time(s) => s.clone(),
    }
}

pub struct ResultTable {
    columns: Vec<SharedString>,
    rows: Vec<Vec<SharedString>>,
    /// False before anything ran: the table asks for a run.
    loaded: bool,
    scroll: UniformListScrollHandle,
}

impl ResultTable {
    pub fn new() -> Self {
        Self {
            columns: vec![],
            rows: vec![],
            loaded: false,
            scroll: UniformListScrollHandle::new(),
        }
    }

    pub fn set(&mut self, columns: Vec<String>, rows: Vec<Vec<DataValue>>, cx: &mut Context<Self>) {
        self.columns = columns.into_iter().map(SharedString::from).collect();
        self.rows = rows
            .iter()
            .map(|r| r.iter().map(|v| SharedString::from(show_value(v))).collect())
            .collect();
        self.loaded = true;
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.columns.clear();
        self.rows.clear();
        self.loaded = false;
        cx.notify();
    }

    /// Every cell as shown.
    pub fn cells(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|r| r.iter().map(|c| c.to_string()).collect())
            .collect()
    }

    fn visible_rows(&self, range: Range<usize>, cx: &App) -> Vec<Div> {
        let border = cx.theme().border;
        range
            .filter_map(|r| self.rows.get(r).map(|row| (r, row)))
            .map(|(r, row)| {
                h_flex()
                    .h(px(ROW_HEIGHT))
                    .border_b_1()
                    .border_color(border)
                    .children(row.iter().enumerate().map(|(c, value)| {
                        widgets::styled_text(
                            SharedString::from(format!("cell-{r}-{c}")),
                            value.clone(),
                            |d| {
                                d.w(px(COLUMN_WIDTH))
                                    .flex_none()
                                    .px_2()
                                    .py_1()
                                    .text_sm()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                            },
                        )
                    }))
            })
            .collect()
    }
}

impl Default for ResultTable {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for ResultTable {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.loaded {
            return widgets::empty_state("results-empty", "Run a query to see results", "", cx)
                .into_any_element();
        }
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted);
        let width = (self.columns.len().max(1) as f32) * COLUMN_WIDTH;
        let header = h_flex()
            .h(px(ROW_HEIGHT))
            .bg(muted)
            .border_b_1()
            .border_color(border)
            .children(self.columns.iter().enumerate().map(|(c, name)| {
                widgets::styled_text(SharedString::from(format!("col-{c}")), name.clone(), |d| {
                    d.w(px(COLUMN_WIDTH))
                        .flex_none()
                        .px_2()
                        .py_1()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                })
            }));
        let count = self.rows.len();
        let body = uniform_list(
            "result-rows",
            count,
            cx.processor(|this, range: Range<usize>, _, cx| this.visible_rows(range, cx)),
        )
        .track_scroll(&self.scroll)
        .h(px((count.min(12) as f32) * ROW_HEIGHT));
        v_flex()
            .id("result-table")
            .overflow_x_scroll()
            .w_full()
            .child(
                v_flex()
                    .w(px(width))
                    .child(header)
                    .child(body)
                    .when(count == 0, |el| {
                        el.child(widgets::empty_state(
                            "results-none",
                            "Query returned no rows",
                            "",
                            cx,
                        ))
                    }),
            )
            .into_any_element()
    }
}
