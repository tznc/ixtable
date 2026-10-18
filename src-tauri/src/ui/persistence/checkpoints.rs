//! Local checkpoints of this document: create one now, or restore one as a copy.
//! Port of src/persistence/CheckpointsPanel.tsx.
//!
//! Test ids: card `checkpoints`, `create-checkpoint`, `checkpoint-error`,
//! `checkpoint:<i>`, `restore-checkpoint:<i>`, `no-checkpoints`.

use super::{
    eid,
    format::{format_bytes, format_date_time},
};
use crate::{
    checkpoints::CheckpointInfo,
    manager::AppError,
    ui::{dialogs, forms, prelude::*, shell::Shell, toast::Toast},
};

pub struct CheckpointsPanel {
    shell: Entity<Shell>,
    checkpoints: Vec<CheckpointInfo>,
    busy: bool,
    error: Option<AppError>,
    /// Only the newest listing may land: a slow first load must not overwrite the list
    /// fetched after a create.
    latest: u64,
}

impl CheckpointsPanel {
    pub fn new(shell: Entity<Shell>, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            shell,
            checkpoints: vec![],
            busy: false,
            error: None,
            latest: 0,
        };
        this.refresh(None, cx);
        this
    }

    fn label(&self, cx: &App) -> String {
        self.shell.read(cx).label().to_string()
    }

    /// Whether a listing started as `request` may replace the shown one.
    fn accept(&mut self, request: u64, listing: Vec<CheckpointInfo>) -> bool {
        if request != self.latest {
            return false;
        }
        self.checkpoints = listing;
        true
    }

    /// Lists checkpoints, then announces `message` (never before the list it describes).
    fn refresh(&mut self, message: Option<String>, cx: &mut Context<Self>) {
        self.latest += 1;
        let request = self.latest;
        let label = self.label(cx);
        task::spawn_call(
            cx,
            move || crate::checkpoints::list_checkpoints(label),
            move |this, result, cx| {
                match result {
                    Ok(listing) => {
                        this.accept(request, listing);
                    }
                    Err(e) => this.error = Some(e),
                }
                if let Some(message) = message {
                    this.busy = false;
                    this.shell.update(cx, |shell, cx| shell.toast(Toast::success(message), cx));
                }
                cx.notify();
            },
        );
    }

    fn begin(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        cx.notify();
    }

    fn fail(&mut self, e: AppError, cx: &mut Context<Self>) {
        self.busy = false;
        self.error = Some(e);
        cx.notify();
    }

    fn create(&mut self, cx: &mut Context<Self>) {
        self.begin(cx);
        let label = self.label(cx);
        task::spawn_call(
            cx,
            move || crate::checkpoints::create_checkpoint(label, Some("manual".into())),
            |this, result, cx| match result {
                Ok(_) => this.refresh(Some("Checkpoint created.".into()), cx),
                Err(e) => this.fail(e, cx),
            },
        );
    }

    fn restore(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(checkpoint) = self.checkpoints.get(index).cloned() else {
            return;
        };
        self.begin(cx);
        let name = self.shell.read(cx).doc.name.clone();
        let pick = dialogs::save_file(
            cx,
            "Restore checkpoint as a copy",
            &format!("{name} (restored).ixt"),
            &[dialogs::IXT],
        );
        task::after(cx, pick, move |this, path, cx| {
            let Some(path) = path else {
                this.busy = false;
                this.shell.update(cx, |shell, cx| shell.toast(Toast::info("Restore canceled."), cx));
                cx.notify();
                return;
            };
            let label = this.label(cx);
            let dest = path.to_string_lossy().into_owned();
            task::spawn_call(
                cx,
                move || crate::checkpoints::restore_checkpoint_as_copy(label, checkpoint.id, dest),
                |this, result, cx| match result {
                    Ok(written) => this.refresh(
                        Some(format!("Restored a copy to {written}. Open it from Recent documents.")),
                        cx,
                    ),
                    Err(e) => this.fail(e, cx),
                },
            );
        });
    }
}

impl Render for CheckpointsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut card = forms::card("checkpoints", "Checkpoints", cx)
            .child(forms::muted(
                "Local copies of this application, including unsaved changes. ixtable also \
                 creates one before migrations and recovery.",
                cx,
            ))
            .child(
                forms::secondary("create-checkpoint", "Create checkpoint")
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, _, cx| this.create(cx))),
            );
        if let Some(e) = &self.error {
            card = card.child(widgets::error_banner("checkpoint-error", e, None, cx));
        }
        if self.checkpoints.is_empty() {
            return card.child(widgets::text("no-checkpoints", "No checkpoints yet."));
        }
        for (i, c) in self.checkpoints.iter().enumerate() {
            let when = format_date_time(&c.created_at);
            card = card.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(widgets::text(
                        eid(format!("checkpoint:{i}")),
                        format!("{when} {} · {}", c.reason, format_bytes(c.size)),
                    ))
                    .child(
                        forms::secondary(eid(format!("restore-checkpoint:{i}")), "Restore as copy")
                            .disabled(self.busy)
                            .on_click(cx.listener(move |this, _, _, cx| this.restore(i, cx))),
                    ),
            );
        }
        card
    }
}

#[cfg(test)]
mod tests {
    use super::{CheckpointInfo, CheckpointsPanel};
    use crate::ui::testing::Harness;
    use gpui_kit::{AppContext as _, TestAppContext};

    fn info(id: &str) -> CheckpointInfo {
        CheckpointInfo {
            id: id.into(),
            document_id: "d".into(),
            reason: "manual".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            path: String::new(),
            size: 1,
        }
    }

    // Port of tests/unit/checkpoints-panel.test.tsx: a slow first listing must not
    // overwrite the newer one fetched after a create.
    #[gpui_kit::test]
    fn a_late_listing_cannot_overwrite_a_newer_one(cx: &mut TestAppContext) {
        let mut h = Harness::new_document(cx);
        let shell = h.shell();
        let panel = h.cx.update(|cx| cx.new(|cx| CheckpointsPanel::new(shell, cx)));
        h.settle();
        h.cx.update(|cx| {
            panel.update(cx, |p, _| {
                p.latest = 2;
                assert!(p.accept(2, vec![info("new")]));
                assert!(!p.accept(1, vec![info("stale")]));
                assert_eq!(p.checkpoints.len(), 1);
                assert_eq!(p.checkpoints[0].id, "new");
            })
        });
    }
}
