//! Small form controls shared by the schema, import and export views: text inputs
//! backed by `InputState`, checkboxes, and a select built from a dropdown menu.
//!
//! Test ids: every control takes the id its view gives it. A select is a button with
//! that id; its options are the items (by index) of the `popup-menu` it opens.

use crate::ui::prelude::*;
use gpui_kit::component::{
    checkbox::Checkbox,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
};
use std::rc::Rc;

/// A single-line input with a placeholder and initial text.
pub fn new_input<V: 'static>(
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

pub fn input_text(state: &Entity<InputState>, cx: &App) -> String {
    state.read(cx).value().to_string()
}

/// Replaces an input's text without emitting a change event.
pub fn set_input(state: &Entity<InputState>, text: &str, window: &mut Window, cx: &mut App) {
    let text = text.to_string();
    state.update(cx, |s, cx| s.set_value(text, window, cx));
}

/// Calls `f` whenever the user edits the input.
pub fn on_change<V: 'static>(
    cx: &mut Context<V>,
    state: &Entity<InputState>,
    f: impl Fn(&mut V, &mut Context<V>) + 'static,
) -> Subscription {
    cx.subscribe(state, move |this, _, e: &InputEvent, cx| {
        if matches!(e, InputEvent::Change) {
            f(this, cx);
        }
    })
}

/// A rendered input with its test id and accessibility label.
pub fn text_input(state: &Entity<InputState>, id: impl Into<ElementId>, label: &str) -> Input {
    Input::new(state)
        .id(id)
        .aria_label(label.to_string())
}

/// A controlled checkbox: the owner passes the current value and handles the change.
pub fn check(
    id: impl Into<ElementId>,
    label: &str,
    checked: bool,
    on_change: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> Checkbox {
    Checkbox::new(id)
        .label(label.to_string())
        .accessibility_label(label.to_string())
        .checked(checked)
        .on_click(move |value, window, cx| on_change(*value, window, cx))
}

/// A select: a button showing the chosen option's label that opens a menu of
/// `options` ((value, label) pairs). `on_pick` gets the chosen value.
pub fn select(
    id: impl Into<ElementId>,
    current: &str,
    options: Vec<(String, String)>,
    placeholder: &str,
    on_pick: impl Fn(&str, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let shown = options
        .iter()
        .find(|(value, _)| value == current)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| placeholder.to_string());
    let on_pick = Rc::new(on_pick);
    let current = current.to_string();
    Button::new(id)
        .outline()
        .small()
        .label(shown)
        .icon(IconName::ChevronDown)
        .dropdown_menu(move |menu, _, _| {
            let mut menu = menu;
            for (value, label) in options.clone() {
                let on_pick = on_pick.clone();
                let chosen = value == current;
                menu = menu.item(
                    PopupMenuItem::new(label)
                        .checked(chosen)
                        .on_click(move |_, window, cx| on_pick(&value, window, cx)),
                );
            }
            menu
        })
}

/// A titled block of a design form. The block's id is its title, so tests can scope
/// to it with `Harness::click_in`.
pub fn section(title: &str, cx: &App) -> Stateful<Div> {
    v_flex()
        .id(SharedString::from(title.to_string()))
        .gap_2()
        .py_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .text_base()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.to_string()),
        )
}

/// A danger-styled inline error line with a test id.
pub fn error_line(id: impl Into<ElementId>, message: &str, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .px_3()
        .py_2()
        .rounded_md()
        .bg(theme.danger.opacity(0.12))
        .text_color(theme.danger)
        .text_sm()
        .child(crate::ui::widgets::text(id, message.to_string()))
}

/// A modal panel over the whole view: a dimmed backdrop with the panel centred. The
/// parent must be `relative()`. `id` names the panel for tests.
pub fn modal(id: impl Into<ElementId>, title: &str, body: impl IntoElement, cx: &App) -> impl IntoElement {
    modal_sized(id, title, body, 640.0, cx)
}

/// `modal` with a chosen panel width in pixels.
pub fn modal_sized(
    id: impl Into<ElementId>,
    title: &str,
    body: impl IntoElement,
    width: f32,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui_kit::black().opacity(0.45))
        .occlude()
        .child(
            v_flex()
                .id(id)
                .aria_label(title.to_string())
                .w(px(width))
                .max_w_full()
                .max_h_full()
                .overflow_y_scroll()
                .gap_3()
                .p_4()
                .rounded_lg()
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .shadow_lg()
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(crate::ui::widgets::text("modal-title", title.to_string())),
                )
                .child(body)
                .test_support(),
        )
}

/// A muted note line.
pub fn note(text: impl IntoElement, cx: &App) -> Div {
    div().text_sm().text_color(cx.theme().muted_foreground).child(text)
}

/// (value, label) pairs where each value is its own label.
pub fn plain_options<S: AsRef<str>>(items: &[S]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|s| (s.as_ref().to_string(), s.as_ref().to_string()))
        .collect()
}
