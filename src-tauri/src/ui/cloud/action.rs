//! Busy, error and notice state of a cloud action, and the lines that show it. Port of
//! useCloudAction.ts and ActionStatus in StudioPanels.tsx.
//!
//! Test ids: `<prefix>-notice` (text) and `<prefix>-error` (code and summary).

use super::errors::{CloudError, cloud_error_notice};
use crate::ui::{forms::eid, prelude::*};

#[derive(Default)]
pub struct Action {
    pub busy: bool,
    pub error: Option<CloudError>,
    pub notice: String,
}

impl Action {
    /// Starts an action: busy, with the previous outcome cleared.
    pub fn begin(&mut self) {
        self.busy = true;
        self.error = None;
        self.notice.clear();
    }

    /// Ends an action with its outcome (`Ok("")` shows no notice).
    pub fn finish(&mut self, outcome: Result<String, CloudError>) {
        self.busy = false;
        match outcome {
            Ok(notice) => self.notice = notice,
            Err(error) => self.error = Some(error),
        }
    }
}

/// The notice and the error of an action, each under its own test id.
pub fn action_status(prefix: &str, action: &Action, cx: &App) -> Div {
    v_flex()
        .gap_1()
        .when(!action.notice.is_empty(), |el| {
            el.child(widgets::text(eid(format!("{prefix}-notice")), action.notice.clone()))
        })
        .when_some(action.error.as_ref(), |el, e| {
            el.child(cloud_error_notice(eid(format!("{prefix}-error")), e, cx))
        })
}
