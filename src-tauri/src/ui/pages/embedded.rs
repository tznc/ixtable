//! A form placed on a dashboard (port of `EmbeddedForm.tsx`). Renders the component's form
//! with the dashboard `params` in scope. Detail and edit modes open the record named by
//! `recordId` (an expression over `params` and `app`), or the first row of the form's source
//! when it is blank; that row is looked up again after record writes, but only while the form
//! has no unsaved edits, and a shown table record that was deleted gets a message instead of
//! a switch. The form's row filter holds here too (`page_engine::binding`).
//!
//! Test ids: the messages `embed-missing`, `embed-unsupported`, `embed-loading`,
//! `embed-empty`, `embed-gone` (with the `embed-show-first` button), `embed-outside` and
//! `embed-error`; the form itself is a `FormRenderer`, whose elements share ids across
//! embedded forms, so tests look them up inside the widget (`widget:<title>`).

use crate::{
    page_engine::{
        binding::{effective_expression, form_table, resolve, Resolved},
        model::embeddable_modes,
    },
    pages::PageComponent,
    design::FormMode,
    forms::registry::resolve_form,
    ui::{
        events::DocEvent,
        prelude::*,
        runtime::{message, FormRenderer, FormRendererProps, RuntimeEnv},
    },
};
use serde_json::{Map, Value};
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Debug, PartialEq)]
pub enum Binding {
    Loading,
    /// The record to show (None for modes that need none).
    Ready(Option<Value>),
    Empty,
    Gone,
    Outside,
    Error(String),
}

pub struct EmbeddedForm {
    env: RuntimeEnv,
    form_id: String,
    mode: Option<FormMode>,
    record_id: Option<String>,
    params: Map<String, Value>,
    pub binding: Binding,
    renderer: Option<Entity<FormRenderer>>,
    version: u64,
    generation: u64,
    // Set by the form while it has unsaved edits; a record change then waits until it is clean.
    dirty: Rc<Cell<bool>>,
    stale: Rc<Cell<bool>>,
    // The first row shown for the current parameters.
    shown: Option<Value>,
    started: bool,
    _subscription: Subscription,
}

impl EmbeddedForm {
    pub fn new(env: RuntimeEnv, component: &PageComponent, params: Map<String, Value>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(&env.bus.clone(), |this, _, _: &DocEvent, cx| this.records_changed(cx));
        Self {
            env,
            form_id: component.form_id.clone().unwrap_or_default(),
            mode: component.mode,
            record_id: component.record_id.clone(),
            params,
            binding: Binding::Loading,
            renderer: None,
            version: 0,
            generation: 0,
            dirty: Rc::default(),
            stale: Rc::default(),
            shown: None,
            started: false,
            _subscription: subscription,
        }
    }

    /// The component's settings changed (the Studio's View tab shows edits live).
    pub fn set_component(&mut self, component: &PageComponent, params: Map<String, Value>, cx: &mut Context<Self>) {
        let (form_id, mode, record_id) = (component.form_id.clone().unwrap_or_default(), component.mode, component.record_id.clone());
        if form_id == self.form_id && mode == self.mode && record_id == self.record_id && params == self.params {
            return;
        }
        // New filter values pick a new first row; a deleted record only matters for the same values.
        if params != self.params || form_id != self.form_id {
            self.shown = None;
        }
        self.form_id = form_id;
        self.mode = mode;
        self.record_id = record_id;
        self.params = params;
        self.started = false;
        cx.notify();
    }

    /// The renderer of the form, once built.
    pub fn renderer(&self) -> Option<Entity<FormRenderer>> {
        self.renderer.clone()
    }

    fn form(&self, cx: &App) -> Option<crate::design::Form> {
        resolve_form(self.env.config(cx), &self.form_id)
    }

    pub fn shown_mode(&self, cx: &App) -> FormMode {
        let form = self.form(cx);
        self.mode.or_else(|| form.as_ref().and_then(|f| f.modes.first().copied())).unwrap_or_default()
    }

    fn needs_record(&self, cx: &App) -> bool {
        matches!(self.shown_mode(cx), FormMode::Detail | FormMode::Edit)
    }

    fn expression(&self, cx: &App) -> String {
        self.form(cx).map_or(String::new(), |f| effective_expression(&f, self.record_id.as_deref()).to_string())
    }

    fn records_changed(&mut self, cx: &mut Context<Self>) {
        // A named record only needs a recheck when a write can move it out of the row filter.
        let form = self.form(cx);
        let filtered = form.as_ref().is_some_and(|f| f.filter.as_deref().is_some_and(|x| !x.trim().is_empty()));
        if !self.needs_record(cx) || (!self.expression(cx).is_empty() && !filtered) {
            return;
        }
        // Pin the record while it has unsaved edits; re-resolve once it is clean.
        if self.dirty.get() {
            self.stale.set(true);
        } else {
            self.version += 1;
            self.started = false;
            cx.notify();
        }
    }

