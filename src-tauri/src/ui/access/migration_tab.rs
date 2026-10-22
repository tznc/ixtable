//! Settings > Access migration: everything an Access import converted, with what each
//! object lost, links to the result, review marks, and export
//! (docs/decisions/access-import.md, "Migration report"). Port of
//! src/access/MigrationTab.tsx.
//!
//! Test ids: heading `access-migration`, `access-not-imported`, `access-intro`,
//! table `access-summary` with rows `summary:<kind>` ("Tables: converted C, partly P,
//! not converted N"), filters
//! `access-status:<attention|skipped|partial|converted|all>`, `access-kind:<all|kind>`,
//! input `access-search`, checkbox `access-hide-reviewed`, `export-md`, `export-csv`,
//! `access-error`, `access-no-items`; per item (key `kind:name`)
//! `item:<key>` (the name), `item-kind:<key>`, badge `item-status:<key>`, `item-note:<key>:<i>`,
//! `reviewed:<key>` (label "Reviewed"), `open-item:<key>` ("Open form name");
//! warning callouts `access-warning:<i>`.

use super::{
    migration::{
        AccessMigration, MigrationFilter, StatusFilter, filter_items, item_key, open_count,
        read_migration, to_csv, to_markdown, with_reviewed,
    },
    summary::{ImportTarget, ItemStatus, KIND_ORDER, kind_label, status_counts},
};
use crate::{
    manager::AppError,
    ui::{
        dialogs::{self, FileFilter},
        display::{data_table, status_badge},
        forms::{self, eid},
        prelude::*,
        shell::{AutomationTab, RevealTarget, Shell},
        store::StoreEvent,
        toast::Toast,
        widgets::Severity,
    },
};
use gpui_kit::component::input::{Input, InputEvent, InputState};

const MARKDOWN: FileFilter = FileFilter { name: "Markdown", extensions: &["md"] };
const CSV: FileFilter = FileFilter { name: "CSV", extensions: &["csv"] };

pub struct AccessMigrationTab {
    shell: Entity<Shell>,
    filter: MigrationFilter,
    search: Entity<InputState>,
    error: Option<AppError>,
    _subscriptions: Vec<Subscription>,
}

fn reveal_for(target: &ImportTarget) -> Option<RevealTarget> {
    let object_id = target.id.clone();
    match target.kind.as_str() {
        "form" => Some(RevealTarget::Design { object_id, element_id: None }),
        "report" => Some(RevealTarget::Reports { object_id, element_id: None }),
        "action" => Some(RevealTarget::Automation { tab: AutomationTab::Actions, object_id }),
        _ => None,
    }
}

fn open_label(kind: &str) -> &'static str {
    match kind {
        "form" => "Open form",
        "report" => "Open report",
        _ => "Open action",
    }
}

