//! `PageHeader`: where am I, what is this, and what can I do here. Every
//! screen starts with one.

use gpui_bootstrap::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ElementId, IntoElement, RenderOnce, SharedString, Window,
    base::{h_flex, v_flex},
    div,
};

use crate::{
    action::{ActionButton, Emphasis},
    typography::{Type, TypeScale as _},
};

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
    actions: Vec<ActionButton>,
    primary: Option<ActionButton>,
}

impl PageHeader {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: None,
            breadcrumb: None,
            actions: Vec::new(),
            primary: None,
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

    /// A secondary action. A primary passed here is shown as secondary.
    pub fn action(mut self, action: ActionButton) -> Self {
        self.actions.push(action.not_primary());
        self
    }

    /// The screen's one primary action, shown last on the right.
    pub fn primary(mut self, action: ActionButton) -> Self {
        self.primary = Some(action.emphasis(Emphasis::Primary));
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
                    .child(
                        h_flex()
                            .gap(theme.spacing(2))
                            .children(self.actions)
                            .children(self.primary),
                    ),
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
