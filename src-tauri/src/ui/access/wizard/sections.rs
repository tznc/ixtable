//! The wizard's two bodies: what the Access file holds, and the import report.

use super::{AccessImportWizard, Imported, grouped, plural};
use crate::{
    access::Inventory,
    ui::{
        access::summary::{
            AccessFormat, items_needing_attention, kind_label, status_counts,
            total_rows,
        },
        display::{self, status_badge},
        forms::{self, eid},
        prelude::*,
        widgets::Severity,
    },
};

/// The plural object label for `n` of them: "1 Report" shows "Report", 2 show "Reports".
fn count_word(label: &str, n: usize) -> String {
    if n == 1 { widgets::singular(label) } else { label.to_string() }
}

#[cfg(test)]
mod tests {
    use super::count_word;

    #[test]
    fn object_counts_use_the_singular_for_one() {
        assert_eq!(count_word("Reports", 1), "Report");
        assert_eq!(count_word("Modules", 1), "Module");
        assert_eq!(count_word("Queries", 1), "Query");
        assert_eq!(count_word("Reports", 2), "Reports");
    }
}

/// Attention lists up to this long open expanded.
const SHOW_ATTENTION_UP_TO: usize = 10;

impl AccessImportWizard {
    pub(super) fn contents(&self, inventory: &Inventory, cx: &mut Context<Self>) -> Div {
        let rows = total_rows(inventory);
        let objects = [
            ("Queries", inventory.queries.len()),
            ("Forms", inventory.forms.len()),
            ("Reports", inventory.reports.len()),
            ("Macros", inventory.macros.len()),
            ("Modules", inventory.modules.len()),
        ];
        let compiled = inventory
            .compiled
            .iter()
            .filter(|(kind, _)| kind == "form" || kind == "report")
            .count();
        let format: AccessFormat = inventory.format.into();
        let theme = cx.theme();
        let summary = format!(
            "{}: {}, {}, {}.",
            format.label(),
            plural(inventory.tables.len() as u64, "table"),
            plural(rows, "row"),
            plural(inventory.relationships as u64, "relationship"),
        );
        let mut block = v_flex().gap_3().child(widgets::styled_text("access-summary", summary, |d| {
            d.font_weight(FontWeight::MEDIUM)
        }));
        let counts: Vec<_> = objects.into_iter().filter(|(_, n)| *n > 0).collect();
        if !counts.is_empty() {
            block = block.child(h_flex().gap_2().flex_wrap().children(counts.into_iter().map(|(label, n)| {
                // "1 Report", "2 Reports"; the id keeps the plural label.
                let word = count_word(label, n);
                widgets::observed(
                    h_flex()
                        .id(eid(format!("access-count:{label}")))
                        .aria_label(format!("{n} {word}"))
                        .gap_1()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(theme.border)
                        .text_sm()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(n.to_string()))
                        .child(div().text_color(theme.muted_foreground).child(word)),
                )
            })));
        }
        let table_rows = inventory
            .tables
            .iter()
            .map(|t| {
                let count = t.rows.map_or("—".to_string(), grouped);
                let label = format!(
                    "{}: {}, {}",
                    t.name,
                    plural(t.columns as u64, "field"),
                    t.rows.map_or("rows unknown".to_string(), |r| plural(r, "row"))
                );
                (eid(format!("access-table:{}", t.name)), label, vec![t.name.clone(), t.columns.to_string(), count])
            })
            .collect();
        block = block.child(display::data_table("access-tables", &["Table", "Fields", "Rows"], table_rows, cx));
        if compiled > 0 {
            block = block.child(widgets::callout(
                "access-compiled",
                Severity::Warning,
                &format!(
                    "This file stores its {compiled} forms and reports compiled, so they cannot be read. ixtable creates a list and a detail form for each table instead."
                ),
                None,
                cx,
            ));
        }
        for (i, w) in inventory.warnings.iter().enumerate() {
            block = block.child(widgets::callout(eid(format!("access-warning:{i}")), Severity::Warning, w, None, cx));
        }
        let entity = cx.entity();
        block.child(
            v_flex()
                .gap_1()
                .child(
                    forms::check(
                        "access-include-data",
                        &format!("Import the data ({} rows)", grouped(rows)),
                        self.include_data,
                        move |on, _, cx| {
                            entity.update(cx, |this, cx| {
                                this.include_data = on;
                                cx.notify();
                            })
                        },
                    )
                    .disabled(self.is_busy()),
                )
                .child(forms::muted(
                    "Turn off to import only the structure: tables, queries, forms and reports.",
                    cx,
                )),
        )
    }

    pub(super) fn report(&self, done: &Imported, cx: &mut Context<Self>) -> Div {
        let attention = items_needing_attention(&done.items);
        let theme = cx.theme();
        let verdict = if attention.is_empty() {
            widgets::callout("access-report", Severity::Success, "Everything converted.", None, cx)
        } else {
            widgets::callout(
                "access-report",
                Severity::Warning,
                &format!(
                    "Imported with {} to review.",
                    plural(attention.len() as u64, "item")
                ),
                Some("The list below says what each one lost."),
                cx,
            )
        };
        let source_tables = self.inventory.as_ref().map_or(done.tables, |i| i.tables.len());
        let extra = done.tables.saturating_sub(source_tables);
        let created = if extra > 0 {
            format!(
                "Created a document with {} (plus {} for attachment fields, multi-value fields or make-table queries) and {}.",
                plural(source_tables as u64, "table"),
                plural(extra as u64, "extra table"),
                plural(done.rows, "row"),
            )
        } else {
            format!(
                "Created a document with {} and {}.",
                plural(done.tables as u64, "table"),
                plural(done.rows, "row"),
            )
        };
        let kinds = status_counts(&done.items)
            .into_iter()
            .map(|c| {
                let label = format!(
                    "{}: converted {}, partly {}, not converted {}",
                    kind_label(&c.kind),
                    c.converted,
                    c.partial,
                    c.skipped
                );
                (
                    eid(format!("report-row:{}", c.kind)),
                    label,
                    vec![
                        kind_label(&c.kind),
                        c.converted.to_string(),
                        c.partial.to_string(),
                        c.skipped.to_string(),
                    ],
                )
            })
            .collect();
        let mut block = v_flex()
            .gap_3()
            .child(verdict)
            .child(widgets::text("access-created", created))
            .child(display::data_table(
                "access-kinds",
                &["Kind", "Converted", "Partly", "Not converted"],
                kinds,
                cx,
            ));
        if !attention.is_empty() {
            let shown = self.show_attention.unwrap_or(attention.len() <= SHOW_ATTENTION_UP_TO);
            block = block.child(
                h_flex().child(
                    forms::link(
                        "attention-toggle",
                        &format!("What did not convert fully ({})", attention.len()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.show_attention = Some(!shown);
                        cx.notify();
                    })),
                ),
            );
            if shown {
                let mut list = v_flex().gap_2();
                for item in &attention {
                    let key = format!("{}:{}", item.kind, item.name);
                    let mut entry = v_flex().gap_0p5().child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(widgets::styled_text(
                                eid(format!("attention:{key}")),
                                format!("{} ({})", item.name, item.kind),
                                |d| d.font_weight(FontWeight::SEMIBOLD).text_sm(),
                            ))
                            .child(status_badge(
                                eid(format!("attention-status:{key}")),
                                item.status.label(),
                                item.status.tone(),
                                cx,
                            )),
                    );
                    for (i, note) in item.notes.iter().enumerate() {
                        entry = entry.child(widgets::styled_text(
                            eid(format!("attention-note:{key}:{i}")),
                            note.clone(),
                            |d| d.pl_3().text_sm().text_color(theme.muted_foreground),
                        ));
                    }
                    list = list.child(entry);
                }
                block = block.child(list);
            }
        }
        for (i, w) in done.warnings.iter().enumerate() {
            block = block.child(widgets::callout(eid(format!("report-warning:{i}")), Severity::Warning, w, None, cx));
        }
        block
            .child(widgets::styled_text(
                "access-note-about-settings",
                "Settings › Access migration keeps this report, with links, review marks and export.",
                |d| d.text_sm().text_color(theme.muted_foreground),
            ))
            .child(widgets::styled_text(
                "access-unsaved",
                "The new document is not saved yet. Save it from the sidebar to keep it.",
                |d| d.text_sm().text_color(theme.muted_foreground),
            ))
    }
}
