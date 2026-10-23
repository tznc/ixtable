//! `Tabs`: switch between views of the same thing. Tabs never look like
//! buttons or links: inactive tabs are muted text, the active one is marked
//! with the accent. The application stores the active tab.

use std::rc::Rc;

use gpui_bootstrap::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ClickEvent, ElementId, SharedString, Window,
    base::{h_flex, v_flex},
    div, px, transparent_black,
};

use crate::typography::{Type, TypeScale as _};

type Select = Rc<dyn Fn(usize, &ClickEvent, &mut Window, &mut App)>;

/// How the tabs mark the active one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabStyle {
    /// An accent underline (a bar on the left when vertical). Page-level
    /// views: Records, Fields, Relationships.
    #[default]
    Underline,
    /// A tinted pill. Filters within a page: All, Open, Closed.
    Pills,
    /// Joined segments in a frame. Two to four modes of one panel: Design,
    /// Preview.
    Segmented,
}

impl TabStyle {
    pub const ALL: [TabStyle; 3] = [TabStyle::Underline, TabStyle::Pills, TabStyle::Segmented];

    pub fn name(self) -> &'static str {
        match self {
            TabStyle::Underline => "underline",
            TabStyle::Pills => "pills",
            TabStyle::Segmented => "segmented",
        }
    }
}

#[cfg_attr(feature = "styleguide", gpui_styleguidist::preview("tabs/basic"))]
#[derive(IntoElement)]
pub struct Tabs {
    id: ElementId,
    labels: Vec<SharedString>,
    counts: Vec<Option<usize>>,
    active: usize,
    style: TabStyle,
    vertical: bool,
    on_select: Option<Select>,
}

impl Tabs {
    pub fn new<S: Into<SharedString>>(
        id: impl Into<ElementId>,
        labels: impl IntoIterator<Item = S>,
    ) -> Self {
        Self {
            id: id.into(),
            labels: labels.into_iter().map(Into::into).collect(),
            counts: Vec::new(),
            active: 0,
            style: TabStyle::Underline,
            vertical: false,
            on_select: None,
        }
    }

    pub fn active(mut self, active: usize) -> Self {
        self.active = active;
        self
    }

    pub fn style(mut self, style: TabStyle) -> Self {
        self.style = style;
        self
    }

    /// Stacks the tabs in a column: settings sections, long lists of views.
    pub fn vertical(mut self) -> Self {
        self.vertical = true;
        self
    }

    /// A count after each label ("Open 12"), `None` for a tab without one.
    pub fn counts(mut self, counts: impl IntoIterator<Item = Option<usize>>) -> Self {
        self.counts = counts.into_iter().collect();
        self
    }

    pub fn on_select(
        mut self,
        handler: impl Fn(usize, &ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Tabs {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        // Not Bootstrap's nav: its inactive items use the link color.
        let theme = cx.theme();
        let colors = &theme.colors;
        let (text, muted, accent) = (
            colors.emphasis_color,
            colors.secondary_color,
            colors.primary,
        );
        let hover = colors.border_color;
        let tint = accent.opacity(0.14);
        let fill = colors.tertiary_bg;
        let radius = theme.radius.base;
        let (style, vertical) = (self.style, self.vertical);
        let id = self.id.clone();
        let tabs = self.labels.into_iter().enumerate().map(|(index, label)| {
            let active = index == self.active;
            let count = self.counts.get(index).copied().flatten();
            let tab = div()
                .id((id.clone(), SharedString::from(format!("tab-{index}"))))
                .flex()
                .items_center()
                .gap(theme.spacing(2))
                .ty(Type::Label)
                .cursor_pointer()
                .text_color(if active { text } else { muted })
                .when(!active, |this| {
                    this.hover(move |style| style.text_color(text))
                });
            let tab = match (style, vertical) {
                (TabStyle::Underline, false) => tab
                    .py(theme.spacing(1))
                    .border_b_2()
                    .border_color(if active { accent } else { transparent_black() })
                    .when(!active, |this| {
                        this.hover(move |style| style.border_color(hover))
                    }),
                (TabStyle::Underline, true) => tab
                    .px(theme.spacing(3))
                    .py(theme.spacing(2))
                    .border_l(px(3.))
                    .border_color(if active { accent } else { transparent_black() })
                    .when(active, |this| this.bg(fill))
                    .when(!active, |this| {
                        this.hover(move |style| style.border_color(hover).bg(fill))
                    }),
                (TabStyle::Pills, _) => tab
                    .px(theme.spacing(3))
                    .py(theme.spacing(1))
                    .rounded(radius)
                    .when(active, |this| this.bg(tint))
                    .when(!active, |this| this.hover(move |style| style.bg(fill))),
                (TabStyle::Segmented, _) => tab
                    .px(theme.spacing(3))
                    .py(theme.spacing(1))
                    .rounded(radius)
                    .when(active, |this| {
                        this.bg(colors.body_bg).shadow(vec![theme.shadow_sm()])
                    }),
            };
            tab.when(vertical && style != TabStyle::Segmented, |this| {
                this.justify_between()
            })
            .child(label)
            .when_some(count, |this, count| {
                this.child(
                    div()
                        .ty(Type::Caption)
                        .px(theme.spacing(1) * 1.5)
                        .rounded_full()
                        .bg(if active { tint } else { fill })
                        .text_color(if active { text } else { muted })
                        .child(count.to_string()),
                )
            })
            .when_some(self.on_select.clone(), |this, select| {
                this.on_click(move |event, window, cx| select(index, event, window, cx))
            })
        });
        let frame = |this: gpui_kit::Stateful<gpui_kit::Div>| match style {
            TabStyle::Segmented => this
                .p(px(2.))
                .gap(px(2.))
                .rounded(radius)
                .bg(colors.secondary_bg)
                .border(theme.border_width)
                .border_color(colors.border_color),
            TabStyle::Pills => this.gap(theme.spacing(1)),
            TabStyle::Underline if vertical => this,
            TabStyle::Underline => this.gap(theme.spacing(3)),
        };
        if vertical {
            frame(v_flex().id(self.id).items_stretch())
                .children(tabs)
                .into_any_element()
        } else {
            frame(h_flex().id(self.id))
                .when(style == TabStyle::Segmented, |this| {
                    this.flex_none().self_start()
                })
                .children(tabs)
                .into_any_element()
        }
    }
}
