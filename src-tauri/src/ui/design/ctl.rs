//! Labeled controls of the properties panel, built on the studio's input caches. Each helper
//! takes the id a test finds it by; the visible label is the field's label.
//!
//! - `text_field`: an input that edits as the user types.
//! - `expression`: an expression input with its live problem line (`<id>-problem`).
//! - `draft`: an input applied on blur or Enter (`<id>-error` shows a rejected value).
//! - `toggle`: a checkbox. `pick`: a labeled choice (options are the `popup-menu` items).

use super::studio::DesignStudio;
use crate::forms::state::expression_problem;
use crate::ui::{
    prelude::*,
    controls::{check, choice, choice_field, expr_field, input},
};
use gpui_kit::component::checkbox::Checkbox;

impl DesignStudio {
    pub(super) fn text_field(
        &mut self,
        id: &str,
        label: &str,
        value: &str,
        placeholder: &str,
        on_change: impl Fn(&mut Self, String, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let state = self.fields.get(id, value, placeholder, move |this, text, _, cx| on_change(this, text, cx), window, cx);
        widgets::field(label, input(&state, SharedString::from(id.to_string()), label, false), cx)
    }

    /// A multi-line field.
    pub(super) fn area_field(
        &mut self,
        id: &str,
        label: &str,
        value: &str,
        on_change: impl Fn(&mut Self, String, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        use gpui_kit::component::input::Textarea;
        let state = self.fields.area(id, value, move |this, text, _, cx| on_change(this, text, cx), window, cx);
        widgets::field(
            label,
            div()
                .id(SharedString::from(id.to_string()))
                .aria_label(label.to_string())
                .h(px(84.))
                .w_full()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border)
                .child(Textarea::new(&state).h(px(82.)))
                .test_support(),
            cx,
        )
    }

    /// An expression field. `columns` give the record names, or `names` replace them.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn expression(
        &mut self,
        id: &str,
        label: &str,
        value: Option<&str>,
        columns: &[String],
        names: Option<Vec<String>>,
        placeholder: &str,
        on_change: impl Fn(&mut Self, Option<String>, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let problem = expression_problem(value, columns, names.as_deref());
        let state = self.fields.get(
            id,
            value.unwrap_or(""),
            placeholder,
            move |this, text, _, cx| on_change(this, (!text.is_empty()).then_some(text), cx),
            window,
            cx,
        );
        expr_field(id, label, &state, (!problem.is_empty()).then_some(problem), cx)
    }

    /// An input applied on blur or Enter; `apply` returns the problem to show, or "".
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draft(
        &mut self,
        id: &str,
        label: &str,
        value: &str,
        placeholder: &str,
        disabled: bool,
        apply: impl Fn(&mut Self, String, &mut Context<Self>) -> String + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        self.drafts.field(id, label, value, placeholder, disabled, move |this, text, _, cx| apply(this, text, cx), window, cx)
    }

    pub(super) fn toggle(
        &self,
        id: &str,
        label: &str,
        checked: bool,
        on_change: impl Fn(&mut Self, bool, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Checkbox {
        let entity = cx.entity();
        check(SharedString::from(id.to_string()), label, checked, move |value, _, cx| {
            entity.update(cx, |this, cx| on_change(this, value, cx));
        })
    }

    /// A labeled choice of (value, label) options.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn pick(
        &self,
        id: &str,
        label: &str,
        current: &str,
        options: Vec<(String, String)>,
        placeholder: &str,
        on_pick: impl Fn(&mut Self, &str, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Div {
        let entity = cx.entity();
        choice_field(
            SharedString::from(id.to_string()),
            label,
            current,
            options,
            placeholder,
            move |value, _, cx| entity.update(cx, |this, cx| on_pick(this, value, cx)),
            cx,
        )
    }

    /// A choice without a label row.
    pub(super) fn pick_bare(
        &self,
        id: &str,
        current: &str,
        options: Vec<(String, String)>,
        on_pick: impl Fn(&mut Self, &str, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let entity = cx.entity();
        choice(SharedString::from(id.to_string()), current, options, "", move |value, _, cx| {
            entity.update(cx, |this, cx| on_pick(this, value, cx))
        })
    }

    /// A fieldset: a bold legend over its controls.
    pub(super) fn group(&self, id: &str, legend: &str, cx: &App) -> Stateful<Div> {
        crate::ui::reports::fields::section(SharedString::from(id.to_string()), legend, cx)
    }
}

/// An icon-only button.
pub(super) fn icon_button(id: impl Into<ElementId>, icon: IconName, label: &str) -> Button {
    Button::new(id).ghost().small().icon(icon).tooltip(label.to_string())
}
