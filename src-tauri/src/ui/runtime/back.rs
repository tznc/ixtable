//! The one back affordance of a runtime page (port of `BackCrumb.tsx`): "Back to <where you
//! came from>". Test id `back-crumb`, labelled "Back to X".

use crate::ui::prelude::*;
use std::rc::Rc;

/// Where a back link returns to: the list a record was opened from, or the previous page.
#[derive(Clone)]
pub struct BackLink {
    pub label: String,
    pub on_back: Rc<dyn Fn(&mut App)>,
}

impl BackLink {
    pub fn new(label: impl Into<String>, on_back: impl Fn(&mut App) + 'static) -> Self {
        Self { label: label.into(), on_back: Rc::new(on_back) }
    }
}

/// The crumb button for `back`.
pub fn back_crumb(back: &BackLink) -> impl IntoElement {
    let on_back = back.on_back.clone();
    let label = format!("Back to {}", back.label);
    div()
        .id("back-crumb")
        .aria_label(label.clone())
        .self_start()
        .child(
            Button::new("back-crumb-button")
                .ghost()
                .small()
                .icon(IconName::ArrowLeft)
                .label(back.label.clone())
                .on_click(move |_, _, cx| on_back(cx)),
        )
        .test_support()
}
