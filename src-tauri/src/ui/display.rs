//! Read-only data display (docs/decisions/design-language.md): `data_table` for lists
//! with columns, `key_value` for one object's properties, and `status_badge` for a
//! fixed status vocabulary. Lists never render as `Label: a, b, c` prose.

use crate::ui::prelude::*;

/// A compact read-only table: a header row, then one observed row per entry. The first
/// column is left-aligned text; the others are right-aligned numbers, with zeros and
/// dashes muted. Each row is observed under its id with `label` as its text for tests.
pub fn data_table(
    id: impl Into<ElementId>,
    headers: &[&str],
    rows: Vec<(ElementId, String, Vec<String>)>,
    cx: &App,
) -> Stateful<Div> {
    table(id, headers, rows, true, cx)
}

/// `data_table` for text-only columns: every column shares the width and is
/// left-aligned (page links, names, paths).
pub fn text_table(
    id: impl Into<ElementId>,
    headers: &[&str],
    rows: Vec<(ElementId, String, Vec<String>)>,
    cx: &App,
) -> Stateful<Div> {
    table(id, headers, rows, false, cx)
}

fn table(
    id: impl Into<ElementId>,
    headers: &[&str],
    rows: Vec<(ElementId, String, Vec<String>)>,
    numeric: bool,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, border) = (theme.muted_foreground, theme.border);
    let cell = move |value: String, first: bool| {
        let number = numeric && !first;
        let quiet = !first && (value == "0" || value == "—");
        div()
            .when(!number, |d| d.flex_1().min_w_0().overflow_hidden().text_ellipsis())
            .when(number, |d| d.w(px(104.)).flex_none().text_right())
            .when(quiet, |d| d.text_color(muted))
            .child(value)
    };
    let header = h_flex()
        .gap_3()
        .px_2()
        .pb_1()
        .border_b_1()
        .border_color(border)
        .ty(Type::Label)
        .text_color(muted)
        .children(headers.iter().enumerate().map(|(i, h)| cell(h.to_string(), i == 0)));
    v_flex()
        .id(id)
        .ty(Type::Body)
        .child(header)
        .children(rows.into_iter().map(|(row_id, label, cells)| {
            h_flex()
                .id(row_id)
                .aria_label(label)
                .gap_3()
                .px_2()
                .py_1()
                .border_b_1()
                .border_color(border.opacity(0.5))
                .children(cells.into_iter().enumerate().map(|(i, c)| cell(c, i == 0)))
                .test_support()
        }))
}

/// One row of a `key_value` grid; the value is observed under `id`.
pub struct Kv {
    id: ElementId,
    label: String,
    value: String,
    mono: bool,
    copy: bool,
}

/// A property row: `label` in the label column, `value` observed under `id`.
pub fn kv(id: impl Into<ElementId>, label: &str, value: impl Into<String>) -> Kv {
    Kv {
        id: id.into(),
        label: label.to_string(),
        value: value.into(),
        mono: false,
        copy: false,
    }
}

impl Kv {
    /// Monospace, ellipsized in the middle-free way gpui allows (end), for hashes and
    /// paths, with a copy button (`<id>-copy`) for the full value.
    pub fn code(mut self) -> Self {
        self.mono = true;
        self.copy = true;
        self
    }
}

/// A two-column grid of labels and values.
pub fn key_value(id: impl Into<ElementId>, rows: Vec<Kv>, cx: &App) -> Stateful<Div> {
    let muted = cx.theme().muted_foreground;
    v_flex()
        .id(id)
        .gap_1()
        .ty(Type::Body)
        .children(rows.into_iter().map(|row| {
            let copy_id = SharedString::from(format!("{}-copy", row.id));
            let full = row.value.clone();
            h_flex()
                .gap_3()
                .items_center()
                .child(
                    div()
                        .w(px(160.))
                        .flex_none()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(muted)
                        .child(row.label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .when(row.mono, |d| d.font_family("monospace").ty(Type::Caption))
                        .child(widgets::text(row.id, row.value)),
                )
                .when(row.copy, |r| {
                    r.child(
                        Button::new(copy_id)
                            .ghost()
                            .xsmall()
                            .icon(IconName::Copy)
                            .tooltip("Copy")
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(full.clone()))
                            }),
                    )
                })
        }))
}

/// The tone of a status badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Success,
    Warning,
    Danger,
}

/// A pill with a tone and short text (observed under `id`), such as "Applied" or
/// "Not converted". Status is carried by text and tint, never by colour alone.
pub fn status_badge(id: impl Into<ElementId>, label: &str, tone: Tone, cx: &App) -> Div {
    let theme = cx.theme();
    let (bg, fg) = match tone {
        Tone::Neutral => (theme.muted, theme.muted_foreground),
        Tone::Success => (theme.success.opacity(0.15), theme.success),
        Tone::Warning => (theme.warning.opacity(0.18), theme.warning),
        Tone::Danger => (theme.danger.opacity(0.15), theme.danger),
    };
    div()
        .flex_none()
        .px_2()
        .rounded_full()
        .ty(Type::Caption)
        .font_weight(FontWeight::MEDIUM)
        .bg(bg)
        .text_color(fg)
        .child(widgets::text(id, label.to_string()))
}
