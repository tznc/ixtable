//! `Toast`: the result of an action, bottom left over the sidebar, gone after
//! a few seconds. `AppShell` places it; the application times it.

use gpui_bootstrap::{ActiveTheme as _, close_button::CloseButton};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ClickEvent, ElementId, IntoElement, RenderOnce, SharedString, Window,
    base::{h_flex, v_flex},
    div, px,
};

use crate::sidebar::SIDEBAR_WIDTH;
use crate::typography::{Tone, Type, TypeScale as _};

/// A toast's width: the sidebar's, less a 12px margin each side.
pub const TOAST_WIDTH: f32 = SIDEBAR_WIDTH - 24.;

type Dismiss = std::rc::Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[cfg_attr(feature = "styleguide", gpui_styleguidist::preview("toast/tones"))]
#[derive(IntoElement)]
pub struct Toast {
    id: ElementId,
    tone: Tone,
    message: SharedString,
    detail: Option<SharedString>,
    on_dismiss: Option<Dismiss>,
}

impl Toast {
    /// `message` is past tense and names the thing: "Saved Orders".
    pub fn new(id: impl Into<ElementId>, tone: Tone, message: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            tone,
            message: message.into(),
            detail: None,
            on_dismiss: None,
        }
    }

    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn on_dismiss(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_dismiss = Some(std::rc::Rc::new(handler));
        self
    }
}

impl RenderOnce for Toast {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let marker = match self.tone {
            Tone::Neutral => theme.colors.secondary_color,
            tone => theme.colors.variant(tone.variant()),
        };
        let id = self.id.clone();
        h_flex()
            .id(self.id)
            .w(px(TOAST_WIDTH))
            .items_start()
            .gap(theme.spacing(2))
            .p(theme.spacing(3) * 0.75)
            .bg(theme.colors.body_bg)
            .border(theme.border_width)
            .border_color(theme.colors.border_color)
            .rounded(theme.radius.base)
            .shadow(vec![theme.shadow()])
            .child(div().w(px(3.)).self_stretch().rounded_full().bg(marker))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .id((id.clone(), "message"))
                            .ty(Type::Label)
                            .child(self.message),
                    )
                    .when_some(self.detail, |this, detail| {
                        this.child(
                            div()
                                .id((id.clone(), "detail"))
                                .ty(Type::Caption)
                                .text_color(theme.colors.secondary_color)
                                .child(detail),
                        )
                    }),
            )
            .when_some(self.on_dismiss, |this, dismiss| {
                this.child(
                    CloseButton::new((id, "dismiss"))
                        .on_click(move |event, window, cx| dismiss(event, window, cx)),
                )
            })
    }
}
