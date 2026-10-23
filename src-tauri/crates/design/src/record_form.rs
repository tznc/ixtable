//! `RecordForm`: one column of fields and one Save. After a failed save a
//! callout at the top says how many fields need attention, and each field
//! shows its own error.

use gpui_bootstrap::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ElementId, Window,
    base::{h_flex, v_flex},
    px,
};

use crate::{
    action::{ActionButton, Emphasis},
    callout::Callout,
    field::Field,
    typography::Tone,
};

#[cfg_attr(
    feature = "styleguide",
    gpui_styleguidist::preview("record-form/order")
)]
#[derive(IntoElement)]
pub struct RecordForm {
    id: ElementId,
    fields: Vec<Field>,
    problems: usize,
    primary: Option<ActionButton>,
    actions: Vec<ActionButton>,
}

impl RecordForm {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            fields: Vec::new(),
            problems: 0,
            primary: None,
            actions: Vec::new(),
        }
    }

    pub fn field(mut self, field: Field) -> Self {
        self.fields.push(field);
        self
    }

    /// How many fields failed the last save. Pass errors to the fields
    /// themselves only after they were touched or a save was attempted.
    pub fn problems(mut self, problems: usize) -> Self {
        self.problems = problems;
        self
    }

    /// Save, usually.
    pub fn primary(mut self, action: ActionButton) -> Self {
        self.primary = Some(action.emphasis(Emphasis::Primary));
        self
    }

    /// Secondary footer actions such as "Discard changes".
    pub fn action(mut self, action: ActionButton) -> Self {
        self.actions.push(action.not_primary());
        self
    }
}

impl RenderOnce for RecordForm {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let id = self.id.clone();
        let summary = match self.problems {
            0 => None,
            1 => Some("1 field needs attention".to_string()),
            n => Some(format!("{n} fields need attention")),
        };
        v_flex()
            .id(self.id)
            .max_w(px(480.))
            .gap(theme.spacing(3))
            .when_some(summary, |this, summary| {
                this.child(
                    Callout::new((id.clone(), "problems"), Tone::Danger, summary)
                        .body("Fix the highlighted fields, then save again."),
                )
            })
            .children(self.fields)
            .child(
                h_flex()
                    .gap(theme.spacing(2))
                    .pt(theme.spacing(2))
                    .children(self.primary)
                    .children(self.actions),
            )
    }
}
