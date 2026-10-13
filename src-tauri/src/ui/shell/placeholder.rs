//! Temporary body for a mode whose feature has not been ported yet.

use crate::ui::widgets;
use gpui_kit::component::v_flex;
use gpui_kit::*;

pub struct ModePlaceholder {
    pub title: &'static str,
    pub description: &'static str,
}

impl Render for ModePlaceholder {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(widgets::titlebar(&format!("Project / {}", self.title), self.title, cx))
            .child(widgets::empty_state(
                "placeholder",
                &format!("{} coming soon", self.title),
                self.description,
                cx,
            ))
    }
}

/// Temporary body for an application-settings tab.
pub struct TabPlaceholder {
    pub title: &'static str,
}

impl Render for TabPlaceholder {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_2()
            .child(widgets::section_heading("tab-title", self.title))
            .child(widgets::empty_state(
                "placeholder",
                &format!("{} coming soon", self.title),
                "This settings tab is being ported.",
                cx,
            ))
    }
}
