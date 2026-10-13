//! Lists `validate_document` issues; re-checks whenever the config changes.
//! Port of src/shell/ProblemsTab.tsx. Test ids: `problems-summary`, `no-problems`,
//! `("problem", index)` (label "<kind> <id>: <message>"), `re-check`.

use super::Shell;
use crate::{
    manager::AppError,
    ui::{store::StoreEvent, task, widgets},
    validation::Issue,
};
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _,
    button::Button,
    h_flex, v_flex,
};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

pub struct ProblemsTab {
    shell: Entity<Shell>,
    issues: Option<Vec<Issue>>,
    error: Option<AppError>,
    _subscription: Subscription,
}

impl ProblemsTab {
    pub fn new(shell: Entity<Shell>, cx: &mut Context<Self>) -> Self {
        let store = shell.read(cx).store.clone();
        let subscription = cx.subscribe(&store, |this, _, e: &StoreEvent, cx| {
            if matches!(e, StoreEvent::Changed) {
                this.check(cx);
            }
        });
        let mut this = Self {
            shell,
            issues: None,
            error: None,
            _subscription: subscription,
        };
        this.check(cx);
        this
    }

    fn check(&mut self, cx: &mut Context<Self>) {
        let label = self.shell.read(cx).label().to_string();
        task::spawn_call(
            cx,
            move || crate::validation::validate_document(label),
            |this, result, cx| {
                match result {
                    Ok(issues) => {
                        this.issues = Some(issues);
                        this.error = None;
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
    }
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

impl Render for ProblemsTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let summary = match &self.issues {
            None => "Checking…".to_string(),
            Some(issues) => {
                let errors = issues.iter().filter(|i| matches!(i.severity, crate::validation::Severity::Error)).count();
                format!(
                    "{}, {}",
                    plural(errors, "error"),
                    plural(issues.len() - errors, "warning")
                )
            }
        };
        let theme = cx.theme();
        let (danger, warning) = (theme.danger, theme.warning);
        v_flex()
            .gap_3()
            .child(widgets::section_heading("problems-heading", "Problems"))
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(widgets::text("problems-summary", summary))
                    .child(
                        Button::new("re-check")
                            .outline()
                            .small()
                            .label("Re-check")
                            .on_click(cx.listener(|this, _, _, cx| this.check(cx))),
                    ),
            )
            .when_some(self.error.clone(), |el, e| {
                el.child(widgets::error_banner("problems-error", &e, None, cx))
            })
            .when(self.issues.as_ref().is_some_and(|i| i.is_empty()), |el| {
                el.child(widgets::text("no-problems", "No problems found."))
            })
            .children(self.issues.iter().flatten().enumerate().map(|(ix, issue)| {
                let text = format!("{} {}: {}", issue.object_kind, issue.object_id, issue.message);
                let color = if matches!(issue.severity, crate::validation::Severity::Error) {
                    danger
                } else {
                    warning
                };
                widgets::styled_text(("problem", ix), text, move |d| d.text_sm().text_color(color))
            }))
    }
}
