//! `KeyValue`: facts about one thing, label left, value right of it.

use gpui_bootstrap::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ElementId, IntoElement, RenderOnce, SharedString, Window,
    base::{h_flex, v_flex},
    div, px,
};

use crate::typography::{Type, TypeScale as _};

#[cfg_attr(feature = "styleguide", gpui_styleguidist::preview("key-value/basic"))]
#[derive(IntoElement)]
pub struct KeyValue {
    id: ElementId,
    rows: Vec<(SharedString, AnyElement)>,
}

impl KeyValue {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            rows: Vec::new(),
        }
    }

    pub fn row(mut self, key: impl Into<SharedString>, value: impl IntoElement) -> Self {
        self.rows.push((key.into(), value.into_any_element()));
        self
    }

    pub fn text(self, key: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        self.row(key, value.into())
    }
}

impl RenderOnce for KeyValue {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.colors.secondary_color;
        let id = self.id;
        v_flex()
            .gap(theme.spacing(2))
            .children(
                self.rows
                    .into_iter()
                    .enumerate()
                    .map(|(index, (key, value))| {
                        h_flex()
                            .id((id.clone(), SharedString::from(format!("row-{index}"))))
                            .gap(theme.spacing(3))
                            .items_baseline()
                            .child(
                                div()
                                    .w(px(160.))
                                    .flex_none()
                                    .ty(Type::Body)
                                    .text_color(muted)
                                    .child(key),
                            )
                            .child(div().flex_1().min_w_0().ty(Type::Body).child(value))
                    }),
            )
    }
}
