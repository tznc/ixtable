//! Keeps automation running while a document is open (port of `AutomationHost.tsx`):
//! the shell owns one `AutomationHost`, which installs the trigger hook, the custom-action
//! router and the job worker and drops them with the document.
//!
//! Actions run on background threads (a write's hooks, the job worker). What they need
//! from the UI thread - messages, refreshes after writes - goes into a `Queue` that a task
//! of the shell drains.

use crate::automation_runner::{
    context::Tone,
    host::SessionHost,
    install::{install, Automation},
    worker::Polling,
};
use crate::manager::AppError;
use crate::ui::{
    dialogs::{DialogProvider, Dialogs},
    shell::Shell,
};
use gpui_kit::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Something a background run asks the UI to do.
#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    /// A `message` step or a notice about an unhandled navigation (true: an error).
    Message(String, bool),
    /// Records were written: data views refresh.
    Refresh,
}

/// Events from background threads. A plain queue polled on a timer: waking a foreground
/// task from another thread is not allowed by the headless test scheduler, and a poll
/// every 100 ms costs nothing.
#[derive(Clone, Default)]
pub struct Queue(Arc<Mutex<Vec<HostEvent>>>);

impl Queue {
    pub fn push(&self, event: HostEvent) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push(event);
    }

    pub fn take(&self) -> Vec<HostEvent> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

const RELAY_POLL: Duration = Duration::from_millis(100);

pub struct AutomationHost {
    _automation: Automation,
    _relay: Task<()>,
}

/// Polling of the job worker: tests poll fast so retries do not take whole seconds.
fn polling() -> Polling {
    if cfg!(test) {
        Polling {
            busy: Duration::from_millis(40),
            idle: Duration::from_millis(200),
        }
    } else {
        Polling::default()
    }
}

/// The host of interactive runs: confirmations ask the user through the dialog provider
/// (the calling thread is a background thread, so blocking is fine), the rest goes into
/// `queue` for the shell.
pub fn session_host(
    label: &str,
    dialogs: Option<Arc<dyn DialogProvider>>,
    queue: Queue,
) -> Arc<SessionHost> {
    let (notify, refresh) = (queue.clone(), queue);
    Arc::new(SessionHost::new(
        label,
        Arc::new(move |message| {
            dialogs
                .as_ref()
                .is_some_and(|d| d.confirm("Confirm", message))
        }),
        Arc::new(move |message, tone| {
            notify.push(HostEvent::Message(message.to_string(), tone == Tone::Error));
        }),
        Arc::new(move || refresh.push(HostEvent::Refresh)),
    ))
}

impl AutomationHost {
    /// Installs automation for the shell's window and starts relaying its events.
    pub fn new(label: &str, cx: &mut Context<Shell>) -> Self {
        let queue = Queue::default();
        let dialogs = cx.try_global::<Dialogs>().map(|d| d.0.clone());
        let host = session_host(label, dialogs, queue.clone());
        let on_job = {
            let queue = queue.clone();
            Arc::new(move |_: &crate::jobs::Job, _: bool| queue.push(HostEvent::Refresh))
        };
        let automation = install(label, host, Some(on_job), polling());
        let relay = cx.spawn(async move |shell, cx| loop {
            cx.background_executor().timer(RELAY_POLL).await;
            let events = queue.take();
            if events.is_empty() {
                continue;
            }
            let handled = shell.update(cx, |shell, cx| {
                for event in events {
                    match event {
                        HostEvent::Message(message, true) => {
                            shell.set_error(Some(AppError::new("ACTION", message)), cx)
                        }
                        HostEvent::Message(message, false) => shell.set_notice(message, cx),
                        HostEvent::Refresh => shell.records_changed(cx),
                    }
                }
            });
            if handled.is_err() {
                break;
            }
        });
        Self {
            _automation: automation,
            _relay: relay,
        }
    }
}
