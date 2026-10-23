//! `DataTable`: a read-only list of records. Headers are labels, numbers
//! align right, and an empty table says what would be here. The datasheet
//! (editable grid) is a separate widget in the app.

use gpui_kit::prelude::*;
use std::rc::Rc;

use gpui_bootstrap::ActiveTheme as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, IntoElement, RenderOnce, SharedString, Window,
    base::{h_flex, v_flex},
    div, px,
};

use crate::{
    empty_state::EmptyState,
    typography::{Type, TypeScale as _},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Start,
    /// Numbers and money.
    End,
}

/// A column: its header and how its cells align.
#[derive(Clone, Debug)]
pub struct Column {
    label: SharedString,
    align: Align,
    width: Option<f32>,
}

impl Column {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            align: Align::Start,
            width: None,
        }
    }

    /// A right-aligned number column.
    pub fn number(label: impl Into<SharedString>) -> Self {
        Self::new(label).align(Align::End)
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// A fixed width in pixels; other columns share the rest.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
}

type Select = Rc<dyn Fn(usize, &ClickEvent, &mut Window, &mut App)>;

#[cfg_attr(
    feature = "styleguide",
    gpui_styleguidist::preview("data-table/playground")
)]
#[derive(IntoElement)]
pub struct DataTable {
    id: ElementId,
    columns: Vec<Column>,
    rows: Vec<Vec<AnyElement>>,
    selected: Option<usize>,
    on_select: Option<Select>,
    empty: Option<EmptyState>,
    footer: Option<SharedString>,
}

impl DataTable {
    pub fn new(id: impl Into<ElementId>, columns: impl IntoIterator<Item = Column>) -> Self {
        Self {
            id: id.into(),
            columns: columns.into_iter().collect(),
            rows: Vec::new(),
            selected: None,
            on_select: None,
            empty: None,
            footer: None,
        }
    }

    pub fn row(mut self, cells: impl IntoIterator<Item = AnyElement>) -> Self {
        self.rows.push(cells.into_iter().collect());
        self
    }

    pub fn text_row<S: Into<SharedString>>(self, cells: impl IntoIterator<Item = S>) -> Self {
        self.row(cells.into_iter().map(|cell| cell.into().into_any_element()))
    }

    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// Makes rows clickable. The application stores the selection.
    pub fn on_select(
        mut self,
        handler: impl Fn(usize, &ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }

    /// Shown instead of the table when there are no rows.
    pub fn empty(mut self, empty: EmptyState) -> Self {
        self.empty = Some(empty);
        self
    }

    /// A count under the table: "4 of 830 orders".
    pub fn footer(mut self, footer: impl Into<SharedString>) -> Self {
        self.footer = Some(footer.into());
        self
    }
}

fn cell(
    column: &Column,
    (pad_x, pad_y): (gpui_kit::Pixels, gpui_kit::Pixels),
    rule: gpui_kit::Hsla,
    last: bool,
) -> gpui_kit::Div {
    div()
        .px(pad_x)
        .py(pad_y)
        .when(!last, |this| this.border_r_1().border_color(rule))
        .min_w_0()
        .overflow_hidden()
        .flex()
        .items_center()
        .map(|this| match column.width {
            Some(width) => this.w(px(width)).flex_none(),
            None => this.flex_1(),
        })
        .when(column.align == Align::End, |this| this.justify_end())
}

impl RenderOnce for DataTable {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let (pad_x, pad_y) = (theme.spacing(3) * 0.625, theme.spacing(2) * 0.75);
        let id = self.id;
        if self.rows.is_empty()
            && let Some(empty) = self.empty
        {
            return v_flex().id(id).child(empty);
        }
        // A light wash of the accent: the selected row stays readable.
        let selected_bg = colors.primary.opacity(0.12);
        // Friendly grid: a tinted header row and a rule between every cell.
        let rule = colors.border_color;
        let last = self.columns.len().saturating_sub(1);
        let header = h_flex()
            .id((id.clone(), "header"))
            .items_stretch()
            .bg(colors.tertiary_bg)
            .border_b(theme.border_width)
            .border_color(rule)
            .children(self.columns.iter().enumerate().map(|(index, column)| {
                cell(column, (pad_x, pad_y), rule, index == last)
                    .ty(Type::Label)
                    .text_color(colors.emphasis_color)
                    .child(column.label.clone())
            }));
        let columns = Rc::new(self.columns);
        let row_count = self.rows.len();
        let rows = self.rows.into_iter().enumerate().map(|(index, cells)| {
            let columns = columns.clone();
            h_flex()
                .id((id.clone(), SharedString::from(format!("row-{index}"))))
                .items_stretch()
                .when(index + 1 < row_count, |this| {
                    this.border_b(theme.border_width)
                        .border_color(rule.opacity(0.7))
                })
                .ty(Type::Body)
                .when(self.selected == Some(index), |this| this.bg(selected_bg))
                .when_some(self.on_select.clone(), |this, select| {
                    this.cursor_pointer()
                        .hover(|style| style.bg(colors.tertiary_bg))
                        .on_click(move |event, window, cx| select(index, event, window, cx))
                })
                .children(cells.into_iter().zip(columns.iter()).enumerate().map(
                    |(i, (content, column))| {
                        cell(column, (pad_x, pad_y), rule.opacity(0.7), i == last).child(content)
                    },
                ))
        });
        let table = v_flex()
            .border(theme.border_width)
            .border_color(rule)
            .rounded(theme.radius.base)
            .overflow_hidden()
            .child(header)
            .children(rows);
        v_flex()
            .id(id.clone())
            .child(table)
            .when_some(self.footer, |this, footer| {
                this.child(
                    div()
                        .id((id, "footer"))
                        .pt(pad_y)
                        .ty(Type::Caption)
                        .text_color(colors.secondary_color)
                        .child(footer),
                )
            })
    }
}
