//! `ActionButton`: the one button. Its emphasis says how much the action matters,
//! and a disabled action always says why.

use gpui_bootstrap::{ActiveTheme as _, Size, Variant, button::Button};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ClickEvent, ElementId, IntoElement, RenderOnce, SharedString, Window, base::v_flex, div,
};

use crate::typography::{Type, TypeScale as _};

/// How loudly an action asks to be pressed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Emphasis {
    /// Filled teal. At most one per screen or dialog: the action that
    /// completes the task.
    Primary,
    /// Outline. Every other action.
    #[default]
    Secondary,
    /// Link-styled. Dense toolbars and inline "Show more".
    Quiet,
    /// Red outline: delete, remove, reset. Filled red only as the final
    /// confirm in a dialog ([`ActionButton::confirm`]).
    Danger,
}

type Handler = std::rc::Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A labelled button. Labels are a verb plus an object ("Create table"),
/// with `…` only when a dialog or picker opens first.
#[cfg_attr(
    feature = "styleguide",
    gpui_styleguidist::preview("action/playground")
)]
#[derive(IntoElement)]
pub struct ActionButton {
    id: ElementId,
    label: SharedString,
    emphasis: Emphasis,
    confirm: bool,
    small: bool,
    disabled_reason: Option<SharedString>,
    on_click: Option<Handler>,
}

impl ActionButton {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            emphasis: Emphasis::Secondary,
            confirm: false,
            small: false,
            disabled_reason: None,
            on_click: None,
        }
    }

    pub fn primary(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(id, label).emphasis(Emphasis::Primary)
    }

    pub fn quiet(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(id, label).emphasis(Emphasis::Quiet)
    }

    pub fn danger(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(id, label).emphasis(Emphasis::Danger)
    }

    pub fn emphasis(mut self, emphasis: Emphasis) -> Self {
        self.emphasis = emphasis;
        self
    }

    /// The final, irreversible step of a danger dialog: filled red.
    pub fn confirm(mut self) -> Self {
        self.confirm = true;
        self
    }

    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }

    /// Disables the action and shows `reason` under it ("Choose a file
    /// first"). There is no way to disable without a reason.
    pub fn disabled_because(mut self, reason: impl Into<SharedString>) -> Self {
        self.disabled_reason = Some(reason.into());
        self
    }

    /// Enables the action when `reason` is `None`.
    pub fn blocked_by(mut self, reason: Option<impl Into<SharedString>>) -> Self {
        self.disabled_reason = reason.map(Into::into);
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(std::rc::Rc::new(handler));
        self
    }

    fn button(&self) -> Button {
        let button = Button::new(self.id.clone()).label(self.label.clone());
        let button = match self.emphasis {
            Emphasis::Primary => button.variant(Variant::Primary),
            Emphasis::Secondary => button.variant(Variant::Secondary).outline(),
            Emphasis::Quiet => button.link(),
            Emphasis::Danger if self.confirm => button.variant(Variant::Danger),
            Emphasis::Danger => button.variant(Variant::Danger).outline(),
        };
        button
            .with_size(if self.small {
                Size::Small
            } else {
                Size::Medium
            })
            .disabled(self.disabled_reason.is_some())
    }
}

impl RenderOnce for ActionButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut button = self.button();
        if let (Some(handler), None) = (self.on_click.clone(), &self.disabled_reason) {
            button = button.on_click(move |event, window, cx| handler(event, window, cx));
        }
        let muted = cx.theme().colors.secondary_color;
        let id = self.id;
        v_flex().items_start().gap_1().child(button).when_some(
            self.disabled_reason,
            |this, reason| {
                this.child(
                    div()
                        .id((id, "reason"))
                        .ty(Type::Caption)
                        .text_color(muted)
                        .child(reason),
                )
            },
        )
    }
}
