//! Archive size by section, largest entries and the cloud limit (PRD 7.4).
//! Port of src/persistence/ArchiveSizePanel.tsx.
//!
//! Test ids: card `archive-size`, `measure-archive`, `size-error`, `size-summary`,
//! `size-limit`, `size-over-limit`, `size-section:<Records|Configuration|Assets|Other>`,
//! `size-largest:<i>`.

use super::{
    eid,
    format::{format_bytes, format_percent},
};
use crate::{
    assets::ArchiveSizeReport,
    manager::AppError,
    ui::{forms, prelude::*},
};

pub struct ArchiveSizePanel {
    label: String,
    report: Option<ArchiveSizeReport>,
    error: Option<AppError>,
    busy: bool,
}

impl ArchiveSizePanel {
    pub fn new(label: String, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            label,
            report: None,
            error: None,
            busy: false,
        };
        this.measure(cx);
        this
    }

    pub fn measure(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        let label = self.label.clone();
        task::spawn_call(
            cx,
            move || crate::assets::archive_size_report(label),
            |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(report) => this.report = Some(report),
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    pub fn report(&self) -> Option<&ArchiveSizeReport> {
        self.report.as_ref()
    }
}

impl Render for ArchiveSizePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut card = forms::card("archive-size", "Archive size", cx)
            .child(
                forms::actions()
                    .child(
                        forms::secondary("measure-archive", "Measure archive size")
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.measure(cx))),
                    )
                    .when(self.busy, |el| el.child("Measuring…")),
            );
        if let Some(e) = &self.error {
            card = card.child(widgets::error_banner("size-error", e, None, cx));
        }
        if let Some(r) = &self.report {
            card = card.child(widgets::text(
                "size-summary",
                format!(
                    "{} in total; assets take {}. {}",
                    format_bytes(r.total_bytes),
                    format_percent(r.asset_share),
                    if r.measured == "saved" {
                        "Measured on the saved archive."
                    } else {
                        "Measured on a snapshot that includes unsaved changes."
                    }
                ),
            ));
            card = if r.over_cloud_limit {
                card.child(widgets::text(
                    "size-over-limit",
                    format!(
                        "Over the cloud limit: this archive is larger than {}. It keeps working \
                         locally but cannot sync to ixtable Cloud until it is smaller.",
                        format_bytes(r.cloud_limit_bytes)
                    ),
                ))
            } else {
                card.child(widgets::text(
                    "size-limit",
                    format!("Within the {} cloud limit.", format_bytes(r.cloud_limit_bytes)),
                ))
            };
            for (name, bytes) in [
                ("Records", r.records_bytes),
                ("Configuration", r.config_bytes),
                ("Assets", r.assets_bytes),
                ("Other", r.other_bytes),
            ] {
                card = card.child(widgets::text(
                    eid(format!("size-section:{name}")),
                    format!("{name}: {}", format_bytes(bytes)),
                ));
            }
            card = card.child(div().font_weight(FontWeight::MEDIUM).child("Largest entries"));
            for (i, e) in r.largest.iter().enumerate() {
                card = card.child(widgets::text(
                    eid(format!("size-largest:{i}")),
                    format!("{} ({}) {}", e.name, e.section, format_bytes(e.bytes)),
                ));
            }
        }
        card
    }
}
