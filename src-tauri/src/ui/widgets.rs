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

/// The type scale (design-language.md §2). Feature code sizes text only through
/// `.ty(Type::..)`; raw `text_xs()`..`text_3xl()` calls are linted outside this file.
/// `ty` sets size and weight; color stays with the caller (`Label` and `Caption` take
/// `muted_foreground` unless they carry a status color).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    /// A dashboard metric value or the start-screen wordmark.
    Display,
    /// The mode or page title, one per view.
    Title,
    /// Card, dialog and panel-section titles.
    Section,
    /// Content, inputs and table cells.
    Body,
    /// Field labels, column headers and nav items.
    Label,
    /// Hints, counts, timestamps and help.
    Caption,
}

pub trait TypeScale: Styled + Sized {
    fn ty(self, ty: Type) -> Self {
        match ty {
            Type::Display => self.text_3xl().font_weight(FontWeight::SEMIBOLD),
            Type::Title => self.text_xl().font_weight(FontWeight::SEMIBOLD),
            Type::Section => self.text_base().font_weight(FontWeight::SEMIBOLD),
            Type::Body => self.text_sm(),
            Type::Label => self.text_sm().font_weight(FontWeight::MEDIUM),
            Type::Caption => self.text_xs(),
        }
    }
}

impl<T: Styled> TypeScale for T {}

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
                        .ty(Type::Caption)
                        .text_color(theme.muted_foreground)
                        .child(kicker.to_uppercase()),
                )
                .child(styled_text("mode-title", title.to_string(), |d| {
                    d.ty(Type::Title)
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
                .ty(Type::Body)
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
                .ty(Type::Body)
                .child(styled_text(
                    SharedString::from(format!("{id}-headline")),
                    headline.to_string(),
                    |d| d.font_weight(FontWeight::SEMIBOLD),
                ))
                .child(text(id.clone(), error.message.clone()))
                // A plain-text error (no code) shows no support line.
                .when(!error.code.is_empty(), |col| {
                    col.child(styled_text(
                        SharedString::from(format!("{id}-code")),
                        format!("Error code {}", error.code),
                        |d| d.ty(Type::Caption).text_color(theme.muted_foreground),
                    ))
                }),
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
        .ty(Type::Body)
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
        .child(div().ty(Type::Section).child(title.to_string()))
        .child(
            div()
                .ty(Type::Body)
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
                .ty(Type::Label)
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(control)
}

/// A section heading inside a panel.
pub fn section_heading(id: impl Into<ElementId>, title: &str) -> impl IntoElement {
    styled_text(id, title.to_string(), |d| d.ty(Type::Section))
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

/// The singular form of a plural noun, the inverse of `plural_word` ("orders" -> "order",
/// "categories" -> "category", "boxes" -> "box"). Words not ending in "s" are unchanged.
pub fn singular(word: &str) -> String {
    match word {
        w if w.ends_with("ies") && w.len() > 3 => format!("{}y", &w[..w.len() - 3]),
        w if w.ends_with("ches") || w.ends_with("xes") || w.ends_with("sses") => w[..w.len() - 2].to_string(),
        w if w.ends_with("ss") => w.to_string(),
        w if w.ends_with('s') && w.len() > 1 => w[..w.len() - 1].to_string(),
        w => w.to_string(),
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

    #[test]
    fn type_scale_maps_steps_to_sizes_and_weights() {
        use super::{Type, TypeScale as _};
        use gpui_kit::{FontWeight, Styled as _, div};
        let text = |mut d: gpui_kit::Div| format!("{:?}", d.style().text);
        assert_eq!(text(div().ty(Type::Title)), text(div().text_xl().font_weight(FontWeight::SEMIBOLD)));
        assert_eq!(text(div().ty(Type::Section)), text(div().text_base().font_weight(FontWeight::SEMIBOLD)));
        assert_eq!(text(div().ty(Type::Body)), text(div().text_sm()));
        assert_eq!(text(div().ty(Type::Label)), text(div().text_sm().font_weight(FontWeight::MEDIUM)));
        assert_eq!(text(div().ty(Type::Caption)), text(div().text_xs()));
        assert_ne!(text(div().ty(Type::Display)), text(div().ty(Type::Title)));
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
                .selected_tint(key == selected, cx)
                .on_click(move |_, window, cx| on_select(key, window, cx))
        }))
}

#[cfg(test)]
mod plural_tests {
    use super::{plural, singular};

    #[test]
    fn singular_inverts_plural_word() {
        for (one, many) in [("order", "orders"), ("category", "categories"), ("box", "boxes"), ("match", "matches"), ("key", "keys")] {
            assert_eq!(singular(many), one);
        }
        assert_eq!(singular("staff"), "staff");
        assert_eq!(singular("address"), "address");
    }

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

/// One tab button in the shared tab look: ghost, medium label, accent tint when selected.
/// For tab rows whose keys are dynamic (form tabs); `tab_strip` covers fixed ones.
pub fn tab_button(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool, cx: &App) -> Button {
    Button::new(id).ghost().small().label(label).font_weight(FontWeight::MEDIUM).selected_tint(selected, cx)
}

/// Selection in navigation, lists and tabs (design-language.md §4): an accent tint and a
/// medium-weight label, so it never reads as hover or disabled. Also sets `selected`.
pub trait SelectedTint: Sized {
    fn selected_tint(self, selected: bool, cx: &App) -> Self;
}

impl SelectedTint for Button {
    fn selected_tint(self, selected: bool, cx: &App) -> Self {
        use gpui_kit::component::Selectable as _;
        let tint = cx.theme().primary.opacity(0.18);
        self.selected(selected).when(selected, |b| b.bg(tint).font_weight(FontWeight::MEDIUM))
    }
}

/// A breadcrumb for drill-in navigation (nested record, form → control). `trail` crumbs are
/// link-style buttons (ids `<id>-<index>`) that call `on_click` with their index; `current`
/// is the open item as plain text (id `<id>-current`). Without `current` the trail ends in a chevron.
/// The container `id` carries the whole path as its label ("A › B").
pub fn breadcrumb(
    id: &'static str,
    trail: Vec<SharedString>,
    current: Option<SharedString>,
    on_click: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> Breadcrumb {
    Breadcrumb { id, trail, current, on_click: std::rc::Rc::new(on_click) }
}

#[derive(IntoElement)]
pub struct Breadcrumb {
    id: &'static str,
    trail: Vec<SharedString>,
    current: Option<SharedString>,
    on_click: std::rc::Rc<dyn Fn(usize, &mut Window, &mut App)>,
}

impl RenderOnce for Breadcrumb {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (id, muted, foreground) = (self.id, cx.theme().muted_foreground, cx.theme().foreground);
        let label = self.trail.iter().chain(self.current.iter()).map(|c| c.as_ref()).collect::<Vec<_>>().join(" › ");
        let last = self.trail.len().saturating_sub(1);
        let has_current = self.current.is_some();
        let mut row = h_flex().id(id).aria_label(label).gap_1().items_center();
        for (index, crumb) in self.trail.into_iter().enumerate() {
            let on_click = self.on_click.clone();
            row = row.child(
                Button::new(SharedString::from(format!("{id}-{index}")))
                    .link()
                    .small()
                    .label(crumb)
                    .text_color(muted)
                    .on_click(move |_, window, cx| on_click(index, window, cx)),
            );
            if has_current || index < last {
                row = row.child(div().ty(Type::Body).text_color(muted).child("›"));
            }
        }
        if let Some(current) = self.current {
            row = row.child(styled_text(SharedString::from(format!("{id}-current")), current, move |d| {
                d.ty(Type::Body).font_weight(FontWeight::MEDIUM).text_color(foreground)
            }));
        }
        row.test_support()
    }
}

/// Slides `element` in: its height grows from 0 to its natural height (capped at 160 px)
/// while it fades in, over 200 ms. `id` should change with the content so a new
/// message re-animates.
pub fn slide_in(id: impl Into<ElementId>, element: impl IntoElement) -> impl IntoElement {
    use gpui::{Animation, AnimationExt as _, ease_out_quint};
    div()
        .w_full()
        .overflow_hidden()
        .child(element)
        .with_animation(
            id,
            Animation::new(std::time::Duration::from_millis(200)).with_easing(ease_out_quint()),
            // The cap only applies while sliding, so long messages are never clipped once shown.
            |el, delta| if delta >= 1. { el } else { el.max_h(px(160. * delta)).opacity(delta) },
        )
}
