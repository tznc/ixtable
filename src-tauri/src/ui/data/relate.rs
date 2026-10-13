//! Creates a relationship from the diagram (port of `src/data/RelateDialog.tsx`): the
//! relationship editor opens prefilled with the drawn columns so the user picks more
//! columns and the referential actions, then the change is previewed and applied like
//! any other schema change.
//!
//! Test ids: `Referencing table` (select), the relationship editor's ids (its add
//! button is `Preview relationship`), `relate-error`, `relate-cancel`; the preview is an
//! `ImpactDialog` confirming with `Create relationship`.

use super::relationships::DrawnRelationship;
use crate::data::{AlterTable, CreateForeignKey, TableSchema};
use crate::manager::AppError;
use crate::recordstore::{
    capabilities::StoreCapabilities,
    commands::{apply_table_changes, preview_table_changes},
    ChangePlan,
};
use crate::ui::{
    prelude::*,
    schema::{
        constraints::{ForeignKeyDraft, ForeignKeyEditor, ForeignKeyEvent, TableRef},
        controls::{error_line, modal, select},
        impact::{ImpactDialog, ImpactEvent},
    },
    shell::Shell,
};

pub enum RelateEvent {
    /// The relationship was created on this (child) table.
    Applied(String),
    Cancel,
}

struct Preview {
    foreign_key: CreateForeignKey,
    dialog: Entity<ImpactDialog>,
    _subscription: Subscription,
}

pub struct RelateDialog {
    shell: Entity<Shell>,
    schemas: Vec<TableSchema>,
    capabilities: Option<StoreCapabilities>,
    drawn: Option<DrawnRelationship>,
    child: String,
    editor: Entity<ForeignKeyEditor>,
    editor_subscription: Subscription,
    preview: Option<Preview>,
    /// The last previewed relationship, restored when the preview is cancelled.
    draft: Option<CreateForeignKey>,
    error: String,
    busy: bool,
}
impl EventEmitter<RelateEvent> for RelateDialog {}

fn draft_of(fk: &CreateForeignKey) -> ForeignKeyDraft {
    ForeignKeyDraft {
        columns: fk.columns.clone(),
        target_table: fk.target_table.clone(),
        target_columns: fk.target_columns.clone(),
        on_update: fk.on_update.clone(),
        on_delete: fk.on_delete.clone(),
    }
}

impl RelateDialog {
    pub fn new(
        shell: Entity<Shell>,
        schemas: Vec<TableSchema>,
        capabilities: Option<StoreCapabilities>,
        drawn: Option<DrawnRelationship>,
        initial_child: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let child = drawn.as_ref().map_or(initial_child, |d| d.child_table.clone());
        let initial = drawn.as_ref().filter(|d| d.child_table == child).map(|d| ForeignKeyDraft {
            columns: vec![d.child_column.clone()],
            target_table: d.parent_table.clone(),
            target_columns: vec![d.parent_column.clone()],
            on_update: None,
            on_delete: None,
        });
        let (editor, editor_subscription) = Self::build_editor(&schemas, &capabilities, &child, initial, cx);
        Self {
            shell,
            schemas,
            capabilities,
            drawn,
            child,
            editor,
            editor_subscription,
            preview: None,
            draft: None,
            error: String::new(),
            busy: false,
        }
    }

