//! Debounced autosave (PRD §7.2): each change restarts a debounce timer, and a max-wait
//! timer started by the first unsaved change caps the delay. One save runs at a time;
//! changes made during a save schedule another one. Port of src/persistence/autosave.ts.

use crate::manager::{AppError, SessionState};
use gpui_kit::*;
use std::time::Duration;

pub const DEBOUNCE: Duration = Duration::from_millis(1500);
pub const MAX_WAIT: Duration = Duration::from_millis(10_000);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveStatus {
    Clean,
    Dirty,
    Saving,
    Saved,
    Error,
}

pub enum AutosaveEvent {
    /// An autosave finished with this session state.
    Saved(SessionState),
}

pub struct Autosave {
    label: SharedString,
    pub status: SaveStatus,
    /// Autosave runs only for titled, unconflicted, editable documents.
    pub eligible: bool,
    pub last_saved_at: Option<String>,
    pub error: Option<AppError>,
    debounce: Option<Task<()>>,
    max_wait: Option<Task<()>>,
    in_flight: bool,
    changed_during_save: bool,
}
impl EventEmitter<AutosaveEvent> for Autosave {}

fn is_eligible(state: &SessionState) -> bool {
    state.path.is_some() && !state.conflict && !state.runtime_only
}

impl Autosave {
    pub fn new(label: &str, initial: &SessionState, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            label: label.to_string().into(),
            status: SaveStatus::Clean,
            eligible: false,
            last_saved_at: None,
            error: None,
            debounce: None,
            max_wait: None,
            in_flight: false,
            changed_during_save: false,
        };
        this.sync(initial, cx);
        this
    }

    /// Adopts a backend SessionState (from any command). A dirty state counts as a change.
    pub fn sync(&mut self, state: &SessionState, cx: &mut Context<Self>) {
        self.eligible = is_eligible(state);
        if state.last_saved_at.is_some() {
            self.last_saved_at = state.last_saved_at.clone();
        }
        if state.dirty {
            self.touch(cx);
        } else if !self.in_flight {
            self.clear_timers();
            self.error = state.last_error.clone();
            self.status = if self.error.is_some() {
                SaveStatus::Error
            } else if state.last_saved_at.is_some() {
                SaveStatus::Saved
            } else {
                SaveStatus::Clean
            };
        }
        cx.notify();
    }

    /// Records a change: shows "dirty" and (re)schedules a save when eligible.
    pub fn touch(&mut self, cx: &mut Context<Self>) {
        if self.in_flight {
            self.changed_during_save = true;
            return;
        }
        if self.status != SaveStatus::Error {
            self.status = SaveStatus::Dirty;
        }
        cx.notify();
        if !self.eligible {
            return;
        }
        self.debounce = Some(self.timer(DEBOUNCE, cx));
        if self.max_wait.is_none() {
            self.max_wait = Some(self.timer(MAX_WAIT, cx));
        }
    }

    fn timer(&self, after: Duration, cx: &mut Context<Self>) -> Task<()> {
        let timer = cx.background_executor().timer(after);
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| this.flush(cx));
        })
    }

    fn clear_timers(&mut self) {
        self.debounce = None;
        self.max_wait = None;
    }

    /// Saves now (Retry, or a timer firing).
    pub fn flush(&mut self, cx: &mut Context<Self>) {
        self.clear_timers();
        if self.in_flight || !self.eligible {
            return;
        }
        self.in_flight = true;
        self.changed_during_save = false;
        self.status = SaveStatus::Saving;
        cx.notify();
        let label = self.label.to_string();
        crate::ui::task::spawn_call(
            cx,
            move || crate::manager::autosave_document(label),
            |this, result, cx| {
                this.in_flight = false;
                match result {
                    Ok(state) => {
                        this.eligible = is_eligible(&state);
                        if state.last_saved_at.is_some() {
                            this.last_saved_at = state.last_saved_at.clone();
                        }
                        let failed = state.last_error.clone();
                        this.error = if state.dirty { failed.clone() } else { None };
                        this.status = match (state.dirty, failed.is_some()) {
                            (true, true) => SaveStatus::Error,
                            (true, false) => SaveStatus::Dirty,
                            _ if state.last_saved_at.is_some() => SaveStatus::Saved,
                            _ => SaveStatus::Clean,
                        };
                        let again = state.dirty || this.changed_during_save;
                        cx.emit(AutosaveEvent::Saved(state));
                        if again {
                            this.touch(cx);
                        }
                    }
                    Err(e) => {
                        this.status = SaveStatus::Error;
                        this.error = Some(e);
                    }
                }
                cx.notify();
            },
        );
    }
}
