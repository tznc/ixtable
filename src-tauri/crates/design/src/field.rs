//! `Field`: a label above one control, with "Optional" instead of a
//! required star, a hint under it, and an error that replaces the hint.

use gpui_bootstrap::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ElementId, IntoElement, RenderOnce, SharedString, Window,
    base::{h_flex, v_flex},
    div,
};

use crate::typography::{Type, TypeScale as _};

#[cfg_attr(feature = "styleguide", gpui_styleguidist::preview("field/states"))]
#[derive(IntoElement)]
pub struct Field {
    id: ElementId,
    label: SharedString,
    optional: bool,
    hint: Option<SharedString>,
    error: Option<SharedString>,
    control: Option<AnyElement>,
}

impl Field {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            optional: false,
            hint: None,
            error: None,
            control: None,
        }
    }

    /// Marks the field "Optional". Most fields are required, so only the
    /// exceptions are labelled.
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Show only after the field was touched or the form submitted. It says
    /// how to fix the value: "Enter a date like 2026-10-22".
    pub fn error(mut self, error: Option<impl Into<SharedString>>) -> Self {
        self.error = error.map(Into::into);
        self
    }

    pub fn control(mut self, control: impl IntoElement) -> Self {
        self.control = Some(control.into_any_element());
        self
    }
}

impl RenderOnce for Field {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.colors.secondary_color;
        let danger = theme.variant(gpui_bootstrap::Variant::Danger).text_emphasis;
        let id = self.id.clone();
        let message = match (self.error, self.hint) {
            (Some(error), _) => Some(("error", error, danger)),
            (None, Some(hint)) => Some(("hint", hint, muted)),
            (None, None) => None,
        };
        v_flex()
            .id(self.id)
            .gap(theme.spacing(1))
            .child(
                h_flex()
                    .gap(theme.spacing(2))
                    .items_baseline()
                    .child(
                        div()
                            .id((id.clone(), "label"))
                            .ty(Type::Label)
                            .child(self.label),
                    )
                    .when(self.optional, |this| {
                        this.child(
                            div()
                                .id((id.clone(), "optional"))
                                .ty(Type::Caption)
                                .text_color(muted)
                                .child("Optional"),
                        )
                    }),
            )
            .children(self.control)
            .when_some(message, |this, (kind, text, color)| {
                this.child(
                    div()
                        .id((id, kind))
                        .ty(Type::Caption)
                        .text_color(color)
                        .child(text),
                )
            })
    }
}
