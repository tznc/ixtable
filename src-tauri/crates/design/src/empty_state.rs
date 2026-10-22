//! `EmptyState`: what goes here, and the one way to add the first one.

use gpui_bootstrap::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ElementId, IntoElement, RenderOnce, SharedString, Window, base::v_flex, div,
};

use crate::typography::{Type, TypeScale as _};

#[cfg_attr(
    feature = "styleguide",
    gpui_styleguidist::preview("empty-state/playground")
)]
#[derive(IntoElement)]
pub struct EmptyState {
    id: ElementId,
    title: SharedString,
    body: Option<SharedString>,
    action: Option<AnyElement>,
}

impl EmptyState {
    /// `title` names what is missing: "No tables yet".
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            body: None,
            action: None,
        }
    }

    /// What appears here once there is something: "Tables hold your records."
    pub fn body(mut self, body: impl Into<SharedString>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// The primary action that fills it: "Create table".
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let id = self.id.clone();
        v_flex()
            .id(self.id)
            .items_center()
            .gap(theme.spacing(2))
            .py(theme.spacing(5))
            .px(theme.spacing(3))
            .child(
                div()
                    .id((id.clone(), "title"))
                    .ty(Type::Section)
                    .child(self.title),
            )
            .when_some(self.body, |this, body| {
                this.child(
                    div()
                        .id((id, "body"))
                        .ty(Type::Body)
                        .text_color(theme.colors.secondary_color)
                        .max_w(gpui_kit::px(420.))
                        .text_center()
                        .child(body),
                )
            })
            .when_some(self.action, |this, action| {
                this.child(div().pt(theme.spacing(2)).child(action))
            })
    }
}
