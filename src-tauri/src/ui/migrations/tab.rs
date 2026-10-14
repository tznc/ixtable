//! Settings > Migrations: author, preview, dry-run, apply and roll back migrations
//! (PRD 24). Port of src/migrations/index.tsx.
//!
//! Test ids: heading `migrations`, `migrations-disabled` (PostgreSQL note), buttons
//! `new-migration`, `dry-run` , `apply-migrations` (label "Apply pending (N)"),
//! `rollback-migration`, `migrations-error`, `migration-issue:<i>`; per migration (by
//! name) `migration-order:<name>`, `migration-name:<name>`, `migration-target:<name>`,
//! `migration-status:<name>`, `edit-migration:<name>`; sections `dry-run-result`
//! (`dry-run-status`), `last-run` (`run-status`), `after-problems`
//! (`after-problem:<i>`), `migration-log` (`log-empty`); log entries are
//! `<section>:<i>` (line), `<section>:<i>:status`, `:health`, `:error`, `:recovery`
//! with section `dry`, `run` or `history`. The editor adds its own (see `editor`).

use super::editor::{EditorEvent, MigrationEditor};
use crate::{
    manager::AppError,
    migrations::{
        Migration, MigrationLog,
        commands::{MigrationRun, MigrationStatus},
    },
    ui::{forms, forms::eid, prelude::*, shell::Shell, store::StoreEvent},
    validation::{Issue, Severity},
};
use gpui_kit::component::Disableable as _;
use std::collections::HashSet;

pub struct MigrationsTab {
    shell: Entity<Shell>,
    status: Vec<MigrationStatus>,
    history: Vec<MigrationLog>,
    issues: Vec<Issue>,
    editing: Option<Entity<MigrationEditor>>,
    run: Option<MigrationRun>,
    dry: Option<MigrationLog>,
    after: Option<Vec<Issue>>,
    // A dry run refused in preflight blocks Apply until the migrations change: Apply
    // would be refused too.
    preflight_failed: bool,
    error: Option<AppError>,
    busy: bool,
    _subscriptions: Vec<Subscription>,
}

/// Old documents may say `any` (runs on SQLite) or `postgres` (not supported).
fn target_label(target: &str) -> &'static str {
    if target == "postgres" { "PostgreSQL (not supported)" } else { "SQLite" }
}

fn issue_key(i: &Issue) -> String {
    format!("{}|{}|{}", i.object_kind, i.object_id, i.message)
}

impl MigrationsTab {
    pub fn new(shell: Entity<Shell>, cx: &mut Context<Self>) -> Self {
        let store = shell.read(cx).store.clone();
        let subscriptions = vec![cx.subscribe(&store, |this, _, e: &StoreEvent, cx| {
            if matches!(e, StoreEvent::Changed) {
                this.preflight_failed = false;
                this.refresh(cx);
            }
        })];
        // The definition may have changed behind the store: load it, after this render.
        let reload_store = store.clone();
        cx.defer(move |cx| {
            let _ = reload_store.update(cx, |s, cx| s.reload(None, cx));
        });
        let mut this = Self {
            shell,
            status: vec![],
            history: vec![],
            issues: vec![],
            editing: None,
            run: None,
            dry: None,
            after: None,
            preflight_failed: false,
            error: None,
            busy: false,
            _subscriptions: subscriptions,
        };
        this.refresh(cx);
        this
    }

