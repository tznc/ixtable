//! Lists `validate_document` issues plus the expression errors found by `expr::check`
//! (`expression_issues`); re-checks whenever the config changes. Expression errors link to
//! their object. Port of src/shell/ProblemsTab.tsx. Test ids: `problems-summary`,
//! `no-problems`, `("problem", index)` (label "<kind> <id>: <message>", or
//! "<kind> <name>: <field>: <message>" for an expression error), `("problem-open", index)`
//! (the link to an expression error's object, labelled "Open <kind> <name>"), `re-check`.

use super::{Shell, expression_issues::{ExpressionIssue, load_expression_issues}};
use crate::{
    manager::AppError,
    ui::{store::StoreEvent, task, widgets},
    validation::{Issue, Severity},
};
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

#[derive(Clone)]
enum Problem {
    Expression(ExpressionIssue),
    Plain(Issue),
}

pub struct ProblemsTab {
    shell: Entity<Shell>,
    issues: Option<Vec<Problem>>,
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
        let config = self.shell.read(cx).config(cx).clone();
        task::spawn_call(
            cx,
            move || {
                let expressions = load_expression_issues(&label, &config);
                crate::validation::validate_document(label).map(|plain| (expressions, plain))
            },
            |this, result, cx| {
                match result {
                    Ok((expressions, plain)) => {
                        this.issues = Some(
                            expressions
                                .into_iter()
                                .map(Problem::Expression)
                                .chain(plain.into_iter().map(Problem::Plain))
                                .collect(),
                        );
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

impl Problem {
    fn is_error(&self) -> bool {
        match self {
            Problem::Expression(_) => true,
            Problem::Plain(issue) => matches!(issue.severity, Severity::Error),
        }
    }
}

impl Render for ProblemsTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let summary = match &self.issues {
            None => "Checking…".to_string(),
            Some(issues) => {
                let errors = issues.iter().filter(|i| i.is_error()).count();
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
            .children(self.issues.iter().flatten().enumerate().map(|(ix, problem)| {
                let color = if problem.is_error() { danger } else { warning };
                match problem {
                    Problem::Plain(issue) => {
                        let text = format!("{} {}: {}", issue.object_kind, issue.object_id, issue.message);
                        widgets::styled_text(("problem", ix), text, move |d| d.text_sm().text_color(color))
                            .into_any_element()
                    }
                    Problem::Expression(issue) => {
                        let name = if issue.object_name.is_empty() { &issue.object_id } else { &issue.object_name };
                        let text = format!("{} {}: {}: {}", issue.object_kind, name, issue.field, issue.message);
                        let target = issue.target.clone();
                        let shell = self.shell.clone();
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(widgets::styled_text(("problem", ix), text, move |d| {
                                d.text_sm().text_color(color)
                            }))
                            .child(
                                Button::new(("problem-open", ix))
                                    .link()
                                    .small()
                                    .label(format!("Open {} {}", issue.object_kind, name))
                                    .on_click(move |_, window, cx| {
                                        let target = target.clone();
                                        shell.update(cx, |s, cx| s.request_reveal(target, window, cx));
                                    }),
                            )
                            .into_any_element()
                    }
                }
            }))
    }
}
