//! Small shared building blocks on top of gpui-kit: banners, title bars, empty states
//! and labeled fields. Feature views use these so every mode looks and tests the same.
//!
//! Test ids: an element whose text a test reads gets `.id(..)`, `.test_support()` and an
//! `aria_label` with that text (the headless tests read labels, not pixels).

use crate::manager::AppError;
use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

/// Left-aligned button content. gpui-component centers a button's icon and label and
/// ignores `justify_start`, so list and navigation buttons put both in a full-width row.
/// The label stays the button's accessibility label, so tests still find it by name.
pub trait StartAligned: Sized {
    fn start_label(self, icon: Option<gpui_kit::component::Icon>, label: impl Into<SharedString>) -> Self;
    /// A left-aligned, ellipsized label with `icon` at the right edge (selects, menus).
    fn end_icon_label(self, icon: impl Into<gpui_kit::component::Icon>, label: impl Into<SharedString>) -> Self;
}

impl StartAligned for Button {
    fn end_icon_label(self, icon: impl Into<gpui_kit::component::Icon>, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.accessibility_label(label.clone()).child(
            h_flex()
                .w_full()
                .min_w_0()
                .gap_1()
                .justify_between()
                .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(label))
                .child(icon.into().small()),
        )
    }

    fn start_label(self, icon: Option<gpui_kit::component::Icon>, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.accessibility_label(label.clone()).child(
            h_flex()
                .w_full()
                .min_w_0()
                .gap_1()
                .justify_start()
                .when_some(icon, |row, icon| row.child(icon.small()))
                .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(label)),
        )
    }
}

/// The theme color a table cell or field takes for its tone (None for a custom tone).
pub fn tone_color(tone: &crate::design::Tone, cx: &App) -> Option<Hsla> {
    use crate::design::Tone;
    let theme = cx.theme();
    Some(match tone {
        Tone::Positive => theme.success,
        Tone::Negative => theme.danger,
        Tone::Warning => theme.warning,
        Tone::Muted => theme.muted_foreground,
        Tone::Emphasis => theme.primary,
        Tone::Other(_) => return None,
    })
}

/// Observed text: `id` gets `label` as its accessibility label so tests can read it.
pub fn text(id: impl Into<ElementId>, label: impl Into<SharedString>) -> impl IntoElement {
    styled_text(id, label, |d| d)
}

/// `text` with styles. Observation wraps the element last (`test_support` returns a
/// type without style methods), so styles go through `style`.
pub fn styled_text(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    style: impl FnOnce(Stateful<Div>) -> Stateful<Div>,
) -> impl IntoElement {
    let label = label.into();
    style(div().id(id).aria_label(label.clone()).child(label)).test_support()
}

/// Observes any element with an id and accessibility label (for tests).
pub fn observed(el: Stateful<Div>) -> impl IntoElement {
    el.test_support()
}

/// A mode's heading block: a small kicker line and the title (id `mode-title`).
pub fn titlebar(kicker: &str, title: &str, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .w_full()
        .justify_between()
        .items_center()
        .px_4()
        .py_3()
        .border_b_1()
        .border_color(theme.border)
        .child(
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(kicker.to_uppercase()),
                )
                .child(styled_text("mode-title", title.to_string(), |d| {
                    d.text_xl().font_weight(FontWeight::SEMIBOLD)
                })),
        )
}

/// How loud a callout is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Success,
    Warning,
    Danger,
}

impl Severity {
    pub fn color(self, cx: &App) -> Hsla {
        let theme = cx.theme();
        match self {
            Self::Info => theme.primary,
            Self::Success => theme.success,
            Self::Warning => theme.warning,
            Self::Danger => theme.danger,
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            Self::Info => IconName::Info,
            Self::Success => IconName::CircleCheck,
            Self::Warning => IconName::TriangleAlert,
            Self::Danger => IconName::CircleX,
        }
    }
}

/// A tinted box with an icon, a bold title (observed under `id`) and optional body text
/// (`<id>-body`) and children. Status is never shown by text colour alone.
pub fn callout(
    id: impl Into<ElementId>,
    severity: Severity,
    title: &str,
    body: Option<&str>,
    cx: &App,
) -> Div {
    let id: ElementId = id.into();
    let color = severity.color(cx);
    let theme = cx.theme();
    h_flex()
        .w_full()
        .gap_2()
        .items_start()
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(color.opacity(0.35))
        .bg(color.opacity(0.08))
        .child(div().pt_0p5().text_color(color).child(gpui_kit::component::Icon::new(severity.icon()).small()))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .text_sm()
                .child(styled_text(id.clone(), title.to_string(), |d| {
                    d.font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground)
                }))
                .when_some(body, |col, body| {
                    col.child(styled_text(
                        SharedString::from(format!("{id}-body")),
                        body.to_string(),
                        |d| d.text_color(theme.foreground),
                    ))
                }),
        )
}

/// A plain-language headline for an error code, shown above the message.
pub fn error_headline(code: &str) -> &'static str {
    match code {
        "IO_ERROR" | "MISSING_FILE" => "The file could not be read or written.",
        "IMPORT_FAILED" => "This file can't be imported.",
        "VALIDATION_ERROR" | "VALIDATION" | "INVALID_CONFIG" | "INVALID_FILTER" => {
            "This can't be saved as it is."
        }
        "NO_DOCUMENT" => "No document is open.",
        "NOT_FOUND" => "It no longer exists.",
        "FORBIDDEN" | "READ_ONLY" | "REVOKED" => "You don't have permission to do this.",
        "DATABASE_ERROR" | "CONNECTION" => "The database could not be reached.",
        "BUSY" => "Another task is still running.",
        "BUNDLE_SIGNATURE" | "MANIFEST_SIGNATURE" => "This bundle's signature could not be verified.",
        "BUNDLE_PASSWORD" => "The password is not correct.",
        "MIGRATION_FAILED" => "A migration failed. Nothing was changed.",
        _ => "Something went wrong.",
    }
}

