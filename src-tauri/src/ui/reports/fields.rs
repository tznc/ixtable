//! Form controls of the reports views. Same shape as the automation editors' controls:
//! text inputs are `InputState` entities kept in an `Inputs` cache owned by the editing
//! view, whose model stays the source of truth.
//!
//! Test ids: every control takes the id its view gives it. An input is found by that id;
//! a choice is a button with that id whose options are the items (by index) of the
//! `popup-menu` it opens; a problem line under an expression is `<id>-problem`.

use crate::ui::prelude::*;
use gpui_kit::component::{
    checkbox::Checkbox,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
};
use std::{collections::HashMap, hash::Hash, rc::Rc};

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

/// A labeled expression input with its live problem line (`<id>-problem`).
pub fn expr_field(
    id: &str,
    label: &str,
    state: &Entity<InputState>,
    problem: Option<String>,
    cx: &App,
) -> Div {
    let danger = cx.theme().danger;
    widgets::field(label, input(state, id.to_string(), label, true), cx).when_some(problem, |el, p| {
        el.child(widgets::styled_text(
            SharedString::from(format!("{id}-problem")),
            p,
            move |d| d.text_xs().text_color(danger),
        ))
    })
}

/// A controlled checkbox: the owner passes the value and handles the change.
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

/// A choice: a button showing the chosen option's label that opens a menu of `options`
/// ((value, label) pairs). A value that is not an option shows as `Missing (value)`.
pub fn choice(
    id: impl Into<ElementId>,
    current: &str,
    options: Vec<(String, String)>,
    placeholder: &str,
    on_pick: impl Fn(&str, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let mut options = options;
    if !current.is_empty() && !options.iter().any(|(v, _)| v == current) {
        options.push((current.to_string(), format!("Missing ({current})")));
    }
    let shown = options
        .iter()
        .find(|(value, _)| value == current)
        .map_or_else(|| placeholder.to_string(), |(_, label)| label.clone());
    let on_pick = Rc::new(on_pick);
    let current = current.to_string();
    Button::new(id)
        .outline()
        .small()
        .label(shown)
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

/// (value, label) pairs from `(value, label)` string slices.
pub fn pairs(values: &[(&str, &str)]) -> Vec<(String, String)> {
    values.iter().map(|(v, l)| (v.to_string(), l.to_string())).collect()
}

/// A section: a bold legend over its controls.
pub fn section(id: impl Into<ElementId>, legend: &str, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    v_flex()
        .id(id)
        .gap_1p5()
        .p_2()
        .border_1()
        .border_color(theme.border)
        .rounded_md()
        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(legend.to_string()))
}
