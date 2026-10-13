//! Loads the saved queries a live dashboard uses (port of `useDashboardData.ts`): every
//! query runs once, in parallel, with the dashboard parameters; a new parameter set or a
//! refresh cancels runs still in flight; progress and Cancel appear once a load has taken
//! longer than `SLOW_QUERY_MS` (PRD §27.3).
//!
//! Each query runs on a worker thread and leaves its outcome in a slot that a tick loop picks
//! up, which keeps loads deterministic under the test clock. A `QueryRunner` global replaces
//! the backend call (tests use it to run slow or blocking queries).

use crate::{manager::AppError, queries::QueryRun, ui::prelude::*};
use serde_json::{Map, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

/// PRD §27.3: long loads expose progress and cancellation after 2 seconds.
pub const SLOW_QUERY_MS: u64 = 2000;
pub const TICK_MS: u64 = 50;

type RunFn = Arc<dyn Fn(&str, &str, &Map<String, Value>, &str) -> Result<QueryRun, AppError> + Send + Sync>;
type CancelFn = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// How queries run: `run(label, query id, params, run id)` and `cancel(label, run id)`.
#[derive(Clone)]
pub struct QueryRunner {
    pub run: RunFn,
    pub cancel: CancelFn,
}

impl Default for QueryRunner {
    fn default() -> Self {
        Self {
            run: Arc::new(|label, id, params, run_id| crate::dashboard_engine::load::run_saved(label, id, params, None, run_id)),
            cancel: Arc::new(|label, run_id| {
                crate::dashboard_engine::load::cancel(label, run_id);
            }),
        }
    }
}

impl Global for QueryRunner {}

impl QueryRunner {
    /// The runner in force: the installed override, else the backend.
    pub fn current(cx: &App) -> Self {
        cx.try_global::<Self>().cloned().unwrap_or_default()
    }
}

/// Where one query stands.
#[derive(Clone)]
pub enum QueryState {
    /// Running; the previous result (if any) stays on screen meanwhile.
    Loading(Option<Arc<QueryRun>>),
    Ready(Arc<QueryRun>),
    Error(String),
    Cancelled,
    /// The previewed role may not read this query, so it is not run.
    Denied,
}

type Outcomes = Arc<Mutex<Vec<(String, String, Result<QueryRun, AppError>)>>>;

pub struct Loader {
    pub states: HashMap<String, QueryState>,
    pub elapsed_ms: u64,
    ids: Vec<String>,
    key: String,
    version: u64,
    generation: u64,
    // Query id -> run id of the runs in flight.
    runs: HashMap<String, String>,
}

impl Loader {
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
            elapsed_ms: 0,
            ids: vec![],
            key: String::new(),
            version: 0,
            generation: 0,
            runs: HashMap::new(),
        }
    }

    /// Queries still running.
    pub fn pending(&self) -> usize {
        self.ids
            .iter()
            .filter(|id| matches!(self.states.get(*id), None | Some(QueryState::Loading(_))))
            .count()
    }

    pub fn total(&self) -> usize {
        self.ids.len()
    }

    pub fn slow(&self) -> bool {
        self.pending() > 0 && self.elapsed_ms >= SLOW_QUERY_MS
    }

    /// Asks for a new load (Refresh, or a record or schema change).
    pub fn refresh(&mut self) {
        self.version += 1;
    }

    /// Interrupts the runs in flight; their queries report "Query cancelled.".
    pub fn cancel(&mut self, label: &str, runner: &QueryRunner) {
        for run_id in self.runs.values() {
            (runner.cancel)(label, run_id);
        }
    }

    /// Starts a load when the queries, the parameters or the refresh count changed.
    pub fn sync<V: 'static>(
        view: &mut V,
        select: fn(&mut V) -> &mut Loader,
        label: &str,
        ids: Vec<String>,
        params: &Map<String, Value>,
        cx: &mut Context<V>,
    ) {
        let this = select(view);
        let key = serde_json::json!([ids, params, this.version]).to_string();
        if key == this.key {
            return;
        }
        this.key = key;
        let runner = QueryRunner::current(cx);
        this.cancel(label, &runner);
        this.generation += 1;
        this.elapsed_ms = 0;
        this.runs.clear();
        let previous = std::mem::take(&mut this.states);
        for id in &ids {
            let shown = match previous.get(id) {
                Some(QueryState::Ready(run)) => Some(run.clone()),
                Some(QueryState::Loading(run)) => run.clone(),
                _ => None,
            };
            this.states.insert(id.clone(), QueryState::Loading(shown));
        }
        this.ids = ids.clone();
        if ids.is_empty() {
            return;
        }
        let generation = this.generation;
        let slot: Outcomes = Arc::default();
        for id in ids {
            let run_id = uuid::Uuid::new_v4().to_string();
            this.runs.insert(id.clone(), run_id.clone());
            let (slot, runner, label, params) = (slot.clone(), runner.clone(), label.to_string(), params.clone());
            tauri::async_runtime::spawn_blocking(move || {
                let outcome = (runner.run)(&label, &id, &params, &run_id);
                slot.lock().unwrap_or_else(|e| e.into_inner()).push((id, run_id, outcome));
            });
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(TICK_MS)).await;
                let live = this
                    .update(cx, |view, cx| {
                        let loader = select(view);
                        if loader.generation != generation {
                            return false;
                        }
                        let done: Vec<_> = std::mem::take(&mut *slot.lock().unwrap_or_else(|e| e.into_inner()));
                        for (id, run_id, outcome) in done {
                            if loader.runs.get(&id) == Some(&run_id) {
                                loader.runs.remove(&id);
                            }
                            let state = match outcome {
                                Ok(run) => QueryState::Ready(Arc::new(run)),
                                Err(e) if e.code == "CANCELLED" => QueryState::Cancelled,
                                Err(e) => QueryState::Error(e.message),
                            };
                            loader.states.insert(id, state);
                        }
                        let pending = loader.pending();
                        if pending > 0 {
                            loader.elapsed_ms += TICK_MS;
                        }
                        cx.notify();
                        pending > 0
                    })
                    .unwrap_or(false);
                if !live {
                    break;
                }
            }
        })
        .detach();
    }
}

impl Default for Loader {
    fn default() -> Self {
        Self::new()
    }
}