/// An error callout: a plain-language headline, the message (observed under `id`), and
/// the error code as a muted detail (`<id>-code`) for support. With `on_dismiss`, a
/// dismiss button (`dismiss-error`).
pub fn error_banner(
    id: impl Into<ElementId>,
    error: &AppError,
    on_dismiss: Option<Box<dyn Fn(&mut Window, &mut App) + 'static>>,
    cx: &App,
) -> Div {
    error_callout(id, error_headline(&error.code), error, on_dismiss, cx)
}

/// `error_banner` with a headline the caller chose (for a domain's own error codes).
pub fn error_callout(
    id: impl Into<ElementId>,
    headline: &str,
    error: &AppError,
    on_dismiss: Option<Box<dyn Fn(&mut Window, &mut App) + 'static>>,
    cx: &App,
) -> Div {
    let id: ElementId = id.into();
    let theme = cx.theme();
    let color = theme.danger;
    h_flex()
        .w_full()
        .gap_2()
        .items_start()
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(color.opacity(0.35))
        .bg(color.opacity(0.08))
        .child(div().pt_0p5().text_color(color).child(gpui_kit::component::Icon::new(IconName::CircleX).small()))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .text_sm()
                .child(styled_text(
                    SharedString::from(format!("{id}-headline")),
                    headline.to_string(),
                    |d| d.font_weight(FontWeight::SEMIBOLD),
                ))
                .child(text(id.clone(), error.message.clone()))
                .child(styled_text(
                    SharedString::from(format!("{id}-code")),
                    format!("Error code {}", error.code),
                    |d| d.text_xs().text_color(theme.muted_foreground),
                )),
        )
        .when_some(on_dismiss, |el, f| {
            el.child(
                Button::new("dismiss-error")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .tooltip("Dismiss error")
                    .on_click(move |_, window, cx| f(window, cx)),
            )
        })
}

/// Progress line (id `progress`) shown while a long command runs.
pub fn progress_line(label: &str, cx: &App) -> Div {
    div()
        .px_3()
        .py_1()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text("progress", label.to_string()))
}

/// Centered empty state with a title and one line of explanation.
pub fn empty_state(id: impl Into<ElementId>, title: &str, body: &str, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    v_flex()
        .id(id)
        .aria_label(title.to_string())
        .items_center()
        .justify_center()
        .gap_1()
        .p_8()
        .child(div().font_weight(FontWeight::SEMIBOLD).child(title.to_string()))
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(body.to_string()),
        )
        .test_support()
}

/// A labeled form row: the label above its control.
pub fn field(label: &str, control: impl IntoElement, cx: &App) -> Div {
    v_flex()
        .gap_1()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(control)
}

/// A section heading inside a panel.
pub fn section_heading(id: impl Into<ElementId>, title: &str) -> impl IntoElement {
    styled_text(id, title.to_string(), |d| d.text_lg().font_weight(FontWeight::SEMIBOLD))
}

/// A count with its noun, grouped and pluralized: "1 table", "1,204 records". Nouns
/// that don't take a plain "s" pass through `plural_word`'s exceptions.
pub fn plural(count: usize, word: &str) -> String {
    let noun = if count == 1 { word.to_string() } else { plural_word(word) };
    format!("{} {noun}", crate::ui::export::names::thousands(count as u64))
}

/// The plural form of an English noun as the app uses them ("entry" -> "entries").
pub fn plural_word(word: &str) -> String {
    match word {
        w if w.ends_with('y') && !w.ends_with("ey") && !w.ends_with("ay") => format!("{}ies", &w[..w.len() - 1]),
        w if w.ends_with('s') || w.ends_with("ch") || w.ends_with('x') => format!("{w}es"),
        w => format!("{w}s"),
    }
}

/// Turns a label into a kebab-case element id ("Save project" -> "save-project").
pub fn slug(label: &str) -> String {
    let mut out = String::new();
    for c in label.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn slug_kebab_cases_labels() {
        assert_eq!(super::slug("Save project as"), "save-project-as");
        assert_eq!(super::slug("  Re-check!  "), "re-check");
    }
}

/// A tab strip of gpui-kit buttons. Each tab is a button with id `tab-<key>` (testable
/// with `click`), and the selected one reports `selected`. `on_select` gets the key.
pub fn tab_strip(
    id: impl Into<ElementId>,
    tabs: &[(&'static str, &'static str)],
    selected: &str,
    on_select: impl Fn(&'static str, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Stateful<Div> {
    use gpui_kit::component::Selectable as _;
    let on_select = std::rc::Rc::new(on_select);
    h_flex()
        .id(id)
        .gap_1()
        .px_3()
        .py_1()
        .border_b_1()
        .border_color(cx.theme().border)
        .children(tabs.iter().map(|&(key, label)| {
            let on_select = on_select.clone();
            Button::new(SharedString::from(format!("tab-{key}")))
                .ghost()
                .small()
                .label(label)
                .selected(key == selected)
                .on_click(move |_, window, cx| on_select(key, window, cx))
        }))
}

#[cfg(test)]
mod plural_tests {
    use super::plural;

    #[test]
    fn pluralizes_counts_with_grouping() {
        assert_eq!(plural(1, "table"), "1 table");
        assert_eq!(plural(0, "record"), "0 records");
        assert_eq!(plural(1204, "record"), "1,204 records");
        assert_eq!(plural(2, "query"), "2 queries");
        assert_eq!(plural(3, "match"), "3 matches");
        assert_eq!(plural(2, "key"), "2 keys");
    }
}
