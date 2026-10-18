//! The Jobs tab (port of `JobsPanel.tsx`): the durable queue of asynchronous triggers,
//! filtered by status, with each job's attempt history and cancel / retry.
//!
//! Loads overlap (timer, data changes, commands, filter changes), so each one gets a
//! request number and only the latest may render: a slow answer for an old filter must
//! not replace the current list.
//!
//! Test ids: `jobs-status`, `refresh-jobs`, `jobs-error`, `no-jobs`, `jobs-header`; per job N `job-N`
//! with cells `job-N-trigger`, `-action`, `-status`, `-attempts`, `-next`, `-error` and
//! tools `job-N-history`, `-cancel`, `-retry`; the open history is `job-N-attempts` with
//! `job-N-attempt-M` rows (cells `-number`, `-result`, `-started`, `-error`) and step logs `job-N-attempt-M-log`.

use super::actions::step_log;
use crate::ui::controls::choice_field;
use crate::ui::display::{status_badge, Tone};
use crate::automation_runner::{worker::notify_jobs_changed, StepLog};
use crate::ui::events::DocEvent;
use crate::jobs::{Job, JobAttempt, JobFilter};
use crate::manager::AppError;
use crate::ui::{prelude::*, shell::Shell, task};
use std::collections::HashMap;
use std::time::Duration;

pub const STATUSES: [&str; 5] = ["queued", "running", "succeeded", "failed", "cancelled"];

/// The sentence-case name and badge tone of a job status code.
pub fn status_label(status: &str) -> (&'static str, Tone) {
    match status {
        "queued" => ("Queued", Tone::Neutral),
        "running" => ("Running", Tone::Warning),
        "succeeded" => ("Succeeded", Tone::Success),
        "failed" => ("Failed", Tone::Danger),
        "cancelled" => ("Cancelled", Tone::Neutral),
        _ => ("Unknown", Tone::Neutral),
    }
}

/// How often the list reloads on its own.
pub const REFRESH: Duration = Duration::from_secs(2);

/// The local time of an RFC 3339 timestamp (`fmt` is a chrono format string).
pub fn local_time(iso: &str, fmt: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|t| t.with_timezone(&chrono::Local).format(fmt).to_string())
        .unwrap_or_else(|_| iso.to_string())
}

/// The step log stored with an attempt (`{"steps": [...]}`).
pub fn attempt_steps(log: &serde_json::Value) -> Vec<StepLog> {
    log.get("steps")
        .and_then(|s| serde_json::from_value(s.clone()).ok())
        .unwrap_or_default()
}

pub struct JobsPanel {
    shell: Entity<Shell>,
    pub status: String,
    pub jobs: Vec<Job>,
    pub error: Option<AppError>,
    pub open: Option<String>,
    // Attempts of the opened job, with the job revision (updated_at) they were read at.
    attempts: HashMap<String, (String, Vec<JobAttempt>)>,
    latest: u64,
    _subscription: Option<Subscription>,
    _timer: Task<()>,
}

