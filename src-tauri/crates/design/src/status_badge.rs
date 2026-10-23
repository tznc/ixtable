//! `StatusBadge`: a record's or job's state in one or two words.

use gpui_bootstrap::badge::Badge;
use gpui_kit::{App, IntoElement, RenderOnce, SharedString, Window};

use crate::typography::Tone;

#[cfg_attr(
    feature = "styleguide",
    gpui_styleguidist::preview("status-badge/tones")
)]
#[derive(IntoElement)]
pub struct StatusBadge {
    tone: Tone,
    label: SharedString,
}

impl StatusBadge {
    pub fn new(tone: Tone, label: impl Into<SharedString>) -> Self {
        Self {
            tone,
            label: label.into(),
        }
    }
}

impl RenderOnce for StatusBadge {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        // Subtle: dark text on a pale tint reads at 4.5:1 in every tone.
        Badge::new(self.label)
            .variant(self.tone.variant())
            .subtle(true)
            .pill(true)
    }
}
