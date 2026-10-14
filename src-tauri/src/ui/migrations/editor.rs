//! Edits one migration. Applied migrations are immutable: only their name can change.
//! Port of src/migrations/MigrationEditor.tsx.
//!
//! Test ids: card `migration-editor` (title "New migration" or "Edit NAME"),
//! `migration-id-note`, inputs `migration-name`, `migration-order`, `migration-up`,
//! `migration-down`, `migration-target` (text) and `migration-target-sqlite` (switches a
//! legacy PostgreSQL target), checkboxes `migration-dep:<id>` and `migration-reversible`,
//! buttons `migration-close`, `migration-preview-up`, `migration-preview-down`,
//! `migration-delete`, `migration-save`, preview `sql-preview` ("SQL preview (down)"),
//! `preview-summary`, `preview-statements`, `preview-warning:<i>`, `migration-editor-error`.

use crate::{
    manager::AppError,
    migrations::{Migration, commands::MigrationPreview},
    ui::{
        forms::{self, eid},
        prelude::*,
        shell::Shell,
    },
};
use gpui_kit::component::{
    Disableable as _,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
};

pub enum EditorEvent {
    /// Saved, deleted or dismissed: the host closes this editor.
    Close,
}

pub struct MigrationEditor {
    shell: Entity<Shell>,
    original: Migration,
    is_new: bool,
    applied: bool,
    others: Vec<(String, String)>,
    target: String,
    depends: Vec<String>,
    reversible: bool,
    name: Entity<InputState>,
    order: Entity<InputState>,
    up: Entity<TextareaState>,
    down: Entity<TextareaState>,
    preview: Option<MigrationPreview>,
    error: Option<AppError>,
    _subscriptions: Vec<Subscription>,
}
impl EventEmitter<EditorEvent> for MigrationEditor {}

