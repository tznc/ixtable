//! Local diagnostic log (PRD 27.5): recent lines with secrets already redacted.
//! Port of src/persistence/LogsTab.tsx.
//!
//! Test ids: `logs-refresh`, `logs-copy`, `logs-count`, `logs-status`, `logs-error`,
//! `no-logs`, `log:<i>` (newest first; label "timestamp level area message").

use super::eid;
use crate::{logging::LogEntry, manager::AppError, ui::{forms, prelude::*}};

const LIMIT: usize = 200;

pub fn as_text(entries: &[LogEntry]) -> String {
    entries
        .iter()
        .map(|e| format!("{}\t{}\t{}\t{}", e.timestamp, e.level, e.area, e.message))
        .collect::<Vec<_>>()
        .join("\n")
}

pub struct LogsTab {
    label: String,
    entries: Option<Vec<LogEntry>>,
    error: Option<AppError>,
    status: String,
}

impl LogsTab {
    pub fn new(label: String, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            label,
            entries: None,
            error: None,
            status: String::new(),
        };
        this.refresh(cx);
        this
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        let label = self.label.clone();
        task::spawn_call(
            cx,
            move || crate::logging::read_logs(label, Some(LIMIT)),
            |this, result, cx| {
                match result {
                    Ok(entries) => this.entries = Some(entries),
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        let entries = self.entries.clone().unwrap_or_default();
        cx.write_to_clipboard(ClipboardItem::new_string(as_text(&entries)));
        self.status = format!("Copied {} log lines.", entries.len());
        cx.notify();
    }
}

impl Render for LogsTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.entries.as_ref().map(Vec::len);
        let mut panel = v_flex()
            .gap_3()
            .child(widgets::section_heading("logs-heading", "Logs"))
            .child(forms::muted(
                "Saves, autosaves, recovery, checkpoints, and format upgrades are logged on this \
                 computer only. Passwords and connection-string credentials are removed before \
                 writing.",
                cx,
            ))
            .child(
                forms::actions()
                    .child(
                        forms::secondary("logs-refresh", "Refresh").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.status.clear();
                                this.refresh(cx);
                            },
                        )),
                    )
                    .child(
                        forms::secondary("logs-copy", "Copy")
                            .disabled(count.unwrap_or(0) == 0)
                            .on_click(cx.listener(|this, _, _, cx| this.copy(cx))),
                    )
                    .child(widgets::text(
                        "logs-count",
                        count.map_or("Loading…".to_string(), |n| format!("{n} recent lines")),
                    )),
            );
        if let Some(e) = &self.error {
            panel = panel.child(widgets::error_banner("logs-error", e, None, cx));
        }
        if !self.status.is_empty() {
            panel = panel.child(widgets::text("logs-status", self.status.clone()));
        }
        match &self.entries {
            Some(entries) if entries.is_empty() => {
                panel = panel.child(widgets::text("no-logs", "No log entries yet."));
            }
            Some(entries) => {
                // Newest first.
                for (i, e) in entries.iter().rev().enumerate() {
                    panel = panel.child(widgets::styled_text(
                        eid(format!("log:{i}")),
                        format!("{} {} {} {}", e.timestamp, e.level, e.area, e.message),
                        |d| d.text_sm().font_family("monospace"),
                    ));
                }
            }
            None => {}
        }
        panel
    }
}
