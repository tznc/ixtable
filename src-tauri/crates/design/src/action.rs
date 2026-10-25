//! `ActionButton`: the one button. Its emphasis says how much the action matters,
//! and a disabled action always says why.

use gpui_bootstrap::{
    ActiveTheme as _, ButtonColors, Size, Theme, Variant, button::Button, theme::mix,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, Hsla, IntoElement, RenderOnce, SharedString, Window,
    base::{self, StyledExt as _, v_flex},
    div, relative, transparent_black,
};

use crate::theme::{TEXT_CONTRAST, readable};
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

/// How a secondary or danger button is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonLook {
    /// A hairline border in the divider color, body text, a light fill on
    /// hover. The default.
    #[default]
    Outline,
    /// No border; a light grey fill. For dense rows where borders add noise.
    Soft,
}

impl ButtonLook {
    pub const ALL: [ButtonLook; 2] = [ButtonLook::Outline, ButtonLook::Soft];

    pub fn name(self) -> &'static str {
        match self {
            ButtonLook::Outline => "outline",
            ButtonLook::Soft => "soft",
        }
    }
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
    look: ButtonLook,
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
            look: ButtonLook::Outline,
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

    /// Draws a secondary or danger button another way; see [`ButtonLook`].
    pub fn look(mut self, look: ButtonLook) -> Self {
        self.look = look;
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

    /// Primary demoted to secondary: slots that hold non-primary actions use
    /// this, so a screen can't end up with two filled buttons.
    pub(crate) fn emphasis_of(&self) -> Emphasis {
        self.emphasis
    }

    pub(crate) fn not_primary(mut self) -> Self {
        if self.emphasis == Emphasis::Primary {
            self.emphasis = Emphasis::Secondary;
        }
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(std::rc::Rc::new(handler));
        self
    }

    fn size(&self) -> Size {
        if self.small {
            Size::Small
        } else {
            Size::Medium
        }
    }

    /// Primary, quiet and the filled confirm are Bootstrap's own buttons.
    fn bootstrap(&self) -> Option<Button> {
        let button = Button::new(self.id.clone()).label(self.label.clone());
        let button = match self.emphasis {
            Emphasis::Primary => button.variant(Variant::Primary),
            Emphasis::Quiet => button.link(),
            Emphasis::Danger if self.confirm => button.variant(Variant::Danger),
            _ => return None,
        };
        Some(
            button
                .with_size(self.size())
                .disabled(self.disabled_reason.is_some()),
        )
    }

    /// Secondary and danger: a hairline border and body text (or a readable
    /// red), never Bootstrap's full-strength grey outline and grey label.
    fn drawn(&self, window: &mut Window, cx: &mut App) -> AnyElement {
        let theme = cx.theme();
        let colors = look_colors(theme, self.look, self.emphasis == Emphasis::Danger);
        let focus = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let focused = focus.is_focused(window);
        let theme = cx.theme();
        let control = theme.controls.get(self.size());
        let radius = theme
            .controls
            .button_radius
            .unwrap_or(theme.radius.for_size(self.size()));
        let disabled = self.disabled_reason.is_some();
        let on_click = self.on_click.clone().filter(|_| !disabled);
        base::Button::new(self.id.clone())
            .track_focus(&focus)
            .disabled(disabled)
            .h_flex()
            .flex_shrink_0()
            .px(control.padding_x)
            .py(control.padding_y)
            .min_h(control.min_height)
            .text_size(theme.typography.for_size(self.size()))
            .line_height(relative(theme.typography.line_height))
            .border(theme.border_width)
            .border_color(colors.border)
            .rounded(radius)
            .bg(colors.bg)
            .text_color(colors.color)
            .when(!disabled, |this| {
                this.hover(move |style| {
                    style
                        .bg(colors.hover_bg)
                        .border_color(colors.hover_border)
                        .text_color(colors.hover_color)
                })
                .active(move |style| {
                    style
                        .bg(colors.active_bg)
                        .border_color(colors.active_border)
                        .text_color(colors.active_color)
                })
            })
            .when(disabled, |this| this.opacity(theme.disabled_opacity))
            .when(focused, |this| {
                this.shadow(vec![theme.focus_ring(colors.focus_ring)])
            })
            .when_some(on_click, |this, handler| {
                this.on_click(move |event, window, cx| handler(event, window, cx))
            })
            .child(self.label.clone())
            .into_any_element()
    }
}

/// The colors of a secondary (or, with `danger`, a danger) button drawn in
/// `look`. Text always reaches 4.5:1 on the button's own fill.
pub(crate) fn look_colors(theme: &Theme, look: ButtonLook, danger: bool) -> ButtonColors {
    let colors = &theme.colors;
    let tone = |strength: f32| -> Hsla {
        if danger {
            colors.danger.opacity(strength)
        } else {
            colors.secondary.opacity(strength)
        }
    };
    let (bg, border, hover_bg, active_bg) = match look {
        ButtonLook::Outline => (
            colors.body_bg,
            if danger {
                tone(0.5)
            } else {
                colors.border_color
            },
            colors.tertiary_bg,
            colors.secondary_bg,
        ),
        ButtonLook::Soft => (
            colors.tertiary_bg,
            transparent_black(),
            colors.secondary_bg,
            colors.secondary_bg,
        ),
    };
    // Danger text is the theme's red, adjusted to 4.5:1 on every fill the button takes.
    let text = if danger {
        [bg, hover_bg, active_bg]
            .into_iter()
            .fold(colors.danger, |text, fill| {
                let fill = mix(fill.opacity(1.), colors.body_bg, fill.a);
                readable(text, fill, TEXT_CONTRAST, colors.body_color)
            })
    } else {
        colors.body_color
    };
    let border_hover = if look == ButtonLook::Soft {
        border
    } else if danger {
        tone(0.8)
    } else {
        colors.secondary.opacity(0.6)
    };
    ButtonColors {
        bg,
        border,
        color: text,
        hover_bg,
        hover_border: border_hover,
        hover_color: text,
        active_bg,
        active_border: border_hover,
        active_color: text,
        focus_ring: theme.focus_ring_color(),
    }
}

impl RenderOnce for ActionButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let button = match self.bootstrap() {
            Some(mut button) => {
                if let (Some(handler), None) = (self.on_click.clone(), &self.disabled_reason) {
                    button = button.on_click(move |event, window, cx| handler(event, window, cx));
                }
                button.into_any_element()
            }
            None => self.drawn(window, cx),
        };
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
