//! `Tabs`: switch between views of the same thing. Underline style, so tabs
//! never look like buttons. The application stores the active tab.

use std::rc::Rc;

use gpui_bootstrap::nav::{Nav, NavItem};
use gpui_kit::{App, ClickEvent, ElementId, IntoElement, RenderOnce, SharedString, Window};

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
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let id = self.id.clone();
        Nav::new(self.id)
            .underline()
            .items(self.labels.into_iter().enumerate().map(|(index, label)| {
                let item = NavItem::new(
                    (id.clone(), SharedString::from(format!("tab-{index}"))),
                    label,
                )
                .active(index == self.active);
                match self.on_select.clone() {
                    Some(select) => {
                        item.on_click(move |event, window, cx| select(index, event, window, cx))
                    }
                    None => item,
                }
            }))
    }
}
