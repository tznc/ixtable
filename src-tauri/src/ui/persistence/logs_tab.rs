//! Local diagnostic log (PRD 27.5): recent lines with secrets already redacted.
//! Port of src/persistence/LogsTab.tsx.
//!
//! Test ids: `logs-refresh`, `logs-copy`, `logs-count`, `logs-error`,
//! `no-logs`, `log:<i>` (newest first; label "timestamp level area message") with its
//! level badge `log:<i>:level`.

use super::eid;
use crate::{
    logging::LogEntry,
    manager::AppError,
    ui::{
        display::{Tone, status_badge},
        forms,
        prelude::*,
        shell::Shell,
        toast::Toast,
    },
};

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
    shell: Entity<Shell>,
}

impl LogsTab {
    pub fn new(label: String, shell: Entity<Shell>, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            label,
            entries: None,
            error: None,
            shell,
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
        let toast = Toast::success(format!("Copied {} log lines.", entries.len()));
        self.shell.update(cx, |shell, cx| shell.toast(toast, cx));
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
        match &self.entries {
            Some(entries) if entries.is_empty() => {
                panel = panel.child(widgets::text("no-logs", "No log entries yet."));
            }
            Some(entries) => {
                // Newest first.
                let rows = entries.iter().rev().enumerate().map(|(i, e)| log_row(i, e, cx));
                panel = panel.child(v_flex().text_sm().children(rows));
            }
            None => {}
        }
        panel
    }
}

fn level_tone(level: &str) -> Tone {
    match level.to_ascii_lowercase().as_str() {
        "error" => Tone::Danger,
        "warn" | "warning" => Tone::Warning,
        _ => Tone::Neutral,
    }
}

/// One log line: time, level badge, area, then the message, which alone may wrap.
#[cfg(test)]
#[test]
fn tones_log_levels_case_insensitively() {
    assert_eq!(level_tone("ERROR"), Tone::Danger);
    assert_eq!(level_tone("WARN"), Tone::Warning);
    assert_eq!(level_tone("INFO"), Tone::Neutral);
}

fn log_row(i: usize, e: &LogEntry, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (muted, border) = (theme.muted_foreground, theme.border);
    h_flex()
        .id(eid(format!("log:{i}")))
        .aria_label(format!("{} {} {} {}", e.timestamp, e.level, e.area, e.message))
        .gap_3()
        .items_start()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(border.opacity(0.5))
        .child(
            div()
                .flex_none()
                .whitespace_nowrap()
                .font_family("monospace")
                .text_xs()
                .text_color(muted)
                .child(e.timestamp.clone()),
        )
        .child(
            div()
                .w(px(56.))
                .flex_none()
                .child(status_badge(eid(format!("log:{i}:level")), &e.level, level_tone(&e.level), cx)),
        )
        .child(div().w(px(96.)).flex_none().whitespace_nowrap().text_color(muted).child(e.area.clone()))
        .child(div().flex_1().min_w_0().child(e.message.clone()))
        .test_support()
}
