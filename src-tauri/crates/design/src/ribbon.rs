//! `Ribbon`: Access-style tabs over labelled command groups. It is the
//! persistent "street sign" of Studio: the same tabs on every page, the
//! current tab underlined, and Run App as the one primary on the right.

use std::rc::Rc;

use gpui_bootstrap::ActiveTheme as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, tooltip::Tooltip};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, SharedString, Window,
    base::{h_flex, v_flex},
    div,
};

use crate::{
    action::{ActionButton, Emphasis},
    tabs::Tabs,
    typography::{Type, TypeScale as _},
};

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type Select = Rc<dyn Fn(usize, &ClickEvent, &mut Window, &mut App)>;

/// One command: an icon over a short verb ("Refresh", "New table").
pub struct RibbonCommand {
    id: SharedString,
    icon: IconName,
    label: SharedString,
    disabled_reason: Option<SharedString>,
    on_click: Option<Handler>,
}

impl RibbonCommand {
    pub fn new(
        id: impl Into<SharedString>,
        icon: IconName,
        label: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            icon,
            label: label.into(),
            disabled_reason: None,
            on_click: None,
        }
    }

    /// Dims the command; the reason shows as its tooltip ("Open a table first").
    pub fn disabled_because(mut self, reason: impl Into<SharedString>) -> Self {
        self.disabled_reason = Some(reason.into());
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    fn render(self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let hover = theme.colors.tertiary_bg;
        let reason = self.disabled_reason.clone();
        let enabled = reason.is_none();
        v_flex()
            .id(ElementId::from(self.id))
            .items_center()
            .gap_0p5()
            .min_w(gpui_kit::px(52.))
            .px(theme.spacing(2))
            .py(theme.spacing(1))
            .rounded(theme.radius.base)
            .child(Icon::new(self.icon).text_color(theme.colors.body_color))
            .child(div().ty(Type::Caption).child(self.label))
            .when(enabled, |this| {
                this.cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .when_some(self.on_click, |this, handler| {
                        this.on_click(move |event, window, cx| handler(event, window, cx))
                    })
            })
            .when_some(reason, |this, reason| {
                this.opacity(theme.disabled_opacity)
                    .tooltip(move |window, cx| Tooltip::new(reason.clone()).build(window, cx))
            })
            .into_any_element()
    }
}

/// A labelled group of commands. The label names the group in nouns
/// ("Records", "Tables") so the commands can stay short verbs.
pub struct RibbonGroup {
    key: SharedString,
    label: SharedString,
    commands: Vec<RibbonCommand>,
}

impl RibbonGroup {
    pub fn new(key: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            commands: Vec::new(),
        }
    }

    pub fn command(mut self, command: RibbonCommand) -> Self {
        self.commands.push(command);
        self
    }
}

#[cfg_attr(feature = "styleguide", gpui_styleguidist::preview("ribbon/studio"))]
#[derive(IntoElement)]
pub struct Ribbon {
    id: ElementId,
    tabs: Vec<SharedString>,
    active: usize,
    on_tab: Option<Select>,
    groups: Vec<RibbonGroup>,
    primary: Option<ActionButton>,
}

impl Ribbon {
    pub fn new<S: Into<SharedString>>(
        id: impl Into<ElementId>,
        tabs: impl IntoIterator<Item = S>,
    ) -> Self {
        Self {
            id: id.into(),
            tabs: tabs.into_iter().map(Into::into).collect(),
            active: 0,
            on_tab: None,
            groups: Vec::new(),
            primary: None,
        }
    }

    pub fn active(mut self, active: usize) -> Self {
        self.active = active;
        self
    }

    pub fn on_tab(
        mut self,
        handler: impl Fn(usize, &ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_tab = Some(Rc::new(handler));
        self
    }

    pub fn group(mut self, group: RibbonGroup) -> Self {
        self.groups.push(group);
        self
    }

    /// The ribbon's one primary action, at the right of the tab row.
    pub fn primary(mut self, action: ActionButton) -> Self {
        self.primary = Some(action.emphasis(Emphasis::Primary).small());
        self
    }
}

impl RenderOnce for Ribbon {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let border = theme.colors.border_color;
        let muted = theme.colors.secondary_color;
        let id = self.id.clone();
        let mut tabs = Tabs::new((id.clone(), "tabs"), self.tabs).active(self.active);
        if let Some(on_tab) = self.on_tab {
            tabs = tabs.on_select(move |index, event, window, cx| on_tab(index, event, window, cx));
        }
        let groups: Vec<AnyElement> = self
            .groups
            .into_iter()
            .map(|group| {
                v_flex()
                    .id((
                        id.clone(),
                        SharedString::from(format!("group-{}", group.key)),
                    ))
                    .flex_none()
                    .justify_between()
                    .gap_0p5()
                    .px(theme.spacing(2))
                    .border_r(theme.border_width)
                    .border_color(border)
                    .child(
                        h_flex()
                            .gap_0p5()
                            .children(group.commands.into_iter().map(|c| c.render(cx))),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_center()
                            .ty(Type::Caption)
                            .text_color(muted)
                            .child(group.label),
                    )
                    .into_any_element()
            })
            .collect();
        v_flex()
            .id(self.id)
            .w_full()
            .bg(theme.colors.tertiary_bg)
            .border_b(theme.border_width)
            .border_color(border)
            .child(
                h_flex()
                    .items_center()
                    .px(theme.spacing(2))
                    .child(div().flex_1().child(tabs))
                    .children(self.primary),
            )
            .child(
                h_flex()
                    .items_stretch()
                    .px(theme.spacing(1))
                    .py(theme.spacing(1))
                    .children(groups),
            )
    }
}
