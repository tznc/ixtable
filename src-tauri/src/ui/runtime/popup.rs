//! Popup forms (port of `PopupHost.tsx` and `popup.ts`): an `openForm` step with `popup`
//! shows the form in a modal over the running app, newest on top. A popup closes with the
//! record it saved, with a `closeForm` step's value, or with nothing when dismissed (Close).
//! The waiting action gets the value through the `Runtime`'s popup list (see `host.rs`).
//!
//! Test ids: `popup:<n>` (labelled with the form name; n counts popups opened in the
//! Runtime), `popup-close:<n>` ("Close <form name>").

use super::{
    Runtime, RuntimeEnv,
    stack::{FormRenderer, FormRendererProps},
};
use crate::forms::registry::resolve_form;
use crate::ui::prelude::*;
use serde_json::Value;
use std::collections::HashMap;
use std::rc::Rc;

pub struct PopupHost {
    env: RuntimeEnv,
    runtime: Entity<Runtime>,
    shown: HashMap<u64, Entity<FormRenderer>>,
    _observer: Subscription,
}

impl PopupHost {
    pub fn new(env: RuntimeEnv, runtime: Entity<Runtime>, cx: &mut Context<Self>) -> Self {
        let observer = cx.observe(&runtime, |_, _, cx| cx.notify());
        Self { env, runtime, shown: HashMap::new(), _observer: observer }
    }

    /// How many popups are open.
    pub fn count(&self, cx: &App) -> usize {
        self.runtime.read(cx).popups.len()
    }
}

impl Render for PopupHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let open: Vec<(u64, crate::forms::actions::NavigationTarget)> =
            self.runtime.read(cx).popups.iter().map(|p| (p.id, p.target.clone())).collect();
        self.shown.retain(|id, _| open.iter().any(|(open, _)| open == id));
        if open.is_empty() {
            return div().into_any_element();
        }
        let theme = cx.theme().clone();
        let mut layers = vec![];
        for (id, target) in &open {
            let id = *id;
            let name = resolve_form(self.env.config(cx), &target.id).map_or_else(|| "Form".to_string(), |f| f.name);
            if !self.shown.contains_key(&id) {
                let runtime = self.runtime.clone();
                let (done_to, saved_to) = (runtime.clone(), runtime);
                let mode = match target.mode.as_deref() {
                    Some("detail") => Some(crate::design::FormMode::Detail),
                    Some("create") => Some(crate::design::FormMode::Create),
                    Some("edit") => Some(crate::design::FormMode::Edit),
                    Some("list") => Some(crate::design::FormMode::List),
                    _ => None,
                };
                let mut props = FormRendererProps::new(self.env.clone(), target.id.clone());
                props.mode = mode;
                props.record_id = target.record_id.clone();
                props.params = target.params.clone();
                props.embedded = true;
                props.on_done = Some(Rc::new(move |cx| {
                    done_to.update(cx, |r, cx| r.close_popup_by_id(id, None, cx));
                }));
                props.on_saved = Some(Rc::new(move |record, cx| {
                    saved_to.update(cx, |r, cx| r.close_popup_by_id(id, Some(Value::Object(record)), cx));
                }));
                let renderer = cx.new(|cx| FormRenderer::new(props, window, cx));
                self.shown.insert(id, renderer);
            }
            let renderer = self.shown[&id].clone();
            let runtime = self.runtime.clone();
            layers.push(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui_kit::black().opacity(0.4))
                    .child(
                        v_flex()
                            .id(SharedString::from(format!("popup:{id}")))
                            .aria_label(name.clone())
                            .occlude()
                            .w(px(720.))
                            .max_h(px(640.))
                            .p_4()
                            .gap_2()
                            .rounded_lg()
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.background)
                            .child(div().id(SharedString::from(format!("popup-body:{id}"))).flex_1().min_h_0().overflow_y_scroll().child(renderer))
                            .child(
                                h_flex().justify_end().child(
                                    Button::new(SharedString::from(format!("popup-close:{id}")))
                                        .label(format!("Close {name}"))
                                        .on_click(move |_, _, cx| {
                                            runtime.update(cx, |r, cx| r.close_popup_by_id(id, None, cx));
                                        }),
                                ),
                            )
                            .test_support(),
                    ),
            );
        }
        div().children(layers).into_any_element()
    }
}
