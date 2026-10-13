//! Text inputs of the editor, created on demand and keyed by what they edit. An input
//! owns its text once created; edits flow into the draft through `input_changed`.
//! The cache is cleared when the draft is replaced from outside (opening another
//! query, undo), so inputs are rebuilt from the new draft.

use super::{
    QueryMode,
    model::{BuilderModel, FilterValue, Item, Literal},
};
use crate::ui::prelude::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    Name,
    /// Alias of a builder field (by field id).
    Alias(String),
    /// Default value of a parameter (by name).
    ParamDefault(String),
    /// Run-time value of a parameter (by name).
    ParamValue(String),
    NewParam,
    /// Literal of a filter condition (by condition id).
    FilterValue(String),
    RowLimit,
}

pub struct InputSlot {
    pub state: Entity<InputState>,
    _subscription: Subscription,
}

impl QueryMode {
    /// The input for `key`, created with `initial` text on first use.
    pub(super) fn input_state(
        &mut self,
        key: Field,
        initial: &str,
        placeholder: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(slot) = self.inputs.get(&key) {
            return slot.state.clone();
        }
        let state = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder.to_string())
                .default_value(initial.to_string())
        });
        let subscription = cx.subscribe_in(&state, window, {
            let key = key.clone();
            move |this, state, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let text = state.read(cx).value().to_string();
                    this.input_changed(&key, text, cx);
                }
                InputEvent::PressEnter { .. } => this.input_entered(&key, window, cx),
                _ => {}
            }
        });
        self.inputs.insert(
            key,
            InputSlot {
                state: state.clone(),
                _subscription: subscription,
            },
        );
        state
    }

    /// An input element: `id` is the test id, `label` its accessible name.
    pub(super) fn input_el(
        &mut self,
        key: Field,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        initial: &str,
        placeholder: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Input {
        let state = self.input_state(key, initial, placeholder, window, cx);
        let id: SharedString = id.into();
        Input::new(&state).id(id).aria_label(label)
    }

    /// Replaces the text of an existing input without emitting a change.
    pub(super) fn set_input(&mut self, key: &Field, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(slot) = self.inputs.get(key) {
            slot.state
                .update(cx, |s, cx| s.set_value(text.to_string(), window, cx));
        }
    }

    pub(super) fn input_text(&self, key: &Field, cx: &App) -> String {
        self.inputs
            .get(key)
            .map(|s| s.state.read(cx).value().to_string())
            .unwrap_or_default()
    }

    fn input_changed(&mut self, key: &Field, text: String, cx: &mut Context<Self>) {
        match key {
            Field::Name => self.edit_query(|q| q.name = text, cx),
            Field::Alias(id) => {
                let id = id.clone();
                self.edit_model(
                    |m| {
                        if let Some(f) = m.fields.iter_mut().find(|f| f.id == id) {
                            f.alias = Some(text);
                        }
                    },
                    cx,
                );
            }
            Field::ParamDefault(name) => {
                let name = name.clone();
                self.edit_query(
                    |q| {
                        if let Some(p) = q.parameters.iter_mut().find(|p| p.name == name) {
                            p.default_value = (!text.is_empty()).then(|| Value::String(text));
                        }
                    },
                    cx,
                );
            }
            Field::ParamValue(name) => {
                self.values.insert(name.clone(), text);
                self.parameters_changed(cx);
            }
            Field::NewParam => {
                // Names are trimmed as they are typed.
                cx.notify();
            }
            Field::FilterValue(id) => self.filter_value_typed(id, text, cx),
            Field::RowLimit => {
                let limit = if text.trim().is_empty() {
                    None
                } else {
                    text.trim().parse::<f64>().ok().map(|n| n.floor().max(0.0))
                };
                self.edit_model(|m| m.limit = limit, cx);
            }
        }
    }

    fn input_entered(&mut self, key: &Field, window: &mut Window, cx: &mut Context<Self>) {
        if *key == Field::NewParam {
            self.add_typed_parameter(window, cx);
        }
    }

    /// Typing in a filter's value box: numbers are stored as numbers for numeric columns
    /// and for aggregates (having).
    fn filter_value_typed(&mut self, id: &str, text: String, cx: &mut Context<Self>) {
        let numeric = self.condition_is_numeric(id);
        let literal = match text.trim().parse::<f64>() {
            Ok(n) if numeric && !text.trim().is_empty() && n.is_finite() => Literal::Number(n),
            _ => Literal::Text(text),
        };
        self.edit_model(
            |m| {
                for_each_condition(m, |c| {
                    if c.id == id {
                        c.value = Some(FilterValue::Value {
                            value: literal.clone(),
                        });
                    }
                })
            },
            cx,
        );
    }

    fn condition_is_numeric(&self, id: &str) -> bool {
        let Some(model) = self.model() else {
            return false;
        };
        let mut found = None;
        let mut model = model;
        for_each_condition(&mut model, |c| {
            if c.id == id {
                found = Some((c.source.clone(), c.column.clone(), c.aggregate.is_some()));
            }
        });
        let Some((source, column, aggregate)) = found else {
            return false;
        };
        aggregate || self.column_numeric(&model, &source, &column)
    }

    /// Whether a column's declared type is numeric (INT, REAL, FLOA, DOUB, NUM, DEC).
    pub(super) fn column_numeric(&self, model: &BuilderModel, source: &str, column: &str) -> bool {
        let Some(table) = model.sources.iter().find(|s| s.alias == source) else {
            return false;
        };
        self.schemas
            .get(&table.table)
            .and_then(|s| s.columns.iter().find(|c| c.name == column))
            .is_some_and(|c| {
                let t = c.declared_type.to_uppercase();
                ["INT", "REAL", "FLOA", "DOUB", "NUM", "DEC"]
                    .iter()
                    .any(|n| t.contains(n))
            })
    }
}

/// Applies `f` to every condition in `having` and `filters`.
pub fn for_each_condition(
    model: &mut BuilderModel,
    mut f: impl FnMut(&mut super::model::BuilderCondition),
) {
    fn walk(
        group: &mut super::model::BuilderGroup,
        f: &mut impl FnMut(&mut super::model::BuilderCondition),
    ) {
        for item in &mut group.items {
            match item {
                Item::Condition(c) => f(c),
                Item::Group(g) => walk(g, f),
            }
        }
    }
    walk(&mut model.filters, &mut f);
    walk(&mut model.having, &mut f);
}
