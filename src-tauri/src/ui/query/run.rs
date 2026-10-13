//! Runs one read query at a time with a run id, so a newer run (or Cancel) interrupts the
//! previous one in DuckDB. Port of src/query/useQueryRun.ts and src/query/RunStatus.tsx.
//!
//! Test ids: `run-status` (progress or row counts), `run-error`, `cancel-query`,
//! `run-progress`.

use super::{QueryMode, sql::to_named_values};
use crate::{
    archive::QueryParameter,
    data::NamedValue,
    manager::AppError,
    queries::QueryRun,
    ui::{prelude::*, widgets},
};
use std::{collections::BTreeMap, time::Duration};

/// PRD §27.3: long queries expose progress and cancellation after 2 seconds.
pub const SLOW_QUERY_MS: u64 = 2000;
const TICK_MS: u64 = 250;

#[derive(Default)]
pub struct RunState {
    pub running: bool,
    /// True once the current run has taken longer than `SLOW_QUERY_MS`.
    pub slow: bool,
    pub elapsed_ms: u64,
    pub error: String,
    /// Row count, truncation and timing of the last result; the rows are in the table.
    pub summary: Option<RunSummary>,
    current: Option<String>,
}

pub struct RunSummary {
    pub rows: usize,
    pub truncated: bool,
    pub row_limit: u64,
    pub elapsed_ms: u64,
}

impl RunState {
    pub fn has_result(&self) -> bool {
        self.summary.is_some()
    }
}

impl QueryMode {
    /// Runs read-only SQL on a worker thread (cancellable by run id).
    pub(super) fn run_sql(
        &mut self,
        sql: String,
        values: &BTreeMap<String, String>,
        parameters: Vec<QueryParameter>,
        limit: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        let label = self.shell.read(cx).label().to_string();
        let params = to_named_values(values);
        self.start_run(
            move |run_id| exec(&label, run_id, &sql, &parameters, &params, limit),
            cx,
        );
    }

    fn start_run(
        &mut self,
        work: impl FnOnce(String) -> Result<QueryRun, AppError> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.cancel_current(cx);
        let run_id = uuid::Uuid::new_v4().to_string();
        self.run.current = Some(run_id.clone());
        self.run.running = true;
        self.run.slow = false;
        self.run.elapsed_ms = 0;
        self.run.error.clear();
        let job = {
            let run_id = run_id.clone();
            tauri::async_runtime::spawn_blocking(move || work(run_id))
        };
        cx.spawn({
            let run_id = run_id.clone();
            async move |this, cx| {
                let outcome = job
                    .await
                    .unwrap_or_else(|e| Err(AppError::new("DATABASE_ERROR", e)));
                let _ = this.update(cx, |this, cx| this.finish_run(&run_id, outcome, cx));
            }
        })
        .detach();
        // The elapsed clock: progress and Cancel appear after SLOW_QUERY_MS.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(TICK_MS))
                    .await;
                let live = this
                    .update(cx, |this, cx| {
                        if this.run.current.as_deref() != Some(run_id.as_str()) {
                            return false;
                        }
                        this.run.elapsed_ms += TICK_MS;
                        this.run.slow = this.run.elapsed_ms >= SLOW_QUERY_MS;
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !live {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn finish_run(&mut self, run_id: &str, outcome: Result<QueryRun, AppError>, cx: &mut Context<Self>) {
        // A newer run (or a reset) owns the state now.
        if self.run.current.as_deref() != Some(run_id) {
            return;
        }
        self.run.current = None;
        self.run.running = false;
        self.run.slow = false;
        match outcome {
            Ok(run) => {
                self.run.error.clear();
                self.run.summary = Some(RunSummary {
                    rows: run.rows.len(),
                    truncated: run.truncated,
                    row_limit: run.row_limit,
                    elapsed_ms: run.elapsed_ms,
                });
                self.results
                    .update(cx, |t, cx| t.set(run.columns, run.rows, cx));
            }
            Err(e) => {
                self.run.summary = None;
                self.results.update(cx, |t, cx| t.clear(cx));
                self.run.error = if e.code == "CANCELLED" {
                    "Query cancelled.".into()
                } else {
                    e.message
                };
            }
        }
        cx.notify();
    }

    fn cancel_current(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.run.current.take() {
            let label = self.shell.read(cx).label().to_string();
            let _ = crate::queries::cancel_query(label, Some(id));
        }
    }

    /// Cancel button: interrupts the run, which then reports "Query cancelled.".
    pub(super) fn cancel_query(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.run.current.clone() {
            let label = self.shell.read(cx).label().to_string();
            let _ = crate::queries::cancel_query(label, Some(id));
        }
    }

    /// Forgets the current result and interrupts a run in flight.
    pub(super) fn reset_run(&mut self, cx: &mut Context<Self>) {
        self.cancel_current(cx);
        self.run = RunState::default();
        self.results.update(cx, |t, cx| t.clear(cx));
    }

    pub(super) fn render_run_status(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let run = &self.run;
        if run.running {
            let seconds = if run.slow {
                format!(" {}s", run.elapsed_ms / 1000)
            } else {
                String::new()
            };
            return Some(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(widgets::text("run-status", format!("Running query…{seconds}")))
                    .when(run.slow, |el| {
                        el.child(widgets::text("run-progress", "Query progress"))
                            .child(
                                Button::new("cancel-query")
                                    .outline()
                                    .small()
                                    .icon(IconName::CircleX)
                                    .label("Cancel query")
                                    .on_click(cx.listener(|this, _, _, cx| this.cancel_query(cx))),
                            )
                    })
                    .into_any_element(),
            );
        }
        if !run.error.is_empty() {
            return Some(self.error_line("run-error", &run.error, cx));
        }
        let summary = run.summary.as_ref()?;
        let shown = if summary.truncated {
            format!(
                "First {} rows",
                super::action_text::group_digits(summary.row_limit)
            )
        } else {
            format!(
                "{} {}",
                super::action_text::group_digits(summary.rows as u64),
                if summary.rows == 1 { "row" } else { "rows" }
            )
        };
        Some(
            widgets::text(
                "run-status",
                format!(
                    "{shown} · {} ms",
                    super::action_text::group_digits(summary.elapsed_ms)
                ),
            )
            .into_any_element(),
        )
    }
}

impl Drop for QueryMode {
    fn drop(&mut self) {
        // Leaving Query mode interrupts a run still in flight.
        if let Some(id) = self.run.current.take() {
            let _ = crate::queries::cancel_query(self.label.clone(), Some(id));
        }
    }
}

/// Blocking: runs on a worker thread. Mirrors `execute_parameterized_query` without the
/// async wrapper, so the same thread can be interrupted by `cancel_query`.
fn exec(
    label: &str,
    run_id: String,
    sql: &str,
    parameters: &[QueryParameter],
    params: &[NamedValue],
    limit: Option<u64>,
) -> Result<QueryRun, AppError> {
    crate::authz::require_unrestricted(label, "run ad hoc SQL")?;
    let connection = crate::manager()?.read_connection(label)?;
    crate::queries::run_on(
        &connection,
        label,
        Some(run_id),
        sql,
        parameters,
        params,
        false,
        limit,
    )
}
