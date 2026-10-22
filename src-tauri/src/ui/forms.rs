//! Form building blocks shared by the settings, release, cloud, migration and Access
//! views: text inputs with test ids, checkboxes, cards and inline dialogs.
//!
//! Test ids: a text input is found by the id it is built with (`Input::id`), a
//! checkbox by its id, and a card or dialog by its id (its label is its title).

use crate::ui::widgets::{self, Type, TypeScale as _};
use gpui_kit::component::{
    ActiveTheme as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputState, Textarea, TextareaState},
    v_flex,
};
use gpui_kit::*;

/// An element id built from text (`export-asset:logo.png`).
pub fn eid(text: String) -> ElementId {
    SharedString::from(text).into()
}

/// A single-line input state.
pub fn line_state<V: 'static>(
    window: &mut Window,
    cx: &mut Context<V>,
    placeholder: &str,
    value: &str,
) -> Entity<InputState> {
    let (placeholder, value) = (placeholder.to_string(), value.to_string());
    cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(value)
    })
}

/// A single-line input state that hides what is typed.
pub fn secret_state<V: 'static>(
    window: &mut Window,
    cx: &mut Context<V>,
    placeholder: &str,
) -> Entity<InputState> {
    let placeholder = placeholder.to_string();
    cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder)
            .masked(true)
    })
}

/// A multi-line input state.
pub fn area_state<V: 'static>(
    window: &mut Window,
    cx: &mut Context<V>,
    value: &str,
) -> Entity<TextareaState> {
    let value = value.to_string();
    cx.new(|cx| TextareaState::new(window, cx).default_value(value))
}

/// Replaces the text of an input (no change event reaches listeners that skip it).
pub fn set_text(state: &Entity<InputState>, text: &str, window: &mut Window, cx: &mut App) {
    let text = text.to_string();
    state.update(cx, |s, cx| s.set_value(text, window, cx));
}

pub fn text_of(state: &Entity<InputState>, cx: &App) -> String {
    state.read(cx).value().to_string()
}

pub fn area_text(state: &Entity<TextareaState>, cx: &App) -> String {
    state.read(cx).value().to_string()
}

/// A labeled single-line input found by `id`.
pub fn input_field(
    id: impl Into<ElementId>,
    label: &str,
    state: &Entity<InputState>,
    cx: &App,
) -> Div {
    widgets::field(label, Input::new(state).id(id).aria_label(label.to_string()), cx)
}

/// A labeled multi-line input; `id` names the wrapper that tests click to focus it.
pub fn area_field(
    id: impl Into<ElementId>,
    label: &str,
    state: &Entity<TextareaState>,
    height: Pixels,
    cx: &App,
) -> Div {
    widgets::field(
        label,
        div()
            .id(id)
            .aria_label(label.to_string())
            .child(Textarea::new(state).h(height).aria_label(label.to_string()))
            .test_support(),
        cx,
    )
}

/// A checkbox with a label. `on_change` receives the requested value.
pub fn check(
    id: impl Into<ElementId>,
    label: &str,
    checked: bool,
    on_change: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> Checkbox {
    Checkbox::new(id)
        .label(label.to_string())
        .checked(checked)
        .on_change(move |value, window, cx| on_change(*value, window, cx))
}

/// The heading of a card or dialog: observed text, so tests read it by `id`.
fn heading(id: &'static str, title: &str) -> impl IntoElement {
    widgets::styled_text(id, title.to_string(), |d| {
        d.ty(Type::Section)
    })
}

/// A section card. Its title is observed under `id` (a test reads or waits for it) and
/// the card itself is `<id>-card`, the scope for `click_in`.
pub fn card(id: &'static str, title: &str, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    v_flex()
        .id(SharedString::from(format!("{id}-card")))
        .gap_2()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .child(heading(id, title))
}

/// A row of buttons.
pub fn actions() -> Div {
    h_flex().gap_2().items_center().flex_wrap()
}

/// A primary action button.
pub fn primary(id: impl Into<ElementId>, label: &str) -> Button {
    Button::new(id).primary().label(label.to_string())
}

/// A destructive action (delete, remove, drop, reset, roll back): red text and outline.
pub fn danger(id: impl Into<ElementId>, label: &str) -> Button {
    Button::new(id).danger().outline().label(label.to_string())
}

/// A secondary action button.
pub fn secondary(id: impl Into<ElementId>, label: &str) -> Button {
    Button::new(id).outline().label(label.to_string())
}

/// A link-styled button.
pub fn link(id: impl Into<ElementId>, label: &str) -> Button {
    Button::new(id).link().label(label.to_string())
}

/// Muted explanatory text.
pub fn muted(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .ty(Type::Body)
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}
