//! Selects a saved query by id. Port of src/query/QueryPicker.tsx; other modes (data
//! sources, automation steps) embed it.
//!
//! ```ignore
//! let picker = cx.new(|cx| QueryPicker::new("source-query", shell.store.clone(), cx)
//!     .label("Query").value(current));
//! cx.subscribe(&picker, |this, _, QueryPickerEvent::Change(id), cx| ...);
//! ```
//!
//! Test ids: the trigger has the `id` given to `new`; option rows are `<id>/<query id>`,
//! with `<id>/` for "No query".

use super::choice::{Choice, Opt};
use crate::{
    archive::SavedQuery,
    ui::{prelude::*, store::ConfigStore},
};

pub enum QueryPickerEvent {
    /// The chosen query id ("" for none).
    Change(String),
}

pub struct QueryPicker {
    id: SharedString,
    label: SharedString,
    value: String,
    allow_none: bool,
    disabled: bool,
    actions: bool,
    store: Entity<ConfigStore>,
    open: bool,
    _subscription: Subscription,
}

impl EventEmitter<QueryPickerEvent> for QueryPicker {}

impl QueryPicker {
    pub fn new(id: impl Into<SharedString>, store: Entity<ConfigStore>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&store, |_, _, cx| cx.notify());
        Self {
            id: id.into(),
            label: "Query".into(),
            value: String::new(),
            allow_none: true,
            disabled: false,
            actions: false,
            store,
            open: false,
            _subscription: subscription,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    /// The selected query id ("" for none).
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    /// Offer "No query" (default true).
    pub fn allow_none(mut self, allow: bool) -> Self {
        self.allow_none = allow;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// List action queries too (automation steps); data sources read rows, so they never do.
    pub fn actions(mut self, actions: bool) -> Self {
        self.actions = actions;
        self
    }

    pub fn selected(&self) -> &str {
        &self.value
    }

    pub fn set_value(&mut self, value: impl Into<String>, cx: &mut Context<Self>) {
        self.value = value.into();
        cx.notify();
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.disabled = disabled;
        cx.notify();
    }

    fn queries<'a>(&self, cx: &'a App) -> Vec<&'a SavedQuery> {
        self.store
            .read(cx)
            .config()
            .saved_queries
            .iter()
            .filter(|q| self.actions || q.action.is_none())
            .collect()
    }

    /// The options as shown: None, a "Missing query" for a dangling id, then the queries.
    pub fn options(&self, cx: &App) -> Vec<Opt> {
        let queries = self.queries(cx);
        let mut options = vec![];
        if self.allow_none {
            options.push(Opt::new("", "No query"));
        }
        if !self.value.is_empty() && !queries.iter().any(|q| q.id == self.value) {
            options.push(Opt::new(self.value.clone(), "Missing query"));
        }
        options.extend(queries.iter().map(|q| {
            Opt::new(
                q.id.clone(),
                if q.name.is_empty() { "Untitled query" } else { &q.name },
            )
        }));
        options
    }
}

impl Render for QueryPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let options = self.options(cx);
        let choice = Choice::new(
            self.id.clone(),
            self.value.clone(),
            options,
            self.open,
            |this: &mut Self, open, cx| {
                this.open = open;
                cx.notify();
            },
            |this: &mut Self, value, _, cx| {
                this.value = value.clone();
                cx.emit(QueryPickerEvent::Change(value));
                cx.notify();
            },
        )
        .disabled(self.disabled)
        .render(cx);
        h_flex()
            .gap_2()
            .items_center()
            .child(div().ty(Type::Body).child(self.label.clone()))
            .child(choice)
    }
}
