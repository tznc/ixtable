//! `Tabs`: switch between views of the same thing. Underline style, so tabs
//! never look like buttons or links. The application stores the active tab.

use std::rc::Rc;

use gpui_bootstrap::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{App, ClickEvent, ElementId, SharedString, Window, base::h_flex, div};

use crate::typography::{Type, TypeScale as _};

type Select = Rc<dyn Fn(usize, &ClickEvent, &mut Window, &mut App)>;

#[cfg_attr(feature = "styleguide", gpui_styleguidist::preview("tabs/basic"))]
#[derive(IntoElement)]
pub struct Tabs {
    id: ElementId,
    labels: Vec<SharedString>,
    active: usize,
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
            active: 0,
            on_select: None,
        }
    }

    pub fn active(mut self, active: usize) -> Self {
        self.active = active;
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
        // Drawn here rather than with Bootstrap's underline nav, whose inactive
        // items use the link color and read as links.
        let theme = cx.theme();
        let colors = &theme.colors;
        let (text, muted, accent) = (
            colors.emphasis_color,
            colors.secondary_color,
            colors.primary,
        );
        let hover = colors.border_color;
        let id = self.id.clone();
        h_flex().id(self.id).gap(theme.spacing(3)).children(
            self.labels.into_iter().enumerate().map(|(index, label)| {
                let active = index == self.active;
                div()
                    .id((id.clone(), SharedString::from(format!("tab-{index}"))))
                    .py(theme.spacing(1))
                    .border_b_2()
                    .ty(Type::Label)
                    .cursor_pointer()
                    .map(|this| {
                        if active {
                            this.border_color(accent).text_color(text)
                        } else {
                            this.border_color(gpui_kit::transparent_black())
                                .text_color(muted)
                                .hover(move |style| style.border_color(hover).text_color(text))
                        }
                    })
                    .when_some(self.on_select.clone(), |this, select| {
                        this.on_click(move |event, window, cx| select(index, event, window, cx))
                    })
                    .child(label)
            }),
        )
    }
}
