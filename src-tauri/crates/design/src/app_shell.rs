//! `AppShell`: the frame every screen sits in. With a `Sidebar`, a `Ribbon`
//! and a `PageHeader` it passes Krug's trunk test on any screen: which
//! document, which page, its sections, my options, and where I am.

use gpui_bootstrap::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ElementId, Window,
    base::{h_flex, v_flex},
    div,
};

use crate::{page_header::PageHeader, ribbon::Ribbon, sidebar::Sidebar};

#[cfg_attr(feature = "styleguide", gpui_styleguidist::preview("app-shell/studio"))]
#[derive(IntoElement)]
pub struct AppShell {
    id: ElementId,
    sidebar: Option<Sidebar>,
    ribbon: Option<Ribbon>,
    header: Option<PageHeader>,
    toasts: Vec<AnyElement>,
    children: Vec<AnyElement>,
}

impl AppShell {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            sidebar: None,
            ribbon: None,
            header: None,
            toasts: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn sidebar(mut self, sidebar: Sidebar) -> Self {
        self.sidebar = Some(sidebar);
        self
    }

    pub fn ribbon(mut self, ribbon: Ribbon) -> Self {
        self.ribbon = Some(ribbon);
        self
    }

    pub fn header(mut self, header: PageHeader) -> Self {
        self.header = Some(header);
        self
    }

    /// Toasts stack top right over the content, never over the ribbon.
    pub fn toast(mut self, toast: impl IntoElement) -> Self {
        self.toasts.push(toast.into_any_element());
        self
    }
}

impl ParentElement for AppShell {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for AppShell {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .id(self.id)
            .size_full()
            .items_start()
            .bg(theme.colors.body_bg)
            .text_color(theme.colors.body_color)
            .font_family(theme.typography.font_family.clone())
            .children(self.sidebar)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .children(self.ribbon)
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .child(
                                v_flex()
                                    .size_full()
                                    .gap(theme.spacing(3))
                                    .p(theme.spacing(4))
                                    .children(self.header)
                                    .children(self.children),
                            )
                            .when(!self.toasts.is_empty(), |this| {
                                this.child(
                                    v_flex()
                                        .absolute()
                                        .top(theme.spacing(3))
                                        .right(theme.spacing(3))
                                        .gap(theme.spacing(2))
                                        .children(self.toasts),
                                )
                            }),
                    ),
            )
    }
}