    fn build_editor(
        schemas: &[TableSchema],
        capabilities: &Option<StoreCapabilities>,
        child: &str,
        initial: Option<ForeignKeyDraft>,
        cx: &mut Context<Self>,
    ) -> (Entity<ForeignKeyEditor>, Subscription) {
        let columns: Vec<String> = schemas
            .iter()
            .find(|s| s.name == child)
            .map(|s| s.columns.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default();
        let tables: Vec<TableRef> = schemas.iter().map(TableRef::of).collect();
        let actions = capabilities.as_ref().map(|c| c.foreign_key_actions.clone());
        let editor = cx.new(|cx| ForeignKeyEditor::new(columns, tables, actions, initial, "Preview relationship", cx));
        let subscription = cx.subscribe(&editor, |this, _, e: &ForeignKeyEvent, cx| {
            let ForeignKeyEvent::Add(fk) = e;
            this.preview_relationship(fk.clone(), cx);
        });
        (editor, subscription)
    }

    fn rebuild_editor(&mut self, initial: Option<ForeignKeyDraft>, cx: &mut Context<Self>) {
        let (editor, subscription) = Self::build_editor(&self.schemas, &self.capabilities, &self.child, initial, cx);
        self.editor = editor;
        self.editor_subscription = subscription;
        cx.notify();
    }

    fn change_child(&mut self, child: &str, cx: &mut Context<Self>) {
        self.child = child.to_string();
        self.draft = None;
        let initial = self
            .drawn
            .as_ref()
            .filter(|d| d.child_table == child)
            .map(|d| ForeignKeyDraft {
                columns: vec![d.child_column.clone()],
                target_table: d.parent_table.clone(),
                target_columns: vec![d.parent_column.clone()],
                on_update: None,
                on_delete: None,
            });
        self.rebuild_editor(initial, cx);
    }

    fn operations(foreign_key: &CreateForeignKey) -> Vec<AlterTable> {
        vec![AlterTable::AddForeignKey {
            foreign_key: foreign_key.clone(),
        }]
    }

    fn preview_relationship(&mut self, foreign_key: CreateForeignKey, cx: &mut Context<Self>) {
        self.draft = Some(foreign_key.clone());
        self.busy = true;
        self.error.clear();
        cx.notify();
        let label = self.shell.read(cx).label().to_string();
        let (child, ops) = (self.child.clone(), Self::operations(&foreign_key));
        task::spawn_call(
            cx,
            move || preview_table_changes(label, child, ops),
            move |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(plan) => this.open_preview(foreign_key, plan, cx),
                    Err(e) => {
                        this.error = e.message;
                        // The editor cleared itself when it emitted: bring the draft back.
                        let draft = this.draft.as_ref().map(draft_of);
                        this.rebuild_editor(draft, cx);
                    }
                }
                cx.notify();
            },
        );
    }

    fn open_preview(&mut self, foreign_key: CreateForeignKey, plan: ChangePlan, cx: &mut Context<Self>) {
        let title = format!(
            "Relate {} ({}) to {}?",
            self.child,
            foreign_key.columns.join(", "),
            foreign_key.target_table
        );
        let dialog = cx.new(|_| ImpactDialog::new(title, Some(plan), None, "Create relationship", None));
        let subscription = cx.subscribe(&dialog, |this, _, e: &ImpactEvent, cx| match e {
            ImpactEvent::Cancel => {
                this.error.clear();
                this.preview = None;
                let draft = this.draft.as_ref().map(draft_of);
                this.rebuild_editor(draft, cx);
            }
            ImpactEvent::Confirm => this.apply(cx),
        });
        self.preview = Some(Preview {
            foreign_key,
            dialog,
            _subscription: subscription,
        });
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        let Some(preview) = &self.preview else {
            return;
        };
        preview.dialog.update(cx, |d, cx| d.set_state(true, String::new(), cx));
        let label = self.shell.read(cx).label().to_string();
        let (child, ops) = (self.child.clone(), Self::operations(&preview.foreign_key));
        let table = child.clone();
        task::spawn_call(
            cx,
            move || apply_table_changes(label, child, ops),
            move |this, result, cx| match result {
                Ok(state) => {
                    this.shell.update(cx, |s, cx| s.apply_session(&state, cx));
                    cx.emit(RelateEvent::Applied(table));
                }
                Err(e) => this.fail(e, cx),
            },
        );
    }

    fn fail(&mut self, error: AppError, cx: &mut Context<Self>) {
        if let Some(preview) = &self.preview {
            let message = error.message.clone();
            preview.dialog.update(cx, |d, cx| d.set_state(false, message, cx));
        }
        self.error = error.message;
        cx.notify();
    }
}

impl Render for RelateDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let options: Vec<(String, String)> = self
            .schemas
            .iter()
            .map(|s| (s.name.clone(), s.name.clone()))
            .collect();
        let body = v_flex()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_sm().text_color(cx.theme().muted_foreground).child("Referencing table"))
                    .child(select("Referencing table", &self.child, options, "Choose a table", {
                        move |value, _, cx| {
                            let value = value.to_string();
                            entity.update(cx, |this, cx| this.change_child(&value, cx));
                        }
                    })),
            )
            .child(self.editor.clone())
            .when(!self.error.is_empty() && self.preview.is_none(), |el| {
                el.child(error_line("relate-error", &self.error, cx))
            })
            .child(
                h_flex().justify_end().child(
                    Button::new("relate-cancel")
                        .outline()
                        .label("Cancel")
                        .disabled(self.busy)
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(RelateEvent::Cancel))),
                ),
            );
        div()
            .absolute()
            .inset_0()
            .child(modal("relate-dialog", "New relationship", body, cx))
            .when_some(self.preview.as_ref().map(|p| p.dialog.clone()), |el, dialog| el.child(dialog))
    }
}
