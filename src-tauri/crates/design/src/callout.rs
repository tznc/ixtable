//! `Callout`: an inline message about this screen. Lead with the answer in
//! the title, then one or two sentences, then at most one action.

use gpui_bootstrap::{ActiveTheme as _, alert::Alert};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ElementId, IntoElement, RenderOnce, SharedString, Window, base::v_flex, div,
};

use crate::typography::{Tone, Type, TypeScale as _};

#[cfg_attr(
    feature = "styleguide",
    gpui_styleguidist::preview("callout/playground")
)]
#[derive(IntoElement)]
pub struct Callout {
    id: ElementId,
    tone: Tone,
    title: SharedString,
    body: Option<SharedString>,
    action: Option<AnyElement>,
    details: Option<SharedString>,
}

impl Callout {
    pub fn new(id: impl Into<ElementId>, tone: Tone, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            tone,
            title: title.into(),
            body: None,
            action: None,
            details: None,
        }
    }

    pub fn body(mut self, body: impl Into<SharedString>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// One action, usually a secondary [`crate::action::ActionButton`].
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }

    /// Technical text (SQL, error codes) shown small under the message,
    /// never in the title.
    pub fn details(mut self, details: impl Into<SharedString>) -> Self {
        self.details = Some(details.into());
        self
    }
}

impl RenderOnce for Callout {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = self.id.clone();
        let colors = &cx.theme().colors;
        // Neutral uses the lightest page fill, not Bootstrap's grey tint.
        let neutral = (colors.tertiary_bg, colors.border_color, colors.body_color);
        Alert::new(self.id)
            .variant(self.tone.variant())
            .when(self.tone == Tone::Neutral, |this| {
                this.bg(neutral.0)
                    .border_color(neutral.1)
                    .text_color(neutral.2)
            })
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .id((id.clone(), "title"))
                            .ty(Type::Label)
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(self.title),
                    )
                    .when_some(self.body, |this, body| {
                        this.child(div().id((id.clone(), "body")).ty(Type::Body).child(body))
                    })
                    .when_some(self.details, |this, details| {
                        this.child(
                            div()
                                .id((id, "details"))
                                .ty(Type::Caption)
                                .opacity(0.8)
                                .child(details),
                        )
                    })
                    .when_some(self.action, |this, action| {
                        this.child(div().pt_1().child(action))
                    }),
            )
    }
}
