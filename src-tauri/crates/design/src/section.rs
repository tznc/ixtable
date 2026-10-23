//! `Section`: a titled group within a screen, optionally collapsible. The
//! application stores whether it is open.

use gpui_kit::prelude::*;
use std::rc::Rc;

use gpui_bootstrap::ActiveTheme as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, IntoElement, ParentElement, RenderOnce, SharedString,
    Window,
    base::{h_flex, v_flex},
    div,
};

use crate::typography::{Type, TypeScale as _};

type Toggle = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[cfg_attr(
    feature = "styleguide",
    gpui_styleguidist::preview("section/playground")
)]
#[derive(IntoElement)]
pub struct Section {
    id: ElementId,
    title: SharedString,
    summary: Option<SharedString>,
    open: bool,
    on_toggle: Option<Toggle>,
    children: Vec<AnyElement>,
}

impl Section {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            summary: None,
            open: true,
            on_toggle: None,
            children: Vec::new(),
        }
    }

    /// One line beside the title that answers the section's question while
    /// it is closed: "3 relationships", "Not published".
    pub fn summary(mut self, summary: impl Into<SharedString>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// Makes the section collapsible.
    pub fn collapsible(
        mut self,
        open: bool,
        on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.open = open;
        self.on_toggle = Some(Rc::new(on_toggle));
        self
    }
}

impl ParentElement for Section {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Section {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let id = self.id.clone();
        let collapsible = self.on_toggle.is_some();
        let header = h_flex()
            .id((id.clone(), "header"))
            .gap(theme.spacing(2))
            .items_baseline()
            .when(collapsible, |this| {
                this.cursor_pointer().child(
                    div()
                        .w(gpui_kit::px(12.))
                        .text_color(theme.colors.secondary_color)
                        .child(if self.open { "▾" } else { "▸" }),
                )
            })
            .child(
                div()
                    .id((id.clone(), "title"))
                    .ty(Type::Section)
                    .child(self.title),
            )
            .when_some(self.summary, |this, summary| {
                this.child(
                    div()
                        .id((id.clone(), "summary"))
                        .ty(Type::Caption)
                        .text_color(theme.colors.secondary_color)
                        .child(summary),
                )
            })
            .when_some(self.on_toggle, |this, toggle| {
                this.on_click(move |event, window, cx| toggle(event, window, cx))
            });
        v_flex()
            .id(self.id)
            .gap(theme.spacing(2))
            .child(header)
            .when(self.open, |this| this.children(self.children))
    }
}