    fn set_dirty(&mut self, dirty: bool, cx: &mut Context<Self>) {
        self.dirty.set(dirty);
        if !dirty && self.stale.replace(false) {
            self.version += 1;
            self.started = false;
            cx.notify();
        }
    }

    fn show_first(&mut self, cx: &mut Context<Self>) {
        self.shown = None;
        self.version += 1;
        self.started = false;
        cx.notify();
    }

    /// Looks up the record to show (on a worker thread).
    fn resolve(&mut self, cx: &mut Context<Self>) {
        self.started = true;
        self.generation += 1;
        let generation = self.generation;
        let Some(form) = self.form(cx) else { return };
        if !self.needs_record(cx) {
            self.binding = Binding::Ready(None);
            return;
        }
        if !embeddable_modes(Some(&form)).contains(&self.shown_mode(cx)) {
            return;
        }
        let expression = self.expression(cx);
        let label = self.env.label.to_string();
        let mut scope = Map::new();
        scope.insert("app".into(), Value::Object(self.env.app(cx)));
        scope.insert("params".into(), Value::Object(self.params.clone()));
        let previous = self.shown.clone();
        if !matches!(self.binding, Binding::Ready(_)) {
            self.binding = Binding::Loading;
        }
        task::spawn_call(
            cx,
            move || resolve(&label, &form, &expression, previous.as_ref(), &scope),
            move |this, result, cx| {
                if this.generation != generation {
                    return;
                }
                this.binding = match result {
                    Ok(Resolved::Ready(id)) => {
                        if this.expression(cx).is_empty() {
                            this.shown = Some(id.clone());
                        }
                        Binding::Ready(Some(id))
                    }
                    Ok(Resolved::Empty) => Binding::Empty,
                    Ok(Resolved::Gone) => Binding::Gone,
                    Ok(Resolved::Outside) => Binding::Outside,
                    Err(message) => Binding::Error(message),
                };
                cx.notify();
            },
        );
    }

    fn renderer_for(&mut self, record_id: Option<Value>, mode: FormMode, cx: &mut Context<Self>, window: &mut Window) -> Entity<FormRenderer> {
        let this = cx.weak_entity();
        let mut props = FormRendererProps::new(self.env.clone(), self.form_id.clone());
        props.mode = Some(mode);
        props.record_id = record_id;
        props.params = self.params.clone();
        props.on_dirty = Some(Rc::new(move |dirty, cx| {
            let _ = this.update(cx, |t, cx| t.set_dirty(dirty, cx));
        }));
        match &self.renderer {
            Some(renderer) => {
                renderer.update(cx, |r, cx| r.set_props(props, cx));
                renderer.clone()
            }
            None => {
                let renderer = cx.new(|cx| FormRenderer::new(props, window, cx));
                self.renderer = Some(renderer.clone());
                renderer
            }
        }
    }
}

impl Render for EmbeddedForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(form) = self.form(cx) else {
            return message("embed-missing", "This form does not exist.", true, cx).into_any_element();
        };
        let mode = self.shown_mode(cx);
        if !embeddable_modes(Some(&form)).contains(&mode) {
            let text = format!("{} cannot open in {} mode.", form.name, crate::ui::pages::mode_name(mode));
            return message("embed-unsupported", text, true, cx).into_any_element();
        }
        if !self.started {
            self.resolve(cx);
        }
        if !self.needs_record(cx) {
            return self.renderer_for(None, mode, cx, window).into_any_element();
        }
        match self.binding.clone() {
            Binding::Loading => message("embed-loading", "Loading…", false, cx).into_any_element(),
            Binding::Empty => message("embed-empty", "No record to show.", false, cx).into_any_element(),
            Binding::Gone => h_flex()
                .gap_2()
                .items_center()
                .child(message("embed-gone", "The record shown here was deleted.", true, cx))
                .child(
                    Button::new("embed-show-first")
                        .outline()
                        .small()
                        .label("Show first record")
                        .on_click(cx.listener(|this, _, _, cx| this.show_first(cx))),
                )
                .into_any_element(),
            Binding::Outside => message("embed-outside", "This record is outside the form's filter.", true, cx).into_any_element(),
            Binding::Error(text) => message("embed-error", text, true, cx).into_any_element(),
            Binding::Ready(id) => self.renderer_for(id, mode, cx, window).into_any_element(),
        }
    }
}

/// Whether the form reads a table (so a record id expression applies).
pub fn table_sourced(form: &crate::design::Form) -> bool {
    form_table(form).is_some()
}
