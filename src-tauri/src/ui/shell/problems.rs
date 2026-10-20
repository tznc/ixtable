//! Lists `validate_document` issues plus the expression errors found by `expr::check`
//! (`expression_issues`); re-checks whenever the config changes. Expression errors link to
//! their object. Port of src/shell/ProblemsTab.tsx. Test ids: `problems-summary`
//! (only while checking), `problems-errors` and `problems-warnings` (count badges),
//! `no-problems`, per row `("problem-severity", index)`, `("problem-object", index)`
//! ("<Kind> <name>"), `("problem", index)` (the message, prefixed "<field>: " for an
//! expression error) and `("problem-open", index)` (the link to an expression error's
//! object, labelled "Open <kind> <name>"), `re-check`.

use super::{AutomationTab, RevealTarget, Shell, expression_issues::{ExpressionIssue, load_expression_issues}};
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
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use crate::ui::widgets::{Severity as Sev, Type, TypeScale as _};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

#[derive(Clone)]
enum Problem {
    Expression(ExpressionIssue),
    Plain(Issue),
}

pub struct ProblemsTab {
    shell: Entity<Shell>,
    /// Keys of the repeated-problem groups that are expanded.
    expanded: std::collections::HashSet<String>,
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
            expanded: Default::default(),
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

/// Where a plain issue's owner lives, when it has an editor to open.
fn plain_target(kind: &str, id: &str) -> Option<RevealTarget> {
    let object_id = id.to_string();
    match kind {
        "form" => Some(RevealTarget::Design { object_id, element_id: None }),
        "report" => Some(RevealTarget::Reports { object_id, element_id: None }),
        "page" | "dashboard" => Some(RevealTarget::Page { object_id, element_id: None }),
        "action" => Some(RevealTarget::Automation { tab: AutomationTab::Actions, object_id }),
        "trigger" => Some(RevealTarget::Automation { tab: AutomationTab::Triggers, object_id }),
        _ => None,
    }
}

/// A run of problems that differ only in the object they name.
pub struct Group {
    pub key: String,
    /// "3 tables have no concurrency policy".
    pub title: String,
    pub members: Vec<usize>,
}

/// A row of the list: one problem, or a group of repeats.
pub enum Entry {
    One(usize),
    Group(Group),
}

/// "Table orders has no concurrency policy; set one in X" with name "orders" gives
/// "has no concurrency policy" and kind "table".
fn tail(message: &str, name: &str, id: &str) -> Option<String> {
    let mut rest = message;
    for key in [name, id] {
        if key.is_empty() {
            continue;
        }
        // The name as a whole word, so a short name does not match inside "Table".
        let found = message.match_indices(key).find(|(at, _)| {
            let before = message[..*at].chars().next_back();
            let after = message[at + key.len()..].chars().next();
            before.is_none_or(|c| !c.is_alphanumeric()) && after.is_none_or(|c| !c.is_alphanumeric())
        });
        if let Some((at, _)) = found {
            rest = &message[at + key.len()..];
            break;
        }
    }
    let rest = rest.split(';').next()?.trim();
    let verb = ["has ", "is ", "have ", "are "].iter().find(|v| rest.starts_with(**v))?;
    let swapped = match *verb {
        "has " => "have ",
        "is " => "are ",
        v => v,
    };
    Some(format!("{swapped}{}", &rest[verb.len()..]))
}

/// Groups three or more problems of the same severity, kind and wording.
pub fn group_problems(items: &[(bool, String, String, String, String)]) -> Vec<Entry> {
    // Each item: (is_error, kind, name, id, message).
    let keys: Vec<Option<(bool, String, String)>> = items
        .iter()
        .map(|(error, kind, name, id, message)| {
            tail(message, name, id).map(|t| (*error, kind.clone(), t))
        })
        .collect();
    let mut entries = vec![];
    let mut done = vec![false; items.len()];
    for ix in 0..items.len() {
        if done[ix] {
            continue;
        }
        let Some(key) = &keys[ix] else {
            entries.push(Entry::One(ix));
            continue;
        };
        let members: Vec<usize> = (ix..items.len()).filter(|&j| keys[j].as_ref() == Some(key)).collect();
        if members.len() < 3 {
            entries.push(Entry::One(ix));
            continue;
        }
        for &m in &members {
            done[m] = true;
        }
        let kind = if key.1 == "entity" { "table" } else { key.1.as_str() };
        entries.push(Entry::Group(Group {
            key: format!("{}:{}:{}", key.0, key.1, key.2),
            title: format!("{} {}", plural(members.len(), kind), key.2),
            members,
        }));
    }
    entries
}

const OBJECT_W: f32 = 200.;

impl ProblemsTab {
    fn problem_row(&self, ix: usize, problem: &Problem, indent: bool, config: &DocumentConfig, cx: &mut Context<Self>) -> AnyElement {
        let border = cx.theme().border;
        let (kind, id, text) = match problem {
            Problem::Plain(issue) => (&issue.object_kind, &issue.object_id, issue.message.clone()),
            Problem::Expression(issue) => (&issue.object_kind, &issue.object_id, format!("{}: {}", issue.field, issue.message)),
        };
        let name = match problem {
            Problem::Expression(issue) if !issue.object_name.is_empty() => issue.object_name.clone(),
            _ => object_name(config, kind, id).unwrap_or_else(|| id.clone()),
        };
        let target = match problem {
            Problem::Expression(issue) => Some(issue.target.clone()),
            Problem::Plain(issue) => plain_target(&issue.object_kind, &issue.object_id),
        };
        let go_label = format!("Open {} {}", kind, name);
        let row = h_flex()
            .id(("problem-row", ix))
            .gap_3()
            .items_start()
            .px_2()
            .py_1()
            .when(indent, |r| r.pl_6())
            .border_b_1()
            .border_color(border.opacity(0.5))
            .ty(Type::Body)
            .child(severity_cell(("problem-severity", ix), problem.is_error(), cx))
            .child(div().w(px(OBJECT_W)).flex_none().child(widgets::text(("problem-object", ix), object_label(kind, &name))))
            .child(div().flex_1().min_w_0().child(widgets::text(("problem", ix), text)));
        match target {
            None => row.into_any_element(),
            Some(target) => {
                let (shell, row_target) = (self.shell.clone(), target.clone());
                row.cursor_pointer()
                    .hover(|r| r.bg(cx.theme().muted.opacity(0.5)))
                    .on_click(move |_, window, cx| {
                        let target = row_target.clone();
                        shell.update(cx, |s, cx| s.request_reveal(target, window, cx));
                    })
                    .child(
                        forms::link(("problem-open", ix), &go_label).small().on_click({
                            let shell = self.shell.clone();
                            move |_, window, cx| {
                                let target = target.clone();
                                shell.update(cx, |s, cx| s.request_reveal(target, window, cx));
                            }
                        }),
                    )
                    .into_any_element()
            }
        }
    }
}

/// A severity icon in the status color with a plain-text label.
fn severity_cell(id: (&'static str, usize), error: bool, cx: &App) -> impl IntoElement {
    let (label, severity) = if error { ("Error", Sev::Danger) } else { ("Warning", Sev::Warning) };
    h_flex()
        .w(px(SEVERITY_W))
        .flex_none()
        .gap_1()
        .items_center()
        .child(div().text_color(severity.color(cx)).child(Icon::new(severity.icon()).small()))
        .child(widgets::text(id, label))
}

const SEVERITY_W: f32 = 84.;

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
        let issues = self.issues.clone().unwrap_or_default();
        let items: Vec<_> = issues
            .iter()
            .map(|p| match p {
                Problem::Plain(i) => (
                    p.is_error(),
                    i.object_kind.clone(),
                    object_name(&config, &i.object_kind, &i.object_id).unwrap_or_default(),
                    i.object_id.clone(),
                    i.message.clone(),
                ),
                Problem::Expression(i) => (true, i.object_kind.clone(), i.object_name.clone(), i.object_id.clone(), i.message.clone()),
            })
            .collect();
        let mut rows: Vec<AnyElement> = vec![];
        for entry in group_problems(&items) {
            match entry {
                Entry::One(ix) => rows.push(self.problem_row(ix, &issues[ix], false, &config, cx)),
                Entry::Group(group) => {
                    let open = self.expanded.contains(&group.key);
                    let error = items[group.members[0]].0;
                    let first = group.members[0];
                    let key = group.key.clone();
                    rows.push(
                        h_flex()
                            .id(("problem-group", first))
                            .gap_3()
                            .items_center()
                            .px_2()
                            .py_1()
                            .border_b_1()
                            .border_color(border.opacity(0.5))
                            .ty(Type::Body)
                            .cursor_pointer()
                            .child(severity_cell(("problem-group-severity", first), error, cx))
                            .child(div().flex_1().min_w_0().child(widgets::text(("problem-group-title", first), group.title.clone())))
                            .child(
                                forms::link(("problem-group-toggle", first), if open { "Hide" } else { "Show all" })
                                    .small()
                                    .icon(if open { IconName::ChevronDown } else { IconName::ChevronRight }),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.expanded.remove(&key) {
                                    this.expanded.insert(key.clone());
                                }
                                cx.notify();
                            }))
                            .into_any_element(),
                    );
                    if open {
                        for &ix in &group.members {
                            rows.push(self.problem_row(ix, &issues[ix], true, &config, cx));
                        }
                    }
                }
            }
        }
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
                        .ty(Type::Caption)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(muted)
                        .child(header("Severity").w(px(SEVERITY_W)).flex_none())
                        .child(header("Object").w(px(OBJECT_W)).flex_none())
                        .child(header("Problem").flex_1().min_w_0()),
                )
            })
            .children(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::{Entry, group_problems};

    fn item(name: &str) -> (bool, String, String, String, String) {
        (false, "entity".into(), name.into(), name.into(), format!("Table {name} has no concurrency policy; set one in Settings > Record conflicts"))
    }

    #[test]
    fn groups_three_or_more_repeats_into_one_sentence() {
        let entries = group_problems(&[item("a"), item("b"), item("c")]);
        assert_eq!(entries.len(), 1);
        let Entry::Group(g) = &entries[0] else { panic!("expected a group") };
        assert_eq!(g.title, "3 tables have no concurrency policy");
        assert_eq!(g.members, vec![0, 1, 2]);
    }

    #[test]
    fn leaves_one_or_two_repeats_alone() {
        let entries = group_problems(&[item("a"), item("b")]);
        assert!(entries.iter().all(|e| matches!(e, Entry::One(_))));
    }
}
