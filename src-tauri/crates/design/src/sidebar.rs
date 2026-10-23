//! `Sidebar`: the document's objects, always in the same place. It answers
//! "where am I" (the current object is tinted) and "what's here" (counts),
//! and each group ends with the way to add one.

use std::rc::Rc;

use gpui_bootstrap::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, SharedString, Window,
    base::{h_flex, v_flex},
    div, px,
};

use crate::typography::{Type, TypeScale as _};

/// The sidebar's width. Toasts fit inside it.
pub const SIDEBAR_WIDTH: f32 = 220.;

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

pub struct SidebarItem {
    key: SharedString,
    label: SharedString,
    count: Option<usize>,
    on_click: Option<Handler>,
}

impl SidebarItem {
    pub fn new(key: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            count: None,
            on_click: None,
        }
    }

    /// Records in a table, rows in a query.
    pub fn count(mut self, count: usize) -> Self {
        self.count = Some(count);
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

/// A group of one kind of object: Tables, Queries, Pages.
pub struct SidebarGroup {
    key: SharedString,
    label: SharedString,
    items: Vec<SidebarItem>,
    add: Option<(SharedString, Handler)>,
}

impl SidebarGroup {
    pub fn new(key: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            items: Vec::new(),
            add: None,
        }
    }

    pub fn item(mut self, item: SidebarItem) -> Self {
        self.items.push(item);
        self
    }

    /// The last row: "New table…".
    pub fn add(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.add = Some((label.into(), Rc::new(handler)));
        self
    }
}

#[cfg_attr(feature = "styleguide", gpui_styleguidist::preview("sidebar/document"))]
#[derive(IntoElement)]
pub struct Sidebar {
    id: ElementId,
    title: SharedString,
    mark: Option<AnyElement>,
    groups: Vec<SidebarGroup>,
    selected: Option<SharedString>,
}

impl Sidebar {
    /// `title` is the document's name.
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            mark: None,
            groups: Vec::new(),
            selected: None,
        }
    }

    /// The app icon beside the title.
    pub fn mark(mut self, mark: impl IntoElement) -> Self {
        self.mark = Some(mark.into_any_element());
        self
    }

    pub fn group(mut self, group: SidebarGroup) -> Self {
        self.groups.push(group);
        self
    }

    /// The key of the open object.
    pub fn selected(mut self, key: impl Into<SharedString>) -> Self {
        self.selected = Some(key.into());
        self
    }
}

impl RenderOnce for Sidebar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let (muted, hover, selected_bg) = (
            colors.secondary_color,
            colors.tertiary_bg,
            colors.primary.opacity(0.14),
        );
        let link = colors.link_color;
        let id = self.id.clone();
        let row = |key: SharedString| {
            h_flex()
                .id((id.clone(), key))
                .items_center()
                .gap(theme.spacing(2))
                .px(theme.spacing(2))
                .py(theme.spacing(1))
                .rounded(theme.radius.base)
                .cursor_pointer()
        };
        let groups = self.groups.into_iter().map(|group| {
            let items = group.items.into_iter().map(|item| {
                let selected = self.selected.as_ref() == Some(&item.key);
                row(item.key.clone())
                    .ty(Type::Body)
                    .when(selected, |this| {
                        this.bg(selected_bg)
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                    })
                    .when(!selected, |this| this.hover(move |style| style.bg(hover)))
                    .when_some(item.on_click, |this, handler| {
                        this.on_click(move |event, window, cx| handler(event, window, cx))
                    })
                    .child(div().flex_1().min_w_0().truncate().child(item.label))
                    .when_some(item.count, |this, count| {
                        this.child(
                            div()
                                .ty(Type::Caption)
                                .text_color(muted)
                                .child(count.to_string()),
                        )
                    })
            });
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .px(theme.spacing(2))
                        .pb(theme.spacing(1))
                        .ty(Type::Caption)
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(muted)
                        .child(group.label),
                )
                .children(items)
                .when_some(group.add, |this, (label, handler)| {
                    this.child(
                        row(SharedString::from(format!("add-{}", group.key)))
                            .ty(Type::Body)
                            .text_color(link)
                            .hover(move |style| style.bg(hover))
                            .on_click(move |event, window, cx| handler(event, window, cx))
                            .child(label),
                    )
                })
        });
        v_flex()
            .id(self.id)
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_none()
            .gap(theme.spacing(3))
            .p(theme.spacing(2))
            .bg(colors.tertiary_bg)
            .border_r(theme.border_width)
            .border_color(colors.border_color)
            .child(
                h_flex()
                    .items_center()
                    .gap(theme.spacing(2))
                    .px(theme.spacing(2))
                    .pt(theme.spacing(1))
                    .children(self.mark)
                    .child(div().ty(Type::Section).truncate().child(self.title)),
            )
            .children(groups)
    }
}
