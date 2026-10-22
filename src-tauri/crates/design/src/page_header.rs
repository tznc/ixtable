//! `PageHeader`: where am I, what is this, and what can I do here. Every
//! screen starts with one.

use gpui_bootstrap::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ElementId, IntoElement, RenderOnce, SharedString, Window,
    base::{h_flex, v_flex},
    div,
};

use crate::typography::{Type, TypeScale as _};

#[cfg_attr(
    feature = "styleguide",
    gpui_styleguidist::preview("page-header/playground")
)]
#[derive(IntoElement)]
pub struct PageHeader {
    id: ElementId,
    title: SharedString,
    description: Option<SharedString>,
    breadcrumb: Option<AnyElement>,
    actions: Vec<AnyElement>,
}

impl PageHeader {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: None,
            breadcrumb: None,
            actions: Vec::new(),
        }
    }

    /// One sentence: the screen's state or purpose.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// A [`gpui_bootstrap::breadcrumb::Breadcrumb`] for drill-in screens.
    pub fn breadcrumb(mut self, breadcrumb: impl IntoElement) -> Self {
        self.breadcrumb = Some(breadcrumb.into_any_element());
        self
    }

    /// Actions, right-aligned. Put the primary last.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }
}

impl RenderOnce for PageHeader {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let id = self.id.clone();
        v_flex()
            .id(self.id)
            .gap(theme.spacing(2))
            .children(self.breadcrumb)
            .child(
                h_flex()
                    .gap(theme.spacing(3))
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .id((id.clone(), "title"))
                            .ty(Type::Title)
                            .child(self.title),
                    )
                    .child(h_flex().gap(theme.spacing(2)).children(self.actions)),
            )
            .when_some(self.description, |this, description| {
                this.child(
                    div()
                        .id((id, "description"))
                        .ty(Type::Body)
                        .text_color(theme.colors.secondary_color)
                        .child(description),
                )
            })
    }
}
