//! Design-time preview of the selected form in one mode, with live data (port of
//! `DesignPreview.tsx`). It renders the Runtime's `FormRenderer` detached from Run mode, so
//! what the designer shows is what the application runs.
//!
//! Test ids: `design-preview` (labelled "<form name> preview"), `preview-loading`,
//! `preview-empty` ("No live records to display."); the renderer brings its own ids.

use crate::design::{Form, FormMode};
use crate::forms::data::{first_record_id, source_params};
use crate::ui::{
    prelude::*,
    runtime::{FormRenderer, FormRendererProps, RuntimeEnv},
    shell::Shell,
    store::StoreEvent,
};
use serde_json::{Map, Value};

enum State {
    Loading,
    Empty,
    Ready(Entity<FormRenderer>),
}

pub struct DesignPreview {
    shell: Entity<Shell>,
    form_id: String,
    mode: FormMode,
    state: State,
    // The form and app scope the shown record was chosen for.
    loaded: Option<(Form, Map<String, Value>)>,
    seq: u64,
    _subscription: Subscription,
}

fn needs_record(mode: FormMode) -> bool {
    matches!(mode, FormMode::Detail | FormMode::Edit)
}

impl DesignPreview {
    pub fn new(shell: Entity<Shell>, form_id: &str, mode: FormMode, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = shell.read(cx).store.clone();
        let subscription = cx.subscribe_in(&store, window, |this, _, e: &StoreEvent, window, cx| {
            if matches!(e, StoreEvent::Changed) {
                this.reload_if_changed(window, cx);
            }
        });
        let mut this = Self {
            shell,
            form_id: form_id.to_string(),
            mode,
            state: State::Loading,
            loaded: None,
            seq: 0,
            _subscription: subscription,
        };
        this.load(window, cx);
        this
    }

    pub fn form_id(&self) -> &str {
        &self.form_id
    }

    pub fn mode(&self) -> FormMode {
        self.mode
    }

    /// The renderer once the preview is ready.
    pub fn renderer(&self) -> Option<Entity<FormRenderer>> {
        match &self.state {
            State::Ready(renderer) => Some(renderer.clone()),
            _ => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self.state, State::Empty)
    }

    fn form(&self, cx: &App) -> Option<Form> {
        self.shell.read(cx).config(cx).design.forms.iter().find(|f| f.id == self.form_id).cloned()
    }

    fn env(&self, cx: &App) -> RuntimeEnv {
        let shell = self.shell.read(cx);
        RuntimeEnv { label: shell.label().to_string().into(), store: shell.store.clone(), bus: shell.bus.clone(), runtime: None }
    }

    fn scope(&self, cx: &App) -> Map<String, Value> {
        let mut scope = Map::new();
        scope.insert("app".into(), Value::Object(self.env(cx).app(cx)));
        scope.insert("params".into(), Value::Object(Map::new()));
        scope
    }

    fn reload_if_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !needs_record(self.mode) {
            return;
        }
        let now = self.form(cx).map(|form| (form, self.scope(cx)));
        if now != self.loaded {
            self.load(window, cx);
        }
    }

    fn show(&mut self, record_id: Option<Value>, window: &mut Window, cx: &mut Context<Self>) {
        let mut props = FormRendererProps::new(self.env(cx), self.form_id.clone());
        props.mode = Some(self.mode);
        props.record_id = record_id;
        self.state = State::Ready(cx.new(|cx| FormRenderer::new(props, window, cx)));
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.seq += 1;
        let seq = self.seq;
        let Some(form) = self.form(cx) else {
            self.state = State::Empty;
            return;
        };
        let scope = self.scope(cx);
        self.loaded = Some((form.clone(), scope.clone()));
        if !needs_record(self.mode) {
            self.show(None, window, cx);
            return;
        }
        self.state = State::Loading;
        cx.notify();
        let label = self.shell.read(cx).label().to_string();
        task::spawn_call_in(
            window,
            cx,
            move || {
                let bound = source_params(&form, &scope).map_err(|m| m)?;
                first_record_id(&label, &form, &bound, &scope).map_err(|e| e.message)
            },
            move |this, result, window, cx| {
                if this.seq != seq {
                    return;
                }
                match result {
                    Ok(Some(id)) => this.show(Some(id), window, cx),
                    _ => this.state = State::Empty,
                }
                cx.notify();
            },
        );
    }
}

impl Render for DesignPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self.form(cx).map(|f| f.name).unwrap_or_default();
        let muted = cx.theme().muted_foreground;
        let body = match &self.state {
            State::Loading => widgets::styled_text("preview-loading", "Loading preview…", move |d| d.text_color(muted))
                .into_any_element(),
            State::Empty => widgets::styled_text("preview-empty", "No live records to display.", move |d| {
                d.text_color(muted)
            })
            .into_any_element(),
            State::Ready(renderer) => renderer.clone().into_any_element(),
        };
        v_flex()
            .id("design-preview")
            .aria_label(format!("{name} preview"))
            .w_full()
            .p_3()
            .child(body)
            .test_support()
    }
}
