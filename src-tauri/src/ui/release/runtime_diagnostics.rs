//! Runtime-only windows have no Studio modes, so background jobs (status, attempts,
//! retry, cancel) and the local log are reached from the runtime sidebar instead.
//! Neither changes the application definition. Port of src/release/RuntimeDiagnostics.tsx.
//!
//! Test ids: `diagnostics-open`, dialog title `diagnostics`, `diagnostics-tab-jobs`,
//! `diagnostics-tab-logs`, `diagnostics-close`; the jobs and logs views add theirs
//! (`jobs-empty`, `job:<i>`, `log:<i>`).

use crate::{
    manager::AppError,
    ui::{
        dialog::dialog,
        forms,
        forms::eid,
        persistence::logs_tab::LogsTab,
        prelude::*,
        shell::Shell,
    },
};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticsTab {
    Jobs,
    Logs,
}

/// Builds the jobs panel for a shell. The Automation module registers its full panel
/// (retry, cancel, attempts) with `set_jobs_panel`; until then a read-only list is shown.
pub type JobsPanelBuilder = fn(&Entity<Shell>, &mut Window, &mut App) -> AnyView;

static JOBS_PANEL: OnceLock<JobsPanelBuilder> = OnceLock::new();

pub fn set_jobs_panel(builder: JobsPanelBuilder) {
    let _ = JOBS_PANEL.set(builder);
}

/// A long identifier as its first eight characters.
fn short_id(id: &str) -> String {
    if id.chars().count() > 12 { format!("{}…", id.chars().take(8).collect::<String>()) } else { id.to_string() }
}

/// Read-only list of background jobs.
pub struct JobsList {
    label: String,
    jobs: Option<Vec<String>>,
    error: Option<AppError>,
}

impl JobsList {
    pub fn new(label: String, cx: &mut Context<Self>) -> Self {
        let work_label = label.clone();
        task::spawn_call(
            cx,
            move || crate::jobs::list_jobs(work_label, None),
            |this, result, cx| {
                match result {
                    Ok(jobs) => {
                        this.jobs = Some(
                            jobs.iter()
                                .map(|j| format!("{} · {} · {}", short_id(&j.id), format!("{:?}", j.status).to_lowercase(), short_id(&j.action_id)))
                                .collect(),
                        )
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
        Self { label, jobs: None, error: None }
    }
}

impl Render for JobsList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = &self.label;
        let mut list = v_flex().gap_1().child(forms::muted(
            "The job queue runs only while ixtable is open. Jobs left running when it closed are retried.",
            cx,
        ));
        if let Some(e) = &self.error {
            list = list.child(widgets::error_banner("jobs-error", e, None, cx));
        }
        match &self.jobs {
            Some(jobs) if jobs.is_empty() => {
                list.child(widgets::text("jobs-empty", "No jobs."))
            }
            Some(jobs) => {
                for (i, job) in jobs.iter().enumerate() {
                    list = list.child(widgets::text(eid(format!("job:{i}")), job.clone()));
                }
                list
            }
            None => list,
        }
    }
}

pub struct RuntimeDiagnostics {
    shell: Entity<Shell>,
    open: bool,
    tab: DiagnosticsTab,
    jobs: Option<AnyView>,
    logs: Option<Entity<LogsTab>>,
    focus: FocusHandle,
    focus_dialog: bool,
}

impl RuntimeDiagnostics {
    pub fn new(shell: Entity<Shell>, cx: &mut Context<Self>) -> Self {
        Self {
            shell,
            open: false,
            tab: DiagnosticsTab::Jobs,
            jobs: None,
            logs: None,
            focus: cx.focus_handle(),
            focus_dialog: false,
        }
    }

    pub fn tab(&self) -> DiagnosticsTab {
        self.tab
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    fn show(&mut self, cx: &mut Context<Self>) {
        self.open = true;
        self.tab = DiagnosticsTab::Jobs;
        self.jobs = None;
        self.logs = None;
        self.focus_dialog = true;
        cx.notify();
    }
}

impl Render for RuntimeDiagnostics {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut block = v_flex().gap_2().child(
            forms::secondary("diagnostics-open", "Diagnostics…")
                .on_click(cx.listener(|this, _, _, cx| this.show(cx))),
        );
        if !self.open {
            return block;
        }
        if std::mem::take(&mut self.focus_dialog) {
            window.focus(&self.focus, cx);
        }
        let label = self.shell.read(cx).label().to_string();
        let body: AnyView = match self.tab {
            DiagnosticsTab::Jobs => {
                if self.jobs.is_none() {
                    let view = match JOBS_PANEL.get() {
                        Some(build) => build(&self.shell, window, cx),
                        None => cx.new(|cx| JobsList::new(label, cx)).into(),
                    };
                    self.jobs = Some(view);
                }
                self.jobs.clone().expect("jobs view")
            }
            DiagnosticsTab::Logs => {
                let shell = self.shell.clone();
                let logs = self
                    .logs
                    .get_or_insert_with(|| cx.new(|cx| LogsTab::new(label, shell, cx)))
                    .clone();
                logs.into()
            }
        };
        let entity = cx.entity();
        let close = cx.listener(|this, _, _, cx| {
            this.open = false;
            cx.notify();
        });
        let panel = dialog("diagnostics", "Diagnostics")
            .width(crate::ui::dialog::LARGE)
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("diagnostics-tab-jobs")
                            .ghost()
                            .label("Background jobs")
                            .selected_tint(self.tab == DiagnosticsTab::Jobs, cx)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tab = DiagnosticsTab::Jobs;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("diagnostics-tab-logs")
                            .ghost()
                            .label("Logs")
                            .selected_tint(self.tab == DiagnosticsTab::Logs, cx)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tab = DiagnosticsTab::Logs;
                                cx.notify();
                            })),
                    ),
            )
            .child(body)
            .footer(forms::primary("diagnostics-close", "Close").on_click(close))
            .on_escape(&self.focus, false, move |_, cx| {
                entity.update(cx, |this, cx| {
                    this.open = false;
                    cx.notify();
                })
            })
            .modal();
        block = block.child(panel);
        block
    }
}
