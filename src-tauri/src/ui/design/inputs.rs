//! Text inputs of the Design studio.
//!
//! - `Fields` are inputs that edit the definition as the user types (the React
//!   `value` / `onChange` pairs). The studio's model stays the source of truth: a field is
//!   brought to the model's value on render unless the user is typing in it.
//! - `Drafts` port `DraftInput.tsx`: the typed text stays a local draft and is applied on
//!   blur or Enter, so half-typed values (an empty name, the "1" of "10") never reach the
//!   stored definition. The apply handler returns a problem to show (the draft is kept) or
//!   an empty string when it applied. Escape restores the stored value.
//!
//! Test ids: a field or draft is found by the id it is built with. A draft's problem line
//! is `<id>-error`.

use crate::ui::prelude::*;
use gpui_kit::component::input::{Input, InputEvent, InputState, TextareaState};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

/// Receives the text typed into a field.
type Typed<V> = Rc<dyn Fn(&mut V, String, &mut Window, &mut Context<V>)>;

struct Cell<V: 'static> {
    state: Entity<InputState>,
    handler: Rc<RefCell<Typed<V>>>,
    _subscription: Subscription,
}

struct AreaCell<V: 'static> {
    state: Entity<TextareaState>,
    handler: Rc<RefCell<Typed<V>>>,
    _subscription: Subscription,
}

/// Inputs that edit as the user types, by key. Created on first use.
pub struct Fields<V: 'static> {
    cells: HashMap<String, Cell<V>>,
    areas: HashMap<String, AreaCell<V>>,
}

impl<V: 'static> Default for Fields<V> {
    fn default() -> Self {
        Self { cells: HashMap::new(), areas: HashMap::new() }
    }
}

impl<V: 'static> Fields<V> {
    /// The input for `key`. A new one starts with `value`; an existing one is brought to
    /// `value` unless the user is typing in it. `on_change` gets each edit.
    pub fn get(
        &mut self,
        key: &str,
        value: &str,
        placeholder: &str,
        on_change: impl Fn(&mut V, String, &mut Window, &mut Context<V>) + 'static,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> Entity<InputState> {
        let on_change: Typed<V> = Rc::new(on_change);
        if let Some(cell) = self.cells.get(key) {
            *cell.handler.borrow_mut() = on_change;
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
        let state = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).default_value(initial));
        let handler = Rc::new(RefCell::new(on_change));
        let subscription = {
            let handler = handler.clone();
            cx.subscribe_in(&state, window, move |this, state, e: &InputEvent, window, cx| {
                if matches!(e, InputEvent::Change) {
                    let text = state.read(cx).value().to_string();
                    let f = handler.borrow().clone();
                    f(this, text, window, cx);
                }
            })
        };
        self.cells.insert(key.to_string(), Cell { state: state.clone(), handler, _subscription: subscription });
        state
    }

    /// The multi-line input for `key`; otherwise like `get`.
    pub fn area(
        &mut self,
        key: &str,
        value: &str,
        on_change: impl Fn(&mut V, String, &mut Window, &mut Context<V>) + 'static,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> Entity<TextareaState> {
        let on_change: Typed<V> = Rc::new(on_change);
        if let Some(cell) = self.areas.get(key) {
            *cell.handler.borrow_mut() = on_change;
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
        let initial = value.to_string();
        let state = cx.new(|cx| TextareaState::new(window, cx).default_value(initial));
        let handler = Rc::new(RefCell::new(on_change));
        let subscription = {
            let handler = handler.clone();
            cx.subscribe_in(&state, window, move |this, state, e: &InputEvent, window, cx| {
                if matches!(e, InputEvent::Change) {
                    let text = state.read(cx).value().to_string();
                    let f = handler.borrow().clone();
                    f(this, text, window, cx);
                }
            })
        };
        self.areas.insert(key.to_string(), AreaCell { state: state.clone(), handler, _subscription: subscription });
        state
    }
}

#[cfg(test)]
impl<V: 'static> Fields<V> {
    /// The text of the input or multi-line input `key`.
    pub fn value(&self, key: &str, cx: &App) -> Option<String> {
        self.cells
            .get(key)
            .map(|c| c.state.read(cx).value().to_string())
            .or_else(|| self.areas.get(key).map(|c| c.state.read(cx).value().to_string()))
    }
}

