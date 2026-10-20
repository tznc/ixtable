//! Small form controls shared by the design views: text inputs backed by `InputState`
//! (loose, or cached by key in `Inputs`), checkboxes, and selects built from a dropdown
//! menu.
//!
//! Test ids: every control takes the id its view gives it. An input is found by that id;
//! a select or choice is a button with that id whose options are the items (by index) of
//! the `popup-menu` it opens; a problem line under an expression is `<id>-problem`.

use crate::ui::prelude::*;
use gpui_kit::component::{
    checkbox::Checkbox,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
};
use std::{collections::HashMap, hash::Hash, rc::Rc};

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

/// A select: a button showing the chosen option's label (left-aligned, chevron at the
/// right edge) that opens a menu of `options` ((value, label) pairs). `on_pick` gets the
/// chosen value.
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
        .map_or_else(|| placeholder.to_string(), |(_, label)| label.clone());
    let on_pick = Rc::new(on_pick);
    let current = current.to_string();
    Button::new(id)
        .outline()
        .small()
        .end_icon_label(IconName::ChevronDown, shown)
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

/// A `select` that keeps a value outside `options` visible as `Missing (value)`.
pub fn choice(
    id: impl Into<ElementId>,
    current: &str,
    mut options: Vec<(String, String)>,
    placeholder: &str,
    on_pick: impl Fn(&str, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    if !current.is_empty() && !options.iter().any(|(v, _)| v == current) {
        options.push((current.to_string(), format!("Missing ({current})")));
    }
    select(id, current, options, placeholder, on_pick)
}

type Edit<V, K> = fn(&mut V, &K, String, &mut Context<V>);

struct Cell {
    state: Entity<InputState>,
    _subscription: Subscription,
}

/// Text inputs of a view, by key. Created on first use.
pub struct Inputs<V: 'static, K: Clone + Eq + Hash + 'static> {
    cells: HashMap<K, Cell>,
    on_edit: Edit<V, K>,
}

impl<V: 'static, K: Clone + Eq + Hash + 'static> Inputs<V, K> {
    pub fn new(on_edit: Edit<V, K>) -> Self {
        Self {
            cells: HashMap::new(),
            on_edit,
        }
    }

    /// The input for `key`. A new one starts with `value`; an existing one is brought to
    /// `value` unless the user is typing in it.
    pub fn get(
        &mut self,
        key: K,
        value: &str,
        placeholder: &str,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> Entity<InputState> {
        if let Some(cell) = self.cells.get(&key) {
            let state = cell.state.clone();
            let (focused, shown) = {
                let s = state.read(cx);
                (s.focus_handle(cx).is_focused(window), s.value().to_string())
            };
            if !focused && shown != value {
                state.update(cx, |s, cx| s.set_value(value.to_string(), window, cx));
            }
            return state;
        }
        let (placeholder, initial) = (placeholder.to_string(), value.to_string());
        let state = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(initial)
        });
        let (edited, handler) = (key.clone(), self.on_edit);
        let subscription = cx.subscribe(&state, move |this, state, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                let text = state.read(cx).value().to_string();
                handler(this, &edited, text, cx);
            }
        });
        self.cells.insert(
            key,
            Cell {
                state: state.clone(),
                _subscription: subscription,
            },
        );
        state
    }

    /// The text of the input for `key` right now.
    pub fn text(&self, key: &K, cx: &App) -> Option<String> {
        self.cells.get(key).map(|c| c.state.read(cx).value().to_string())
    }
}

/// A single-line input rendered with its test id and accessibility label.
pub fn input(state: &Entity<InputState>, id: impl Into<ElementId>, label: &str, code: bool) -> impl IntoElement {
    let input = Input::new(state).id(id).aria_label(label.to_string());
    if code {
        div().font_family("monospace").child(input)
    } else {
        div().child(input)
    }
}

/// A labeled text input.
pub fn text_field(
    id: impl Into<ElementId>,
    label: &str,
    state: &Entity<InputState>,
    cx: &App,
) -> Div {
    widgets::field(label, input(state, id, label, false), cx).min_w(px(180.))
}

/// A labeled expression input with its live problem line (`<id>-problem`).
pub fn expr_field(
    id: &str,
    label: &str,
    state: &Entity<InputState>,
    problem: Option<String>,
    cx: &App,
) -> Div {
    let danger = cx.theme().danger;
    widgets::field(label, input(state, id.to_string(), label, true), cx)
        .min_w(px(220.))
        .when_some(problem, |el, p| {
            el.child(widgets::styled_text(
                SharedString::from(format!("{id}-problem")),
                p,
                move |d| d.ty(Type::Caption).text_color(danger),
            ))
        })
}

/// A labeled choice.
pub fn choice_field(
    id: impl Into<ElementId>,
    label: &str,
    current: &str,
    options: Vec<(String, String)>,
    placeholder: &str,
    on_pick: impl Fn(&str, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    widgets::field(label, choice(id, current, options, placeholder, on_pick), cx)
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
                .ty(Type::Section)
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
        .ty(Type::Body)
        .child(crate::ui::widgets::text(id, message.to_string()))
}

/// A muted note line.
pub fn note(text: impl IntoElement, cx: &App) -> Div {
    div().ty(Type::Body).text_color(cx.theme().muted_foreground).child(text)
}

/// (value, label) pairs where each value is its own label.
pub fn plain_options<S: AsRef<str>>(items: &[S]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|s| (s.as_ref().to_string(), s.as_ref().to_string()))
        .collect()
}