impl MigrationEditor {
    /// `others`: (id, name) of the other migrations, for the dependency list.
    pub fn new(
        shell: Entity<Shell>,
        migration: Migration,
        is_new: bool,
        applied: bool,
        others: Vec<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = forms::line_state(window, cx, "Name", &migration.name);
        let order = forms::line_state(window, cx, "Order", &migration.order.to_string());
        let up = forms::area_state(window, cx, &migration.up);
        let down = forms::area_state(window, cx, migration.down.as_deref().unwrap_or(""));
        // Typing changes what Save and the down preview depend on.
        let subscriptions = vec![
            cx.subscribe(&name, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.subscribe(&down, |_, _, _: &InputEvent, cx| cx.notify()),
        ];
        Self {
            shell,
            target: if migration.target_store == "postgres" { "postgres" } else { "sqlite" }
                .to_string(),
            depends: migration.depends_on.clone(),
            reversible: migration.reversible,
            original: migration,
            is_new,
            applied,
            others,
            name,
            order,
            up,
            down,
            preview: None,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    /// The migration as currently edited.
    fn draft(&self, cx: &App) -> Migration {
        let down = forms::area_text(&self.down, cx);
        Migration {
            id: self.original.id.clone(),
            name: forms::text_of(&self.name, cx),
            order: forms::text_of(&self.order, cx).trim().parse().unwrap_or(0),
            target_store: self.target.clone(),
            up: forms::area_text(&self.up, cx),
            down: (!down.is_empty()).then_some(down),
            reversible: self.reversible,
            depends_on: self.depends.clone(),
        }
    }

    fn label(&self, cx: &App) -> String {
        self.shell.read(cx).label().to_string()
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        let next = self.draft(cx);
        let store = self.shell.read(cx).store.clone();
        let label = format!("Save migration {}", next.name);
        let result = store.update(cx, |s, cx| {
            s.update(
                &label,
                |config| match config.migrations.iter_mut().find(|m| m.id == next.id) {
                    Some(slot) => *slot = next.clone(),
                    None => config.migrations.push(next.clone()),
                },
                cx,
            )
        });
        self.finish(result.map(|_| ()), cx);
    }

    fn delete(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        let id = self.original.id.clone();
        let store = self.shell.read(cx).store.clone();
        let result = store.update(cx, |s, cx| {
            s.update("Delete migration", |config| config.migrations.retain(|m| m.id != id), cx)
        });
        self.finish(result.map(|_| ()), cx);
    }

    fn finish(&mut self, result: Result<(), AppError>, cx: &mut Context<Self>) {
        match result {
            Ok(()) => cx.emit(EditorEvent::Close),
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    fn show_preview(&mut self, down: bool, cx: &mut Context<Self>) {
        self.error = None;
        let (label, id) = (self.label(cx), self.original.id.clone());
        task::spawn_call(
            cx,
            move || {
                crate::migrations::commands::preview_migration(
                    label,
                    id,
                    down.then(|| "down".to_string()),
                )
            },
            |this, result, cx| {
                match result {
                    Ok(preview) => this.preview = Some(preview),
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
    }
}

impl Render for MigrationEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let applied = self.applied;
        let title = if self.is_new {
            "New migration".to_string()
        } else {
            format!("Edit {}", self.original.name)
        };
        let name_blank = forms::text_of(&self.name, cx).trim().is_empty();
        let down_text = forms::area_text(&self.down, cx);
        let can_preview_down = self.reversible || !down_text.trim().is_empty();
        let mut editor = forms::card("migration-editor", &title, cx).child(widgets::text(
            "migration-id-note",
            format!(
                "Id {} never changes.{}",
                self.original.id,
                if applied {
                    " This migration is applied, so its SQL is locked; add a new migration instead."
                } else {
                    ""
                }
            ),
        ));
        editor = editor
            .child(forms::input_field("migration-name", "Name", &self.name, cx))
            .child(widgets::field(
                "Order",
                Input::new(&self.order)
                    .id("migration-order")
                    .aria_label("Order")
                    .disabled(applied),
                cx,
            ));
        // Migrations run on the embedded SQLite store; a legacy PostgreSQL target stays
        // visible only so it can be switched to SQLite.
        let legacy = self.target == "postgres";
        let mut target = forms::actions().child(widgets::text(
            "migration-target",
            if legacy { "PostgreSQL (not supported)" } else { "Embedded SQLite" },
        ));
        if legacy {
            target = target.child(
                forms::secondary("migration-target-sqlite", "Use embedded SQLite")
                    .disabled(applied)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.target = "sqlite".into();
                        cx.notify();
                    })),
            );
        }
        editor = editor.child(widgets::field("Target store", target, cx));
        if !self.others.is_empty() {
            let mut deps = v_flex().gap_1();
            for (id, name) in &self.others {
                let (entity, dep) = (cx.entity(), id.clone());
                deps = deps.child(
                    forms::check(
                        eid(format!("migration-dep:{id}")),
                        name,
                        self.depends.contains(id),
                        move |on, _, cx| {
                            entity.update(cx, |this, cx| {
                                this.depends.retain(|d| d != &dep);
                                if on {
                                    this.depends.push(dep.clone());
                                }
                                cx.notify();
                            })
                        },
                    )
                    .disabled(applied),
                );
            }
            editor = editor.child(widgets::field("Depends on", deps, cx));
        }
        let entity = cx.entity();
        editor = editor
            .child(
                forms::check(
                    "migration-reversible",
                    "Reversible (requires down SQL)",
                    self.reversible,
                    move |on, _, cx| {
                        entity.update(cx, |this, cx| {
                            this.reversible = on;
                            cx.notify();
                        })
                    },
                )
                .disabled(applied),
            )
            .child(sql_field("migration-up", "Up SQL", &self.up, applied, cx))
            .child(sql_field(
                "migration-down",
                "Down SQL",
                &self.down,
                applied && self.original.down.is_some(),
                cx,
            ));
        if let Some(e) = &self.error {
            editor = editor.child(widgets::error_banner("migration-editor-error", e, None, cx));
        }
        if let Some(preview) = &self.preview {
            let count = preview.statements.len();
            let mut block = v_flex()
                .gap_1()
                .child(widgets::text("sql-preview", format!("SQL preview ({})", preview.direction)))
                .child(widgets::styled_text(
                    "preview-summary",
                    format!(
                        "{count} statement{} on {}, in {}",
                        if count == 1 { "" } else { "s" },
                        preview.store,
                        if preview.transactional { "one transaction" } else { "autocommit" }
                    ),
                    |d| d.font_weight(FontWeight::SEMIBOLD),
                ))
                .child(widgets::text("preview-statements", preview.statements.join("\n")));
            for (i, warning) in preview.warnings.iter().enumerate() {
                block = block.child(widgets::text(eid(format!("preview-warning:{i}")), warning.clone()));
            }
            editor = editor.child(block);
        }
        let mut actions = forms::actions().child(
            forms::secondary("migration-close", "Close")
                .on_click(cx.listener(|_, _, _, cx| cx.emit(EditorEvent::Close))),
        );
        if !self.is_new {
            actions = actions.child(
                forms::secondary("migration-preview-up", "Preview SQL")
                    .on_click(cx.listener(|this, _, _, cx| this.show_preview(false, cx))),
            );
            if can_preview_down {
                actions = actions.child(
                    forms::secondary("migration-preview-down", "Preview down SQL")
                        .on_click(cx.listener(|this, _, _, cx| this.show_preview(true, cx))),
                );
            }
            if !applied {
                actions = actions.child(
                    forms::danger("migration-delete", "Delete migration")
                        .on_click(cx.listener(|this, _, _, cx| this.delete(cx))),
                );
            }
        }
        actions = actions.child(
            forms::primary("migration-save", "Save migration")
                .disabled(name_blank)
                .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
        );
        editor.child(actions)
    }
}

/// A SQL text area, read-only (disabled) once the migration is applied.
fn sql_field(
    id: &'static str,
    label: &str,
    state: &Entity<TextareaState>,
    locked: bool,
    cx: &App,
) -> Div {
    widgets::field(
        label,
        div()
            .id(id)
            .aria_label(label.to_string())
            .child(
                Textarea::new(state)
                    .h(px(120.))
                    .aria_label(label.to_string())
                    .disabled(locked),
            )
            .test_support(),
        cx,
    )
}