impl JobsPanel {
    pub fn new(shell: Entity<Shell>, refresh: Duration, cx: &mut Context<Self>) -> Self {
        let timer = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(refresh).await;
            if this.update(cx, |this, cx| this.load(cx)).is_err() {
                break;
            }
        });
        Self {
            shell,
            status: String::new(),
            jobs: vec![],
            error: None,
            open: None,
            attempts: HashMap::new(),
            latest: 0,
            _subscription: None,
            _timer: timer,
        }
    }

    // The shell is being updated while it builds this view, so the first render (not
    // `new`) subscribes to its bus and loads the list.
    fn start(&mut self, cx: &mut Context<Self>) {
        if self._subscription.is_some() {
            return;
        }
        let bus = self.shell.read(cx).bus.clone();
        self._subscription = Some(cx.subscribe(&bus, |this, _, event: &DocEvent, cx| {
            if matches!(event, DocEvent::RecordsChanged) {
                this.load(cx);
            }
        }));
        self.load(cx);
    }

    fn label(&self, cx: &App) -> String {
        self.shell.read(cx).label().to_string()
    }

    /// Starts a load and returns its request number.
    pub fn begin_load(&mut self) -> u64 {
        self.latest += 1;
        self.latest
    }

    /// Takes the answer of load `request`; an answer to an older request is dropped.
    pub fn finish_load(
        &mut self,
        request: u64,
        result: Result<Vec<Job>, AppError>,
        cx: &mut Context<Self>,
    ) {
        if request != self.latest {
            return;
        }
        match result {
            Ok(jobs) => {
                self.jobs = jobs;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        if let Some(open) = self.open.clone() {
            self.load_attempts(&open, cx);
        }
        cx.notify();
    }

    pub fn load(&mut self, cx: &mut Context<Self>) {
        let request = self.begin_load();
        let label = self.label(cx);
        let status = (!self.status.is_empty()).then(|| self.status.clone());
        task::spawn_call(
            cx,
            move || {
                crate::jobs::list_jobs(label, Some(JobFilter { status, limit: None }))
            },
            move |this, result, cx| this.finish_load(request, result, cx),
        );
    }

    fn set_status(&mut self, status: String, cx: &mut Context<Self>) {
        if self.status != status {
            self.status = status;
            self.load(cx);
        }
    }

    fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.open.as_deref() == Some(id) {
            self.open = None;
        } else {
            self.open = Some(id.to_string());
            self.load_attempts(id, cx);
        }
        cx.notify();
    }

    // Reads the attempts of a job unless they are already current for its revision.
    fn load_attempts(&mut self, id: &str, cx: &mut Context<Self>) {
        let revision = self
            .jobs
            .iter()
            .find(|j| j.id == id)
            .map(|j| j.updated_at.clone())
            .unwrap_or_default();
        if self.attempts.get(id).is_some_and(|(rev, _)| *rev == revision) {
            return;
        }
        let (label, job) = (self.label(cx), id.to_string());
        let key = job.clone();
        task::spawn_call(
            cx,
            move || crate::jobs::job_attempts(label, job),
            move |this, result, cx| {
                this.attempts.insert(key, (revision, result.unwrap_or_default()));
                cx.notify();
            },
        );
    }

    fn act(
        &mut self,
        work: impl FnOnce(String) -> Result<Job, AppError> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let label = self.label(cx);
        let worker_label = label.clone();
        task::spawn_call(
            cx,
            move || work(worker_label),
            move |this, result, cx| match result {
                Ok(_) => {
                    notify_jobs_changed(&label);
                    this.load(cx);
                }
                Err(e) => {
                    this.error = Some(e);
                    cx.notify();
                }
            },
        );
    }

    fn render_job(&self, n: usize, job: &Job, cx: &mut Context<Self>) -> impl IntoElement {
        let config = self.shell.read(cx).config(cx);
        let trigger = config
            .triggers
            .iter()
            .find(|t| t.id == job.trigger_id)
            .map_or(job.trigger_id.clone(), |t| t.name.clone());
        let action = config
            .actions
            .iter()
            .find(|a| a.id == job.action_id)
            .map_or(job.action_id.clone(), |a| a.name.clone());
        let (status_text, status_tone) = status_label(&job.status);
        let row = format!("job-{n}");
        let next = if job.status == "queued" { local_time(&job.next_run_at, "%H:%M:%S") } else { String::new() };
        let (open, id) = (self.open.as_deref() == Some(job.id.as_str()), job.id.clone());
        let tool = |suffix: &str, label: &str| {
            Button::new(SharedString::from(format!("{row}-{suffix}"))).outline().xsmall().label(label.to_string())
        };
        let (cancel_id, retry_id) = (id.clone(), id.clone());
        let active = job.status == "queued" || job.status == "running";
        let finished = job.status == "failed" || job.status == "cancelled";
        let cell = |w: f32| div().w(px(w)).text_sm();
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .id(SharedString::from(row.clone()))
                    .gap_2()
                    .items_center()
                    .child(cell(140.).child(widgets::text(format!("{row}-trigger"), trigger)))
                    .child(cell(140.).child(widgets::text(format!("{row}-action"), action)))
                    .child(cell(90.).child(status_badge(format!("{row}-status"), status_text, status_tone, cx)))
                    .child(cell(60.).child(widgets::text(
                        format!("{row}-attempts"),
                        format!("{} / {}", job.attempts, job.max_attempts),
                    )))
                    .child(cell(80.).child(widgets::text(format!("{row}-next"), next)))
                    .child(cell(200.).child(widgets::text(
                        format!("{row}-error"),
                        job.last_error.clone().unwrap_or_default(),
                    )))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(tool("history", "History").on_click(
                                cx.listener(move |this, _, _, cx| this.toggle(&id, cx)),
                            ))
                            .when(active, |el| {
                                el.child(tool("cancel", "Cancel").on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        let id = cancel_id.clone();
                                        this.act(move |label| crate::jobs::cancel_job(label, id), cx)
                                    },
                                )))
                            })
                            .when(finished, |el| {
                                el.child(tool("retry", "Retry").on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        let id = retry_id.clone();
                                        this.act(move |label| crate::jobs::retry_job(label, id), cx)
                                    },
                                )))
                            }),
                    ),
            )
            .when(open, |el| el.child(self.render_attempts(&row, &job.id, cx)))
    }

    fn render_attempts(&self, row: &str, job_id: &str, cx: &App) -> impl IntoElement {
        let id = format!("{row}-attempts");
        let Some((_, attempts)) = self.attempts.get(job_id) else {
            return widgets::text(id, "Loading attempts…").into_any_element();
        };
        if attempts.is_empty() {
            return widgets::empty_state(id, "No attempts yet", "The job has not started.", cx).into_any_element();
        }
        let muted = cx.theme().muted_foreground;
        let cell = |w: f32| div().w(px(w)).flex_none().text_sm();
        v_flex()
            .id(SharedString::from(id))
            .gap_1()
            .pl_4()
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(muted)
                    .child(cell(70.).child("Attempt"))
                    .child(cell(100.).child("Result"))
                    .child(cell(160.).child("Started"))
                    .child(div().flex_1().text_sm().child("Error")),
            )
            .children(attempts.iter().enumerate().map(|(m, a)| {
                let (result, tone) = match a.ok {
                    None => ("Running", Tone::Warning),
                    Some(true) => ("Succeeded", Tone::Success),
                    Some(false) => ("Failed", Tone::Danger),
                };
                let at = format!("{row}-attempt-{}", m + 1);
                v_flex()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .id(SharedString::from(at.clone()))
                            .gap_2()
                            .items_center()
                            .child(cell(70.).child(widgets::text(format!("{at}-number"), a.attempt.to_string())))
                            .child(cell(100.).child(status_badge(format!("{at}-result"), result, tone, cx)))
                            .child(cell(160.).child(widgets::text(
                                format!("{at}-started"),
                                local_time(&a.started_at, "%Y-%m-%d %H:%M:%S"),
                            )))
                            .child(div().flex_1().min_w_0().text_sm().child(widgets::text(
                                format!("{at}-error"),
                                a.error.clone().unwrap_or_default(),
                            ))),
                    )
                    .child(step_log(&format!("{at}-log"), &attempt_steps(&a.log), cx))
            }))
            .into_any_element()
    }
}