/// Applies a draft; returns the problem to show, or "" when it applied.
type Apply<V> = Rc<dyn Fn(&mut V, String, &mut Window, &mut Context<V>) -> String>;

#[derive(Default)]
struct Shared {
    // The stored value of the latest render.
    stored: String,
    // The stored value the input text was last brought to; None forces a refresh.
    synced: Option<String>,
    problem: String,
}

struct DraftCell<V: 'static> {
    state: Entity<InputState>,
    apply: Rc<RefCell<Apply<V>>>,
    shared: Rc<RefCell<Shared>>,
    _subscriptions: Vec<Subscription>,
}

/// Inputs applied on blur or Enter, by key.
pub struct Drafts<V: 'static> {
    cells: HashMap<String, DraftCell<V>>,
}

impl<V: 'static> Default for Drafts<V> {
    fn default() -> Self {
        Self { cells: HashMap::new() }
    }
}

#[cfg(test)]
impl<V: 'static> Drafts<V> {
    /// The text of the draft input `id`.
    pub fn value(&self, id: &str, cx: &App) -> Option<String> {
        self.cells.get(id).map(|c| c.state.read(cx).value().to_string())
    }
}

fn commit<V: 'static>(
    this: &mut V,
    state: &Entity<InputState>,
    apply: &Rc<RefCell<Apply<V>>>,
    shared: &Rc<RefCell<Shared>>,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    let draft = state.read(cx).value().to_string();
    if draft == shared.borrow().stored {
        shared.borrow_mut().problem.clear();
        cx.notify();
        return;
    }
    let apply = apply.borrow().clone();
    let problem = apply(this, draft, window, cx);
    let mut shared = shared.borrow_mut();
    if problem.is_empty() {
        // A clamped value equal to the stored one would leave the typed text showing.
        shared.synced = None;
    }
    shared.problem = problem;
    cx.notify();
}

impl<V: 'static> Drafts<V> {
    /// A labeled draft input found by `id` (key and id are the same), with its problem line.
    pub fn field(
        &mut self,
        id: &str,
        label: &str,
        value: &str,
        placeholder: &str,
        disabled: bool,
        apply: impl Fn(&mut V, String, &mut Window, &mut Context<V>) -> String + 'static,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> Div {
        let apply: Apply<V> = Rc::new(apply);
        if !self.cells.contains_key(id) {
            let placeholder = placeholder.to_string();
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            let apply_cell = Rc::new(RefCell::new(apply.clone()));
            let shared = Rc::new(RefCell::new(Shared::default()));
            let subscription = {
                let (apply, shared) = (apply_cell.clone(), shared.clone());
                cx.subscribe_in(&state, window, move |this, state, e: &InputEvent, window, cx| {
                    if matches!(e, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                        commit(this, state, &apply, &shared, window, cx);
                    }
                })
            };
            self.cells.insert(
                id.to_string(),
                DraftCell { state, apply: apply_cell, shared, _subscriptions: vec![subscription] },
            );
        }
        let cell = &self.cells[id];
        *cell.apply.borrow_mut() = apply;
        let state = cell.state.clone();
        let shared = cell.shared.clone();
        {
            let mut s = shared.borrow_mut();
            s.stored = value.to_string();
            if s.synced.as_deref() != Some(value) {
                state.update(cx, |input, cx| input.set_value(value.to_string(), window, cx));
                s.synced = Some(value.to_string());
                s.problem.clear();
            }
        }
        let problem = shared.borrow().problem.clone();
        let danger = cx.theme().danger;
        let on_key = {
            let (state, shared) = (state.clone(), shared.clone());
            move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                if event.keystroke.key == "escape" {
                    let stored = shared.borrow().stored.clone();
                    state.update(cx, |input, cx| input.set_value(stored, window, cx));
                    shared.borrow_mut().problem.clear();
                    window.refresh();
                }
            }
        };
        let input = Input::new(&state).id(SharedString::from(id.to_string())).aria_label(label.to_string());
        widgets::field(
            label,
            v_flex()
                .gap_0p5()
                .child(div().on_key_down(on_key).opacity(if disabled { 0.5 } else { 1.0 }).child(input.disabled(disabled)))
                .when(!problem.is_empty(), |el| {
                    el.child(widgets::styled_text(
                        SharedString::from(format!("{id}-error")),
                        problem,
                        move |d| d.text_xs().text_color(danger),
                    ))
                }),
            cx,
        )
    }
}
