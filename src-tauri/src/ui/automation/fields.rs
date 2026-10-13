//! Form controls of the automation editors (port of `src/automation/fields.tsx`).
//!
//! Text inputs are `InputState` entities kept in an `Inputs` cache owned by the editing
//! view. The view's data stays the source of truth: every `get` shows the model's text in
//! an input that is not being edited, and an edit calls the view's handler with the key it
//! was created for.
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

/// Root names an action or trigger expression may use.
pub const AUTOMATION_SCOPE_NAMES: [&str; 8] =
    ["record", "old", "form", "app", "params", "results", "steps", "trigger"];

/// The first problem of an expression, if any (`required` makes an empty one a problem).
pub fn expression_problem(value: &str, required: bool) -> Option<String> {
    if value.trim().is_empty() {
        return required.then(|| "Expression is required".to_string());
    }
    crate::expr::check(value, Some(&AUTOMATION_SCOPE_NAMES))
        .into_iter()
        .next()
        .map(|d| d.message)
}

/// The problem of a whole-number input (`None` when `text` is a whole number >= `min`).
pub fn number_problem(text: &str, min: i64) -> Option<String> {
    match parse_whole(text, min) {
        Some(_) => None,
        None => Some(format!("Enter a whole number of at least {min}")),
    }
}

/// `text` as a whole number of at least `min`.
pub fn parse_whole(text: &str, min: i64) -> Option<i64> {
    text.trim().parse::<i64>().ok().filter(|n| *n >= min)
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
/// `on_pick` gets the chosen value.
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

/// (value, label) pairs where each value is its own label.
pub fn plain(values: &[&str]) -> Vec<(String, String)> {
    values.iter().map(|v| (v.to_string(), v.to_string())).collect()
}

#[cfg(test)]
mod tests {
    use super::{expression_problem, number_problem, parse_whole};

    #[test]
    fn empty_expressions_are_a_problem_only_when_required() {
        assert_eq!(expression_problem("  ", true).as_deref(), Some("Expression is required"));
        assert_eq!(expression_problem("", false), None);
    }

    #[test]
    fn expressions_report_their_first_diagnostic() {
        assert_eq!(
            expression_problem("'closed' +", true).as_deref(),
            Some("Unexpected end of expression")
        );
        assert_eq!(expression_problem("record.status = 'open'", true), None);
        assert!(expression_problem("nope.value", true).is_some(), "unknown root name");
    }

    #[test]
    fn whole_numbers_respect_the_minimum() {
        assert_eq!(parse_whole(" 3 ", 1), Some(3));
        assert_eq!(parse_whole("0", 1), None);
        assert_eq!(parse_whole("1.5", 0), None);
        assert_eq!(parse_whole("", 0), None);
        assert_eq!(number_problem("x", 1).as_deref(), Some("Enter a whole number of at least 1"));
        assert_eq!(number_problem("600000", 0), None);
    }
}