impl Render for JobsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.start(cx);
        let entity = cx.entity();
        let mut options = vec![(String::new(), "All statuses".to_string())];
        options.extend(STATUSES.iter().map(|s| (s.to_string(), status_label(s).0.to_string())));
        let muted = cx.theme().muted_foreground;
        let jobs: Vec<Job> = self.jobs.clone();
        v_flex()
            .id("jobs-panel")
            .size_full()
            .overflow_y_scroll()
            .gap_3()
            .p_4()
            .child(
                h_flex()
                    .gap_3()
                    .items_end()
                    .child(choice_field(
                        "jobs-status",
                        "Status",
                        &self.status,
                        options,
                        "All statuses",
                        move |value, _, cx| {
                            let value = value.to_string();
                            entity.update(cx, |this, cx| this.set_status(value, cx));
                        },
                        cx,
                    ))
                    .child(
                        Button::new("refresh-jobs")
                            .outline()
                            .small()
                            .icon(IconName::RefreshCw)
                            .label("Refresh jobs")
                            .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                    ),
            )
            .child(div().text_sm().child(
                "The job queue runs only while ixtable is open. Jobs left running when it closed are retried.",
            ))
            .when_some(self.error.clone(), |el, e| el.child(widgets::error_banner("jobs-error", &e, None, cx)))
            .when(jobs.is_empty(), |el| {
                el.child(widgets::empty_state("no-jobs", "No jobs", "Jobs appear here when an asynchronous trigger fires.", cx))
            })
            .when(!jobs.is_empty(), |el| {
                let cell = |w: f32| div().w(px(w)).flex_none();
                el.child(
                    h_flex()
                        .id("jobs-header")
                        .gap_2()
                        .pb_1()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(muted)
                        .child(cell(140.).child("Trigger"))
                        .child(cell(140.).child("Action"))
                        .child(cell(90.).child("Status"))
                        .child(cell(60.).child("Attempts"))
                        .child(cell(80.).child("Next run"))
                        .child(cell(200.).child("Last error")),
                )
            })
            .children(jobs.iter().enumerate().map(|(i, j)| self.render_job(i + 1, j, cx)))
    }
}

#[cfg(test)]
mod tests {
    use super::{attempt_steps, local_time};
    use serde_json::json;

    #[test]
    fn attempt_logs_read_their_steps() {
        let log = json!({"steps": [
            {"stepIndex": 0, "path": "0", "kind": "message", "ok": true, "durationMs": 3},
            {"stepIndex": 1, "kind": "fail", "ok": false, "error": "boom", "durationMs": 1}
        ]});
        let steps = attempt_steps(&log);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[1].error.as_deref(), Some("boom"));
        assert!(steps[1].path.is_empty());
        assert!(attempt_steps(&json!(null)).is_empty());
    }

    #[test]
    fn times_that_do_not_parse_are_shown_as_given() {
        assert_eq!(local_time("not a time", "%H"), "not a time");
        assert_eq!(local_time("2026-01-02T03:04:05.000Z", "%Y").len(), 4);
    }
}
