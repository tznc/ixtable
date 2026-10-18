//! Automation mode (PRD §17): actions, record triggers and the background job queue.
//! Port of `src/automation/`; the runner itself is `crate::automation_runner`.
//!
//! - `AutomationMode` is the mode's view: a tab strip (`tab-actions`, `tab-triggers`,
//!   `tab-jobs`), the list of validation problems and the active tab.
//! - `AutomationHost` (owned by the shell) keeps triggers, custom actions and the job
//!   worker running while a document is open.
//! - `action_picker` is the control other modes use to attach an action.

mod actions;
mod env;
mod fields;
mod host;
mod jobs;
mod picker;
mod step_editor;
mod steps;
mod triggers;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_flows;
#[cfg(test)]
mod tests_runtime;

pub use actions::ActionsPanel;
pub use host::AutomationHost;
pub use jobs::JobsPanel;
pub use fields::AUTOMATION_SCOPE_NAMES;
pub use picker::action_picker;
pub use step_editor::StepEditor;
pub use triggers::TriggersPanel;

use crate::automation::{ActionDef, Trigger};
use crate::ui::shell::{AutomationTab, ModeId, RevealTarget, Shell};
use crate::ui::prelude::*;
use crate::validation::{Issue, Severity};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Actions,
    Triggers,
    Jobs,
}

const TABS: [(&str, &str); 3] = [("actions", "Actions"), ("triggers", "Triggers"), ("jobs", "Jobs")];

/// How long definitions must stay unchanged before they are validated again.
const VALIDATE_AFTER: Duration = Duration::from_millis(300);

pub struct AutomationMode {
    shell: Entity<Shell>,
    pub tab: Tab,
    pub actions: Entity<ActionsPanel>,
    pub triggers: Entity<TriggersPanel>,
    pub jobs: Entity<JobsPanel>,
    pub issues: Vec<Issue>,
    issues_open: bool,
    // The definitions the issues were (or are being) computed for.
    checked: Option<(Vec<ActionDef>, Vec<Trigger>)>,
    validation: Option<Task<()>>,
    _subscription: Subscription,
}

impl AutomationMode {
    pub fn new(shell: Entity<Shell>, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
        let (actions, triggers, jobs) = (
            {
                let shell = shell.clone();
                cx.new(|cx| ActionsPanel::new(shell, None, cx))
            },
            {
                let shell = shell.clone();
                cx.new(|cx| TriggersPanel::new(shell, None, cx))
            },
            {
                let shell = shell.clone();
                cx.new(|cx| JobsPanel::new(shell, jobs::REFRESH, cx))
            },
        );
        Self {
            shell,
            tab: Tab::Actions,
            actions,
            triggers,
            jobs,
            issues: vec![],
            issues_open: false,
            checked: None,
            validation: None,
            _subscription: subscription,
        }
    }

    /// Opens the object another mode asked to reveal.
    fn take_reveal(&mut self, cx: &mut Context<Self>) {
        let request = self.shell.update(cx, |s, _| s.take_reveal(ModeId::Automation));
        if let Some(RevealTarget::Automation { tab, object_id }) = request {
            match tab {
                AutomationTab::Actions => {
                    self.tab = Tab::Actions;
                    self.actions.update(cx, |p, cx| p.focus(&object_id, cx));
                }
                AutomationTab::Triggers => {
                    self.tab = Tab::Triggers;
                    self.triggers.update(cx, |p, cx| p.focus(&object_id, cx));
                }
            }
            cx.notify();
        }
    }

    fn select(&mut self, key: &str, cx: &mut Context<Self>) {
        self.tab = match key {
            "triggers" => Tab::Triggers,
            "jobs" => Tab::Jobs,
            _ => Tab::Actions,
        };
        cx.notify();
    }

    /// Validates again once the definitions have been still for a moment.
    fn schedule_validation(&mut self, cx: &mut Context<Self>) {
        let config = self.shell.read(cx).config(cx);
        let current = (config.actions.clone(), config.triggers.clone());
        if self.checked.as_ref() == Some(&current) {
            return;
        }
        self.checked = Some(current);
        let label = self.shell.read(cx).label().to_string();
        self.validation = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(VALIDATE_AFTER).await;
            let work = cx.background_spawn(async move { crate::automation::validate_automation(label) });
            let issues = work.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                this.issues = issues;
                cx.notify();
            });
        }));
    }

    fn render_issues(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let summary = widgets::plural(self.issues.len(), "automation problem");
        let severity = if self.issues.iter().any(|i| i.severity == Severity::Error) {
            widgets::Severity::Danger
        } else {
            widgets::Severity::Warning
        };
        let open = self.issues_open;
        v_flex()
            .id("automation-issues")
            .gap_2()
            .px_4()
            .py_2()
            .child(widgets::callout("issues-summary", severity, &summary, None, cx))
            .child(
                div().child(
                    Button::new("issues-toggle")
                        .ghost()
                        .small()
                        .start_label(None, if open { "Hide details" } else { "Show details" }.to_string())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.issues_open = !this.issues_open;
                            cx.notify();
                        })),
                ),
            )
            .when(open, |el| {
                el.children(self.issues.iter().enumerate().map(|(i, issue)| {
                    widgets::styled_text(format!("issue-{}", i + 1), issue.message.clone(), |d| d.text_sm())
                }))
            })
    }
}

impl Render for AutomationMode {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The shell is being updated while it builds this view, so reveals are taken here.
        self.take_reveal(cx);
        self.schedule_validation(cx);
        let entity = cx.entity();
        let active = match self.tab {
            Tab::Actions => ("actions", self.actions.clone().into_any_element()),
            Tab::Triggers => ("triggers", self.triggers.clone().into_any_element()),
            Tab::Jobs => ("jobs", self.jobs.clone().into_any_element()),
        };
        v_flex()
            .size_full()
            .child(widgets::titlebar("Project / Automation", "Automation", cx))
            .child(widgets::tab_strip(
                "automation-tabs",
                &TABS,
                active.0,
                move |key, _, cx| entity.update(cx, |this, cx| this.select(key, cx)),
                cx,
            ))
            .when(!self.issues.is_empty(), |el| el.child(self.render_issues(cx)))
            .child(div().id("automation-panel").flex_1().min_h_0().child(active.1))
    }
}
