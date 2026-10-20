//! The one dialog component (docs/decisions/design-language.md): a titled panel with a
//! fixed header, a scrolling body and a fixed footer. As a modal it sits centered over a
//! dimmed backdrop that blocks the page; inline it is a card in place (dialogs inside a
//! panel that already shows them in context, such as a restore warning).
//!
//! Test ids: the title is observed under `id`, the panel is `<id>-dialog`, the body
//! scroll area `<id>-body`, the backdrop `<id>-backdrop` (modal only), and the close
//! button `<id>-dismiss` (only when the dialog has an escape handler).

use crate::ui::widgets::{self, Type, TypeScale as _};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::rc::Rc;

type OnClose = Rc<dyn Fn(&mut Window, &mut App)>;

/// Panel widths.
pub const SMALL: f32 = 480.;
pub const MEDIUM: f32 = 640.;
pub const LARGE: f32 = 880.;

/// Space kept between a modal and the window edge.
const MARGIN: f32 = 24.;

#[derive(IntoElement)]
pub struct Dialog {
    id: &'static str,
    title: SharedString,
    body: Vec<AnyElement>,
    footer: Vec<AnyElement>,
    width: f32,
    modal: bool,
    escape: Option<(FocusHandle, bool, OnClose)>,
}

/// A dialog titled `title`. Add content with `child`, buttons with `footer`, then make
/// it `modal()` unless it belongs inline.
pub fn dialog(id: &'static str, title: &str) -> Dialog {
    Dialog {
        id,
        title: title.to_string().into(),
        body: vec![],
        footer: vec![],
        width: MEDIUM,
        modal: false,
        escape: None,
    }
}

impl ParentElement for Dialog {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.body.extend(elements);
    }
}

impl Dialog {
    /// A footer element: buttons, Cancel before the primary action (`forms::actions()`
    /// works too). The footer stays visible while the body scrolls and lines its
    /// content up on the right, so the primary action ends up rightmost.
    pub fn footer(mut self, element: impl IntoElement) -> Self {
        self.footer.push(element.into_any_element());
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Centered over a dimmed backdrop instead of in place.
    pub fn modal(mut self) -> Self {
        self.modal = true;
        self
    }

    /// `modal()` when `on` is true.
    pub fn modal_if(mut self, on: bool) -> Self {
        self.modal = on;
        self
    }

    /// Closes on Escape and on the header's close button, unless `busy`. The panel takes
    /// keyboard focus from `focus` (call `window.focus(&focus, cx)` when it opens).
    pub fn on_escape(
        mut self,
        focus: &FocusHandle,
        busy: bool,
        on_close: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.escape = Some((focus.clone(), busy, Rc::new(on_close)));
        self
    }
}

impl RenderOnce for Dialog {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let id = self.id;
        let viewport = window.viewport_size();
        let close = self.escape.as_ref().map(|(_, busy, f)| (*busy, f.clone()));
        let header = h_flex()
            .justify_between()
            .items_center()
            .gap_2()
            .px_4()
            .pt_4()
            .pb_2()
            .child(widgets::styled_text(id, self.title.to_string(), |d| {
                d.ty(Type::Section)
            }))
            .when_some(close, |row, (busy, f)| {
                row.child(
                    Button::new(SharedString::from(format!("{id}-dismiss")))
                        .ghost()
                        .small()
                        .icon(IconName::Close)
                        .tooltip("Close")
                        .disabled(busy)
                        .on_click(move |_, window, cx| f(window, cx)),
                )
            });
        let body = v_flex()
            .id(SharedString::from(format!("{id}-body")))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap_3()
            .px_4()
            .pb_4()
            .children(self.body)
            .test_support();
        let footer = (!self.footer.is_empty()).then(|| {
            h_flex()
                .justify_end()
                .gap_2()
                .items_center()
                .px_4()
                .py_3()
                .border_t_1()
                .border_color(theme.border)
                .children(self.footer)
        });
        let mut panel = v_flex()
            .id(SharedString::from(format!("{id}-dialog")))
            .w(px(self.width))
            .max_w(viewport.width - px(2. * MARGIN))
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .shadow_lg()
            .child(header)
            .child(body)
            .children(footer);
        if self.modal {
            panel = panel.max_h(viewport.height - px(2. * MARGIN)).occlude();
        }
        if let Some((focus, busy, on_close)) = self.escape {
            panel = panel.track_focus(&focus).on_key_down(move |event, window, cx| {
                if event.keystroke.key == "escape" && !busy {
                    on_close(window, cx);
                    cx.stop_propagation();
                }
            });
        }
        if !self.modal {
            return panel.into_any_element();
        }
        deferred(
            anchored().position(point(px(0.), px(0.))).child(
                div()
                    .id(SharedString::from(format!("{id}-backdrop")))
                    .w(viewport.width)
                    .h(viewport.height)
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(black().opacity(0.45))
                    .occlude()
                    .child(panel)
                    .test_support(),
            ),
        )
        .with_priority(1)
        .into_any_element()
    }
}
