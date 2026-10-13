//! Upload and download progress: a poller that mirrors the Rust transfer table into a
//! view, and the progress bar. Port of src/cloud/transfer.ts and TransferBar.tsx.
//!
//! Test id of the bar: the `id` given to `transfer_bar` (label "LABEL: DONE of TOTAL").

use super::services::CloudServices;
use crate::{cloud::http::Progress, ui::prelude::*};
use std::{sync::Arc, time::Duration};

const POLL: Duration = Duration::from_millis(300);

/// `1.5 KB`, `2.0 MB` (binary units, as the cloud limit messages use).
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1_048_576 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    }
}

/// Polls Rust transfer progress while an upload or download runs.
#[derive(Default)]
pub struct Transfer {
    pub progress: Option<Progress>,
    poll: Option<Task<()>>,
}

impl Transfer {
    /// Starts polling a new transfer and returns its id for the command to report under.
    /// `of` finds this tracker inside the view.
    pub fn begin<V: 'static>(
        &mut self,
        services: Arc<dyn CloudServices>,
        of: fn(&mut V) -> &mut Transfer,
        cx: &mut Context<V>,
    ) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let watched = id.clone();
        self.poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let current = services.transfer_progress(&watched);
                let alive = this
                    .update(cx, |view, cx| {
                        if let Some(next) = current {
                            of(view).progress = Some(next);
                            cx.notify();
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        }));
        id
    }

    /// Stops polling and hides the bar.
    pub fn end(&mut self) {
        self.poll = None;
        self.progress = None;
    }
}

/// The progress bar of a transfer in flight (nothing when there is none).
pub fn transfer_bar(
    id: &'static str,
    label: &str,
    progress: Option<&Progress>,
    cx: &App,
) -> Option<Div> {
    let progress = progress?;
    let fraction = if progress.total == 0 {
        0.0
    } else {
        (progress.done as f32 / progress.total as f32).clamp(0.0, 1.0)
    };
    let theme = cx.theme();
    Some(
        v_flex()
            .gap_1()
            .child(
                div()
                    .h(px(6.))
                    .w_full()
                    .rounded_full()
                    .bg(theme.muted)
                    .child(div().h_full().w(relative(fraction)).rounded_full().bg(theme.primary)),
            )
            .child(widgets::text(
                id,
                format!(
                    "{label}: {} of {}",
                    format_bytes(progress.done),
                    format_bytes(progress.total)
                ),
            )),
    )
}

#[cfg(test)]
mod tests {
    use super::format_bytes;

    #[test]
    fn formats_binary_sizes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KB");
        assert_eq!(format_bytes(5 * 1_048_576), "5.0 MB");
    }
}