impl AccessMigrationTab {
    pub fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = shell.read(cx).store.clone();
        let search = forms::line_state(window, cx, "Search", "");
        let subscriptions = vec![
            cx.subscribe(&store, |_, _, e: &StoreEvent, cx| {
                if matches!(e, StoreEvent::Changed) {
                    cx.notify();
                }
            }),
            cx.subscribe(&search, |this, state, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    this.filter.search = forms::text_of(&state, cx);
                    cx.notify();
                }
            }),
        ];
        Self {
            shell,
            filter: MigrationFilter::new(StatusFilter::Attention),
            search,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    fn migration(&self, cx: &App) -> Option<AccessMigration> {
        read_migration(&self.shell.read(cx).config(cx).settings)
    }

    fn mark(&mut self, key: String, on: bool, cx: &mut Context<Self>) {
        let store = self.shell.read(cx).store.clone();
        let label = if on { "Mark Access item reviewed" } else { "Unmark Access item reviewed" };
        let result = store.update(cx, |s, cx| {
            s.update(label, |c| c.settings = with_reviewed(&c.settings, &key, on), cx)
        });
        if let Err(e) = result {
            self.error = Some(e);
        }
        cx.notify();
    }

    fn export(&mut self, markdown: bool, cx: &mut Context<Self>) {
        self.error = None;
        let Some(migration) = self.migration(cx) else { return };
        let name = self.shell.read(cx).config(cx).name.clone();
        let title = if name.is_empty() { "Access".to_string() } else { name };
        let extension = if markdown { "md" } else { "csv" };
        let pick = dialogs::save_file(
            cx,
            "Export migration report",
            &format!("{title} migration report.{extension}"),
            if markdown { &[MARKDOWN] } else { &[CSV] },
        );
        task::after(cx, pick, move |this, path, cx| {
            let Some(path) = path else { return };
            let path = path.to_string_lossy().into_owned();
            let text = if markdown { to_markdown(&migration, &title) } else { to_csv(&migration) };
            let label = this.shell.read(cx).label().to_string();
            let shown = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
            task::spawn_call(
                cx,
                move || crate::access::write_access_report(label, path, text),
                move |this, result, cx| {
                    match result {
                        Ok(()) => this.shell.update(cx, |shell, cx| shell.toast(Toast::success(format!("Saved {shown}")), cx)),
                        Err(e) => this.error = Some(e),
                    }
                    cx.notify();
                },
            );
        });
    }

    fn filter_button(
        &self,
        id: String,
        label: &str,
        active: bool,
        set: impl Fn(&mut MigrationFilter) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        forms::secondary(eid(id), label)
            .selected_tint(active, cx)
            .on_click(cx.listener(move |this, _, _, cx| {
                set(&mut this.filter);
                cx.notify();
            }))
    }
}