    fn label(&self, cx: &App) -> String {
        self.shell.read(cx).label().to_string()
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let label = self.label(cx);
        task::spawn_call(
            cx,
            move || {
                let status = crate::migrations::commands::migration_status(label.clone())?;
                let history = crate::migrations::commands::migration_history(label.clone())?;
                let issues = crate::validation::validate_document(label)?
                    .into_iter()
                    .filter(|i| i.object_kind == "migration")
                    .collect::<Vec<_>>();
                Ok::<_, AppError>((status, history, issues))
            },
            |this, result, cx| {
                match result {
                    Ok((status, history, issues)) => {
                        this.status = status;
                        this.history = history;
                        this.issues = issues;
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
    }

    /// The declared migrations in order.
    fn declared(&self, cx: &App) -> Vec<Migration> {
        let mut all = self.shell.read(cx).config(cx).migrations.clone();
        all.sort_by_key(|m| m.order);
        all
    }

    fn is_postgres(&self, cx: &App) -> bool {
        self.shell.read(cx).config(cx).datasource.is_postgres()
    }

    fn status_of(&self, id: &str) -> Option<&MigrationStatus> {
        self.status.iter().find(|s| s.id == id)
    }

    fn open_editor(
        &mut self,
        migration: Migration,
        is_new: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let applied = self.status_of(&migration.id).is_some_and(|s| s.applied);
        let others = self
            .declared(cx)
            .into_iter()
            .filter(|m| m.id != migration.id)
            .map(|m| (m.id, m.name))
            .collect();
        let shell = self.shell.clone();
        let editor = cx.new(|cx| MigrationEditor::new(shell, migration, is_new, applied, others, window, cx));
        // Closing applies only to the editor the event came from: the user may already
        // have opened another one.
        cx.subscribe(&editor, |this, source, _: &EditorEvent, cx| {
            if this.editing.as_ref() == Some(&source) {
                this.editing = None;
                cx.notify();
            }
        })
        .detach();
        self.editing = Some(editor);
        self.after = None;
        cx.notify();
    }

    fn new_migration(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let all = self.declared(cx);
        let migration = Migration {
            id: uuid::Uuid::new_v4().to_string(),
            name: format!("Migration {}", all.len() + 1),
            order: all.iter().map(|m| m.order).max().unwrap_or(0) + 1,
            ..Migration::default()
        };
        self.open_editor(migration, true, window, cx);
    }

    fn dry_run(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        self.dry = None;
        self.preflight_failed = true;
        cx.notify();
        let label = self.label(cx);
        task::spawn_call(
            cx,
            move || crate::migrations::commands::dry_run_migrations(label, None),
            |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(log) => {
                        this.dry = Some(log);
                        this.preflight_failed = false;
                    }
                    Err(e) => this.error = Some(e),
                }
                this.refresh(cx);
                cx.notify();
            },
        );
    }

    // Runs apply or rollback and lists only the definition problems the run introduced.
    fn run_and_check(&mut self, rollback: bool, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        self.after = None;
        cx.notify();
        let label = self.label(cx);
        task::spawn_call(
            cx,
            move || {
                let before: HashSet<String> = crate::validation::validate_document(label.clone())?
                    .iter()
                    .map(issue_key)
                    .collect();
                let run = if rollback {
                    crate::migrations::commands::rollback_migration(label.clone())?
                } else {
                    crate::migrations::commands::apply_migrations(label.clone())?
                };
                let after = if run.ok {
                    Some(
                        crate::validation::validate_document(label)?
                            .into_iter()
                            .filter(|i| i.object_kind != "migration" && !before.contains(&issue_key(i)))
                            .collect::<Vec<_>>(),
                    )
                } else {
                    None
                };
                Ok::<_, AppError>((run, after))
            },
            |this, result, cx| {
                this.busy = false;
                match result {
                    Ok((run, after)) => {
                        this.run = Some(run);
                        this.after = after;
                        let shell = this.shell.clone();
                        shell.update(cx, |s, cx| {
                            s.mark_dirty(cx);
                            s.database_changed(cx);
                        });
                    }
                    Err(e) => this.error = Some(e),
                }
                this.refresh(cx);
                cx.notify();
            },
        );
    }
}

fn log_entry(section: &str, i: usize, log: &MigrationLog) -> Div {
    let key = |suffix: &str| eid(format!("{section}:{i}{suffix}"));
    let mut times = String::new();
    if !log.started_at.is_empty() && !log.finished_at.is_empty() {
        times = format!(" · {} to {}", log.started_at, log.finished_at);
    }
    let mut entry = v_flex().gap_1().child(
        h_flex()
            .gap_2()
            .child(widgets::styled_text(key(""), format!("{} · {}{times}", log.name, log.direction), |d| d))
            .child(widgets::styled_text(key(":status"), log.status.clone(), |d| {
                d.font_weight(FontWeight::SEMIBOLD)
            })),
    );
    if !log.health.is_empty() {
        let failed = log.health.iter().any(|h| h.starts_with("failed"));
        entry = entry.child(widgets::text(
            key(":health"),
            format!("Health {}: {}", if failed { "failed" } else { "passed" }, log.health.join(", ")),
        ));
    }
    if let Some(error) = &log.error {
        entry = entry.child(widgets::text(key(":error"), error.clone()));
    }
    if let Some(recovery) = &log.recovery {
        entry = entry.child(widgets::text(key(":recovery"), format!("Recovery: {recovery}")));
    }
    entry
}

fn issue_line(id: ElementId, issue: &Issue, cx: &App) -> impl IntoElement {
    let danger = (issue.severity == Severity::Error).then(|| cx.theme().danger);
    widgets::styled_text(id, issue.message.clone(), move |d| match danger {
        Some(color) => d.text_color(color),
        None => d,
    })
}

impl Render for MigrationsTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let postgres = self.is_postgres(cx);
        let declared = self.declared(cx);
        let pending = self.status.iter().filter(|s| !s.applied && s.applies_to_store).count();
        let (busy, preflight_failed) = (self.busy, self.preflight_failed);
        let any_applied = self.status.iter().any(|s| s.applied);
        let mut tab = v_flex()
            .gap_3()
            .child(widgets::section_heading("migrations", "Migrations"))
            .child(forms::muted(
                "Ordered SQL changes to the embedded SQLite store, with immutable ids. Applying takes a recovery checkpoint first, runs each migration in one transaction, checks database health, and logs the result.",
                cx,
            ));
        if postgres {
            tab = tab.child(widgets::text(
                "migrations-disabled",
                "Migrations apply to the embedded SQLite store only and are disabled for this document because it uses PostgreSQL. Manage the external database schema yourself.",
            ));
        }
        tab = tab.child(
            forms::actions()
                .child(
                    forms::secondary("new-migration", "New migration")
                        .on_click(cx.listener(|this, _, window, cx| this.new_migration(window, cx))),
                )
                .child(
                    forms::secondary("dry-run", "Dry run pending")
                        .disabled(busy || postgres || pending == 0)
                        .on_click(cx.listener(|this, _, _, cx| this.dry_run(cx))),
                )
                .child(
                    forms::primary("apply-migrations", &format!("Apply pending ({pending})"))
                        .disabled(busy || postgres || pending == 0 || preflight_failed)
                        .on_click(cx.listener(|this, _, _, cx| this.run_and_check(false, cx))),
                )
                .child(
                    forms::danger("rollback-migration", "Roll back last")
                        .disabled(busy || postgres || !any_applied)
                        .on_click(cx.listener(|this, _, _, cx| this.run_and_check(true, cx))),
                ),
        );
        if let Some(e) = &self.error {
            tab = tab.child(widgets::error_banner("migrations-error", e, None, cx));
        }
        for (n, issue) in self.issues.iter().enumerate() {
            tab = tab.child(issue_line(eid(format!("migration-issue:{n}")), issue, cx));
        }
        let mut table = v_flex().gap_1();
        for m in &declared {
            let s = self.status_of(&m.id);
            let state = if s.is_some_and(|s| s.modified) {
                "Changed after apply"
            } else if s.is_some_and(|s| s.applied) {
                "Applied"
            } else if postgres {
                "Disabled"
            } else if s.is_some_and(|s| !s.applies_to_store) {
                "Not supported"
            } else {
                "Pending"
            };
            let (migration, name) = (m.clone(), m.name.clone());
            table = table.child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(widgets::text(eid(format!("migration-order:{name}")), m.order.to_string()))
                    .child(widgets::text(eid(format!("migration-name:{name}")), name.clone()))
                    .child(widgets::text(
                        eid(format!("migration-target:{name}")),
                        target_label(&m.target_store),
                    ))
                    .child(widgets::text(eid(format!("migration-status:{name}")), state))
                    .child(
                        forms::secondary(eid(format!("edit-migration:{name}")), &format!("Edit {name}"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_editor(migration.clone(), false, window, cx)
                            })),
                    ),
            );
        }
        tab = tab.child(table);
        if let Some(editor) = &self.editing {
            tab = tab.child(editor.clone());
        }
        if let Some(dry) = &self.dry {
            let ok = dry.status == "dry_run_ok";
            tab = tab.child(
                forms::card("dry-run-result", "Dry run", cx)
                    .child(widgets::text(
                        "dry-run-status",
                        if ok {
                            "Dry run succeeded; nothing was changed."
                        } else {
                            "Dry run failed; nothing was changed."
                        },
                    ))
                    .child(log_entry("dry", 0, dry)),
            );
        }
        if let Some(run) = &self.run {
            let mut status = if run.ok {
                "All migrations succeeded.".to_string()
            } else {
                "A migration failed; later ones did not run.".to_string()
            };
            if let Some(cp) = &run.checkpoint {
                status.push_str(&format!(" Checkpoint {} was saved first.", cp.id));
            }
            let mut card = forms::card("last-run", "Last run", cx).child(widgets::text("run-status", status));
            for (i, log) in run.logs.iter().enumerate() {
                card = card.child(log_entry("run", i, log));
            }
            tab = tab.child(card);
        }
        if let Some(after) = self.after.as_ref().filter(|a| !a.is_empty()) {
            let mut card = forms::card("after-problems", "Problems after migration", cx).child(forms::muted(
                "The schema changed. These definitions need attention (also listed in Problems).",
                cx,
            ));
            for (n, issue) in after.iter().enumerate() {
                card = card.child(issue_line(eid(format!("after-problem:{n}")), issue, cx));
            }
            tab = tab.child(card);
        }
        let mut log = forms::card("migration-log", "Migration log", cx);
        if self.history.is_empty() {
            log = log.child(widgets::text("log-empty", "No migrations have run on this store."));
        }
        for (i, entry) in self.history.iter().enumerate() {
            log = log.child(log_entry("history", i, entry));
        }
        tab.child(log)
    }
}
