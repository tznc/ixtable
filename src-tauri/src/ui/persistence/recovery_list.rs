//! Start-screen list of unsaved work left by sessions that ended without closing
//! (PRD 7.2). Recover reopens it and saves it back into its .ixt; Discard deletes it.
//! Port of src/persistence/RecoveryList.tsx.
//!
//! Test ids: section `recover-unsaved-work`, `recover:<name>`, `discard:<name>`.

use super::eid;
use crate::{
    manager::{AppError, SessionState},
    recovery::RECOVERY_NEEDS_SAVE_AS,
    storage::RecoveryRecord,
    ui::{
        dialogs::{self, DialogProvider},
        forms,
        prelude::*,
        start::StartScreen,
    },
};
use std::sync::Arc;

/// The name a record is shown under.
pub fn record_label(record: &RecoveryRecord) -> String {
    record
        .name
        .clone()
        .filter(|n| !n.is_empty())
        .or_else(|| {
            record
                .document_path
                .as_deref()
                .and_then(|p| p.rsplit(['/', '\\']).next())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "Untitled document".into())
}

/// Recovers crashed work; untitled work asks for a destination (canceling keeps it
/// open, unsaved). Runs on a background thread, so the dialog is the blocking one.
pub fn recover_work(
    dialogs: &Arc<dyn DialogProvider>,
    window: &str,
    record: &RecoveryRecord,
) -> Result<SessionState, AppError> {
    let state = crate::recovery::recover_session(window.into(), record.session_id.clone())?;
    // The saved file could not be safely replaced: ask where to save instead.
    let needs_save_as = state
        .last_error
        .as_ref()
        .is_some_and(|e| e.code == RECOVERY_NEEDS_SAVE_AS);
    if state.path.is_some() && !needs_save_as {
        return Ok(state);
    }
    let default = format!("{}.ixt", record.name.as_deref().unwrap_or("Recovered"));
    match dialogs.save_file("Save ixtable document", &default, &[dialogs::IXT]) {
        Some(path) => crate::save_document_as(window.into(), path.to_string_lossy().into_owned()),
        None => Ok(state),
    }
}

pub struct RecoveryList {
    screen: WeakEntity<StartScreen>,
    records: Vec<RecoveryRecord>,
    was_busy: bool,
    _subscription: Option<Subscription>,
}

impl RecoveryList {
    pub fn new(screen: &Entity<StartScreen>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(screen, |this, screen, cx| {
            let busy = screen.read(cx).is_busy();
            if this.was_busy && !busy {
                this.refresh(cx);
            }
            this.was_busy = busy;
            cx.notify();
        });
        let mut this = Self {
            screen: screen.downgrade(),
            records: vec![],
            was_busy: false,
            _subscription: Some(subscription),
        };
        this.refresh(cx);
        this
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        task::spawn_call(
            cx,
            || crate::list_recovery_sessions().unwrap_or_default(),
            |this, mut records, cx| {
                // Newest work first.
                records.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
                this.records = records;
                cx.notify();
            },
        );
    }

    fn recover(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(record) = self.records.get(index).cloned() else {
            return;
        };
        let provider = cx.global::<dialogs::Dialogs>().0.clone();
        let pending = format!("Recovering {}…", record_label(&record));
        let _ = self.screen.update(cx, |screen, cx| {
            screen.run(
                &pending,
                move |label| recover_work(&provider, &label, &record),
                cx,
            )
        });
    }

    fn discard(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(record) = self.records.get(index).cloned() else {
            return;
        };
        let name = record_label(&record);
        let ask = dialogs::confirm(
            cx,
            "Discard unsaved work",
            &format!("Discard the unsaved work in {name}? This cannot be undone."),
        );
        task::after(cx, ask, move |_, ok, cx| {
            if !ok {
                return;
            }
            let id = record.session_id.clone();
            task::spawn_call(
                cx,
                move || crate::discard_recovery(id),
                move |this, result, cx| {
                    if let Err(e) = result {
                        let _ = this.screen.update(cx, |s, cx| s.set_error(e, cx));
                    }
                    this.refresh(cx);
                },
            );
        });
    }
}

impl Render for RecoveryList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self
            .screen
            .upgrade()
            .is_some_and(|s| s.read(cx).is_busy());
        if self.records.is_empty() {
            return div().into_any_element();
        }
        let mut card = forms::card("recover-unsaved-work", "Recover unsaved work", cx)
            .child(forms::muted(
                "ixtable closed before these changes were saved. Recover them to save them back \
                 into their document, or discard them.",
                cx,
            ));
        for (i, r) in self.records.iter().enumerate() {
            let name = record_label(r);
            card = card.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().flex_1().min_w_0().truncate().child(format!(
                        "{name} · {}",
                        r.document_path.as_deref().unwrap_or("Never saved")
                    )))
                    .child(
                        forms::primary(eid(format!("recover:{name}")), "Recover")
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, _, cx| this.recover(i, cx))),
                    )
                    .child(
                        forms::danger(eid(format!("discard:{name}")), "Discard")
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, _, cx| this.discard(i, cx))),
                    ),
            );
        }
        card.into_any_element()
    }
}
