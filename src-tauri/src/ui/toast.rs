//! Transient feedback (docs/decisions/design-language.md): a small card in the
//! top-right corner (below the title bar, clear of dialog footers and the bottom-right
//! pager) that never moves the layout and dismisses itself.
//!
//! Test ids: the text is observed under `notice`, the optional action button is
//! `notice-action`, and the close button `dismiss-notice`.

use crate::ui::{prelude::*, widgets::Severity};
use std::{
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

/// Distance from the window top, so the toast sits below the title bar.
const TOP_OFFSET: Pixels = px(56.);

type Run = Rc<dyn Fn(&mut Window, &mut App)>;

static NEXT: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct Toast {
    pub text: String,
    pub severity: Severity,
    action: Option<(SharedString, Run)>,
    id: u64,
}

impl Toast {
    pub fn new(text: impl Into<String>, severity: Severity) -> Self {
        Self {
            text: text.into(),
            severity,
            action: None,
            id: 0,
        }
    }

    /// A neutral notice, such as "Save canceled."
    pub fn info(text: impl Into<String>) -> Self {
        Self::new(text, Severity::Info)
    }

    /// A completed action, such as "Exported orders.pdf".
    pub fn success(text: impl Into<String>) -> Self {
        Self::new(text, Severity::Success)
    }

    /// One follow-up action, such as "Show in folder" or "Undo".
    pub fn action(mut self, label: &str, run: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.action = Some((label.to_string().into(), Rc::new(run)));
        self
    }

    /// Six seconds, or ten when there is an action to take.
    fn lifetime(&self) -> Duration {
        Duration::from_secs(if self.action.is_some() { 10 } else { 6 })
    }
}

/// Puts `toast` in the view's `slot` and clears it after its lifetime, unless a newer
/// toast replaced it first.
pub fn show<V: 'static>(
    view: &mut V,
    slot: fn(&mut V) -> &mut Option<Toast>,
    mut toast: Toast,
    cx: &mut Context<V>,
) {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    toast.id = id;
    let lifetime = toast.lifetime();
    *slot(view) = Some(toast);
    cx.notify();
    cx.spawn(async move |this, cx| {
        cx.background_executor().timer(lifetime).await;
        let _ = this.update(cx, |view, cx| {
            let current = slot(view);
            if current.as_ref().is_some_and(|t| t.id == id) {
                *current = None;
                cx.notify();
            }
        });
    })
    .detach();
}

/// The toast, floating over the top-right corner of the window.
pub fn render(
    toast: &Toast,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
    window: &Window,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let color = toast.severity.color(cx);
    let viewport = window.viewport_size();
    let card = h_flex()
        .id("toast")
        .gap_2()
        .items_center()
        .max_w(px(420.))
        .pl_3()
        .pr_1()
        .py_2()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .shadow_lg()
        .occlude()
        .child(div().text_color(color).child(Icon::new(toast.severity.icon()).small()))
        .child(div().flex_1().min_w_0().text_sm().child(widgets::text("notice", toast.text.clone())))
        .when_some(toast.action.clone(), |row, (label, run)| {
            row.child(
                Button::new("notice-action")
                    .link()
                    .small()
                    .label(label)
                    .on_click(move |_, window, cx| run(window, cx)),
            )
        })
        .child(
            Button::new("dismiss-notice")
                .ghost()
                .xsmall()
                .icon(IconName::Close)
                .tooltip("Dismiss")
                .on_click(move |_, window, cx| on_dismiss(window, cx)),
        )
        .test_support();
    deferred(
        anchored()
            .position(point(viewport.width - px(16.), TOP_OFFSET))
            .anchor(Anchor::TopRight)
            .child(card),
    )
    .with_priority(2)
}