impl Render for AccessMigrationTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut tab = v_flex()
            .gap_3()
            .child(widgets::section_heading("access-migration", "Access migration"));
        let Some(migration) = self.migration(cx) else {
            return tab.child(widgets::text(
                "access-not-imported",
                "This document was not imported from Microsoft Access.",
            ));
        };
        let items = filter_items(&migration, &self.filter);
        let reviewed = &migration.reviewed;
        let open = open_count(&migration);
        let source = if migration.source.is_empty() { "an Access file" } else { &migration.source };
        let format = migration.format.label().replacen("Access", "an Access", 1);
        let on = migration
            .imported_at
            .as_deref()
            .map(|at| format!(" on {}", at.chars().take(10).collect::<String>()))
            .unwrap_or_default();
        let review = if open == 0 {
            "Every item that needs attention is reviewed.".to_string()
        } else {
            format!(
                "{open} item{} still need{} review.",
                if open == 1 { "" } else { "s" },
                if open == 1 { "s" } else { "" }
            )
        };
        tab = tab.child(widgets::text(
            "access-intro",
            format!("Imported from {source}, {format}{on}. {review}"),
        ));
        let counts = status_counts(&migration.report);
        tab = tab.child(data_table(
            "access-summary",
            &["Kind", "Converted", "Partly", "Not converted"],
            counts
                .iter()
                .map(|c| {
                    let kind = kind_label(&c.kind);
                    let label = format!(
                        "{kind}: converted {}, partly {}, not converted {}",
                        c.converted, c.partial, c.skipped
                    );
                    let cells = vec![kind, c.converted.to_string(), c.partial.to_string(), c.skipped.to_string()];
                    (eid(format!("summary:{}", c.kind)), label, cells)
                })
                .collect(),
            cx,
        ));
        // Filters.
        let mut statuses = forms::actions().child(widgets::text("access-show", "Show"));
        let options: [(&str, &str, StatusFilter); 5] = [
            ("attention", "Needs attention", StatusFilter::Attention),
            ("skipped", "Not converted", StatusFilter::Status(ItemStatus::Skipped)),
            ("partial", "Partly converted", StatusFilter::Status(ItemStatus::Partial)),
            ("converted", "Converted", StatusFilter::Status(ItemStatus::Converted)),
            ("all", "Everything", StatusFilter::All),
        ];
        for (key, label, status) in options {
            statuses = statuses.child(self.filter_button(
                format!("access-status:{key}"),
                label,
                self.filter.status == status,
                move |f| f.status = status,
                cx,
            ));
        }
        let mut kinds = forms::actions().child(widgets::text("access-kind-label", "Kind")).child(
            self.filter_button(
                "access-kind:all".into(),
                "All kinds",
                self.filter.kind.is_empty(),
                |f| f.kind.clear(),
                cx,
            ),
        );
        for kind in KIND_ORDER.iter().filter(|k| migration.report.iter().any(|i| i.kind == **k)) {
            let kind = kind.to_string();
            kinds = kinds.child(self.filter_button(
                format!("access-kind:{kind}"),
                &kind_label(&kind),
                self.filter.kind == kind,
                {
                    let kind = kind.clone();
                    move |f| f.kind = kind.clone()
                },
                cx,
            ));
        }
        let entity = cx.entity();
        tab = tab
            .child(statuses)
            .child(kinds)
            .child(widgets::field(
                "Search",
                Input::new(&self.search).id("access-search").aria_label("Search"),
                cx,
            ))
            .child(forms::check(
                "access-hide-reviewed",
                "Hide reviewed",
                self.filter.hide_reviewed,
                move |on, _, cx| {
                    entity.update(cx, |this, cx| {
                        this.filter.hide_reviewed = on;
                        cx.notify();
                    })
                },
            ))
            .child(
                forms::actions()
                    .child(
                        forms::secondary("export-md", "Export Markdown…")
                            .on_click(cx.listener(|this, _, _, cx| this.export(true, cx))),
                    )
                    .child(
                        forms::secondary("export-csv", "Export CSV…")
                            .on_click(cx.listener(|this, _, _, cx| this.export(false, cx))),
                    ),
            );
        if let Some(e) = &self.error {
            tab = tab.child(widgets::error_banner("access-error", e, None, cx));
        }
        if items.is_empty() {
            tab = tab.child(widgets::text("access-no-items", "No items match."));
        }
        let (muted, border) = (cx.theme().muted_foreground, cx.theme().border);
        for item in &items {
            let key = item_key(item);
            let toggle_key = key.clone();
            let entity = cx.entity();
            // One row: name, kind and status on the left; open link and review mark right.
            let mut head = h_flex()
                .gap_2()
                .items_center()
                .child(widgets::styled_text(eid(format!("item:{key}")), item.name.clone(), |d| {
                    d.font_weight(FontWeight::MEDIUM)
                }))
                .child(widgets::styled_text(eid(format!("item-kind:{key}")), item.kind.clone(), move |d| {
                    d.ty(Type::Caption).text_color(muted)
                }))
                .child(status_badge(eid(format!("item-status:{key}")), item.status.label(), item.status.tone(), cx))
                .child(div().flex_1());
            if let Some(target) = item.target.as_ref().and_then(|t| reveal_for(t).map(|r| (t, r))) {
                let (target, reveal) = target;
                head = head.child(
                    forms::link(
                        eid(format!("open-item:{key}")),
                        &format!("{} {}", open_label(&target.kind), item.name),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let reveal = reveal.clone();
                        this.shell.update(cx, |s, cx| s.request_reveal(reveal, window, cx));
                    })),
                );
            }
            head = head.child(forms::check(
                eid(format!("reviewed:{key}")),
                "Reviewed",
                reviewed.contains(&key),
                move |on, _, cx| {
                    let key = toggle_key.clone();
                    entity.update(cx, |this, cx| this.mark(key, on, cx))
                },
            ));
            let mut row = v_flex().gap_1().px_2().py_1p5().border_b_1().border_color(border.opacity(0.5)).child(head);
            for (i, note) in item.notes.iter().enumerate() {
                row = row.child(widgets::styled_text(eid(format!("item-note:{key}:{i}")), note.clone(), move |d| {
                    d.ty(Type::Body).text_color(muted)
                }));
            }
            tab = tab.child(row);
        }
        for (i, w) in migration.warnings.iter().enumerate() {
            tab = tab.child(widgets::callout(eid(format!("access-warning:{i}")), Severity::Warning, w, None, cx));
        }
        tab
    }
}
