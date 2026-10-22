//! The one back affordance of a runtime page (port of `BackCrumb.tsx`): breadcrumb of
//! where you came from. Test id `back-crumb`, labelled with the previous place; its link is `back-crumb-0`.

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

/// The breadcrumb for `back`: the previous place as a link (`back-crumb-0`) and a chevron.
pub fn back_crumb(back: &BackLink) -> impl IntoElement {
    let on_back = back.on_back.clone();
    widgets::breadcrumb("back-crumb", vec![back.label.clone().into()], None, move |_, _, cx| on_back(cx))
}
