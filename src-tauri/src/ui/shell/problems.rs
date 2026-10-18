//! Lists `validate_document` issues plus the expression errors found by `expr::check`
//! (`expression_issues`); re-checks whenever the config changes. Expression errors link to
//! their object. Port of src/shell/ProblemsTab.tsx. Test ids: `problems-summary`
//! (only while checking), `problems-errors` and `problems-warnings` (count badges),
//! `no-problems`, per row `("problem-severity", index)`, `("problem-object", index)`
//! ("<Kind> <name>"), `("problem", index)` (the message, prefixed "<field>: " for an
//! expression error) and `("problem-open", index)` (the link to an expression error's
//! object, labelled "Open <kind> <name>"), `re-check`.

use super::{Shell, expression_issues::{ExpressionIssue, load_expression_issues}};
use crate::{
    archive::DocumentConfig,
    manager::AppError,
    ui::{
        display::{Tone, status_badge},
        forms,
        store::StoreEvent,
        task, widgets,
    },
    validation::{Issue, Severity},
};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
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

use crate::ui::widgets::plural;

impl Problem {
    fn is_error(&self) -> bool {
        match self {
            Problem::Expression(_) => true,
            Problem::Plain(issue) => matches!(issue.severity, Severity::Error),
        }
    }
}

/// The name of the definition object an issue points at, when it has one.
fn object_name(config: &DocumentConfig, kind: &str, id: &str) -> Option<String> {
    let name = match kind {
        "form" => config.design.forms.iter().find(|o| o.id == id).map(|o| &o.name),
        "report" => config.reports.iter().find(|o| o.id == id).map(|o| &o.name),
        "page" | "dashboard" => config.pages.iter().find(|o| o.id == id).map(|o| &o.name),
        "query" => config.saved_queries.iter().find(|o| o.id == id).map(|o| &o.name),
        "action" => config.actions.iter().find(|o| o.id == id).map(|o| &o.name),
        "trigger" => config.triggers.iter().find(|o| o.id == id).map(|o| &o.name),
        "migration" => config.migrations.iter().find(|o| o.id == id).map(|o| &o.name),
        "role" => config.roles.iter().find(|o| o.id == id).map(|o| &o.name),
        _ => None,
    };
    name.filter(|n| !n.trim().is_empty()).cloned()
}

/// "Report Sales": the kind in sentence case, then the object's name (its id when unnamed).
fn object_label(kind: &str, name: &str) -> String {
    // Validation calls tables "entity"; users know them as tables.
    let kind = if kind == "entity" { "table" } else { kind };
    let mut chars = kind.chars();
    let kind = chars.next().map(|c| c.to_uppercase().chain(chars).collect::<String>()).unwrap_or_default();
    format!("{kind} {name}")
}

const SEVERITY_W: f32 = 76.;
const OBJECT_W: f32 = 200.;

impl Render for ProblemsTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let counts = self.issues.as_ref().map(|issues| {
            let errors = issues.iter().filter(|i| i.is_error()).count();
            (errors, issues.len() - errors)
        });
        let config = self.shell.read(cx).config(cx).clone();
        let (muted, border) = (cx.theme().muted_foreground, cx.theme().border);
        let header = |label: &'static str| div().child(label);
        let has_rows = self.issues.as_ref().is_some_and(|i| !i.is_empty());
        v_flex()
            .gap_3()
            .child(widgets::section_heading("problems-heading", "Problems"))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(match counts {
                        None => div().child(widgets::text("problems-summary", "Checking…")),
                        Some((errors, warnings)) => div()
                            .flex()
                            .gap_2()
                            .child(status_badge(
                                "problems-errors",
                                &plural(errors, "error"),
                                if errors > 0 { Tone::Danger } else { Tone::Neutral },
                                cx,
                            ))
                            .child(status_badge(
                                "problems-warnings",
                                &plural(warnings, "warning"),
                                if warnings > 0 { Tone::Warning } else { Tone::Neutral },
                                cx,
                            )),
                    })
                    .child(
                        forms::secondary("re-check", "Check again")
                            .small()
                            .icon(IconName::RefreshCw)
                            .on_click(cx.listener(|this, _, _, cx| this.check(cx))),
                    ),
            )
            .when_some(self.error.clone(), |el, e| {
                el.child(widgets::error_banner("problems-error", &e, None, cx))
            })
            .when(self.issues.as_ref().is_some_and(|i| i.is_empty()), |el| {
                el.child(widgets::empty_state(
                    "no-problems",
                    "No problems found.",
                    "Every form, report, query and expression in this document checks out.",
                    cx,
                ))
            })
            .when(has_rows, |el| {
                el.child(
                    h_flex()
                        .gap_3()
                        .px_2()
                        .pb_1()
                        .border_b_1()
                        .border_color(border)
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(muted)
                        .child(header("Severity").w(px(SEVERITY_W)).flex_none())
                        .child(header("Object").w(px(OBJECT_W)).flex_none())
                        .child(header("Problem").flex_1().min_w_0()),
                )
            })
            .children(self.issues.iter().flatten().enumerate().map(|(ix, problem)| {
                let (severity, tone) = if problem.is_error() { ("Error", Tone::Danger) } else { ("Warning", Tone::Warning) };
                let (kind, id, text) = match problem {
                    Problem::Plain(issue) => (&issue.object_kind, &issue.object_id, issue.message.clone()),
                    Problem::Expression(issue) => {
                        (&issue.object_kind, &issue.object_id, format!("{}: {}", issue.field, issue.message))
                    }
                };
                let name = match problem {
                    Problem::Expression(issue) if !issue.object_name.is_empty() => issue.object_name.clone(),
                    _ => object_name(&config, kind, id).unwrap_or_else(|| id.clone()),
                };
                let row = h_flex()
                    .gap_3()
                    .items_start()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(border.opacity(0.5))
                    .text_sm()
                    .child(
                        div()
                            .w(px(SEVERITY_W))
                            .flex_none()
                            .child(status_badge(("problem-severity", ix), severity, tone, cx)),
                    )
                    .child(
                        div()
                            .w(px(OBJECT_W))
                            .flex_none()
                            .child(widgets::text(("problem-object", ix), object_label(kind, &name))),
                    )
                    .child(div().flex_1().min_w_0().child(widgets::text(("problem", ix), text)));
                match problem {
                    Problem::Plain(_) => row,
                    Problem::Expression(issue) => {
                        let target = issue.target.clone();
                        let shell = self.shell.clone();
                        row.child(
                            forms::link(("problem-open", ix), &format!("Open {} {}", issue.object_kind, name))
                                .small()
                                .on_click(move |_, window, cx| {
                                    let target = target.clone();
                                    shell.update(cx, |s, cx| s.request_reveal(target, window, cx));
                                }),
                        )
                    }
                }
            }))
    }
}
