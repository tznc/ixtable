//! Table designer (port of `src/data/TableSchemaDesigner.tsx`): changes are staged,
//! previewed by the record store (in place vs table rebuild) and applied in one
//! transaction. Destructive changes and rebuilds go through an impact preview.
//!
//! Test ids: `design-heading`, `designer-store`, `drop-table`, `close-designer`,
//! `designer-error`; `Table name` (input) and `rename-table`; per column `Column <name>:`
//! `name|type|...` (see `ColumnFields`), `stage-column:<name>` and `drop-column:<name>`;
//! the new column row `New column:name|type|...` and `Add column`; `set-primary-key`
//! (the picker's boxes are `Primary key columns: <column>`); `remove-fk:<columns>`,
//! `remove-unique:<columns>`, `remove-check:<expression>`, `drop-index:<name>`; the
//! sections are scopes named `Table`, `Columns`, `Primary key`, `Relationships`,
//! `Constraints`, `Indexes` and `Pending changes`, which holds `pending-op:<i>`,
//! `pending-mode:<i>`, `pending-destructive:<i>`, `remove-pending:<i>`, `pending-empty`,
//! `plan-error`, `discard-changes` and `apply-changes`.

use crate::data::{AlterTable, Column, CreateColumn, TableSchema};
use crate::manager::AppError;
use crate::recordstore::{
    capabilities::StoreCapabilities,
    commands::{
        apply_table_changes, create_index, drop_database_table, drop_index, preview_table_changes,
        table_drop_impact,
    },
    ChangeMode, ChangePlan, CreateIndex, PlannedOperation,
};
use crate::ui::{
    fields::settings_editor::FieldSettingsEditor,
    prelude::*,
    schema::{
        column_fields::ColumnFields,
        columns::{draft_from_column, empty_column, spec_from_draft, ColumnDraft},
        constraints::{
            CheckEditor, CheckEvent, ColumnPicker, ForeignKeyEditor, ForeignKeyEvent, IndexEditor,
            IndexEvent, TableRef, UniqueEditor, UniqueEvent,
        },
        controls::{error_line, input_text, new_input, note, section, text_input},
        impact::{change_mode_key, ImpactDialog, ImpactEvent},
        logical::{mode_label, store_label, store_types},
    },
    shell::Shell,
};
use gpui_kit::component::input::InputState;

const RENAME_ACK: &str =
    "I understand the definitions listed above still use the old name and must be updated.";

pub enum DesignerEvent {
    /// The table changed (or an index did); carries the table's name now.
    Changed(String),
    Dropped,
    Cancel,
}

enum DialogKind {
    Apply,
    Drop,
    DropIndex(String),
}

struct Dialog {
    kind: DialogKind,
    view: Entity<ImpactDialog>,
    _subscription: Subscription,
}

struct ColumnRow {
    name: String,
    before: ColumnDraft,
    fields: Entity<ColumnFields>,
}

pub struct TableSchemaDesigner {
    shell: Entity<Shell>,
    schema: TableSchema,
    capabilities: Option<StoreCapabilities>,
    rows: Vec<ColumnRow>,
    new_column: Entity<ColumnFields>,
    table_name: Entity<InputState>,
    primary_key: Entity<ColumnPicker>,
    fk_editor: Entity<ForeignKeyEditor>,
    unique_editor: Entity<UniqueEditor>,
    check_editor: Entity<CheckEditor>,
    index_editor: Entity<IndexEditor>,
    field_settings: Entity<FieldSettingsEditor>,
    pending: Vec<AlterTable>,
    plan: Option<ChangePlan>,
    plan_error: String,
    plan_generation: u64,
    error: String,
    busy: bool,
    // Set by the workbench while the schemas reload; edits are held until they are fresh.
    locked: bool,
    dialog: Option<Dialog>,
    _subscriptions: Vec<Subscription>,
}
impl EventEmitter<DesignerEvent> for TableSchemaDesigner {}

/// Impact preview for dropping an index: in place, no rows removed, the exact statement.
fn drop_index_plan(table: &str, name: &str) -> ChangePlan {
    ChangePlan {
        table: table.to_string(),
        operations: vec![PlannedOperation {
            summary: format!("Drop index {name}"),
            mode: ChangeMode::InPlace,
            destructive: false,
            reason: None,
        }],
        rebuild: false,
        destructive: false,
        statements: vec![format!("DROP INDEX \"{}\"", name.replace('"', "\"\""))],
        warnings: vec!["Queries and lookups that relied on this index may run slower.".into()],
        impact: None,
    }
}

fn operation_name(op: &AlterTable) -> String {
    serde_json::to_value(op)
        .ok()
        .and_then(|v| v.get("operation").and_then(|o| o.as_str().map(|s| s.replace('_', " "))))
        .unwrap_or_default()
}

impl TableSchemaDesigner {
    pub fn new(
        shell: Entity<Shell>,
        schema: TableSchema,
        tables: Vec<TableSchema>,
        capabilities: Option<StoreCapabilities>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let columns: Vec<String> = schema.columns.iter().map(|c| c.name.clone()).collect();
        let refs: Vec<TableRef> = tables.iter().map(TableRef::of).collect();
        let actions = capabilities.as_ref().map(|c| c.foreign_key_actions.clone());
        let rows: Vec<ColumnRow> = schema
            .columns
            .iter()
            .map(|c| {
                let draft = draft_from_column(c, &schema);
                let label = format!("Column {}", c.name);
                let fields = cx.new(|cx| ColumnFields::new(&label, draft.clone(), true, window, cx));
                ColumnRow {
                    name: c.name.clone(),
                    before: draft,
                    fields,
                }
            })
            .collect();
        let new_column = cx.new(|cx| ColumnFields::new("New column", empty_column(""), true, window, cx));
        let primary: Vec<String> = if schema.primary_key.is_empty() {
            schema
                .columns
                .iter()
                .filter(|c| c.primary_key_position > 0)
                .map(|c| c.name.clone())
                .collect()
        } else {
            schema.primary_key.clone()
        };
        let primary_key = {
            let columns = columns.clone();
            cx.new(|_| ColumnPicker::new("Primary key columns", columns, primary))
        };
        let fk_editor = {
            let (columns, refs) = (columns.clone(), refs.clone());
            cx.new(|cx| ForeignKeyEditor::new(columns, refs, actions, None, "Add relationship", cx))
        };
        let unique_editor = {
            let columns = columns.clone();
            cx.new(|cx| UniqueEditor::new(columns, cx))
        };
        let check_editor = cx.new(|cx| CheckEditor::new(window, cx));
        let index_editor = {
            let (name, columns) = (schema.name.clone(), columns.clone());
            cx.new(|cx| IndexEditor::new(&name, columns, window, cx))
        };
        let field_settings = {
            let (shell, name, columns) = (shell.clone(), schema.name.clone(), schema.columns.clone());
            cx.new(|cx| FieldSettingsEditor::new(shell, &name, columns, window, cx))
        };
        let table_name = new_input(window, cx, "", &schema.name);
        let subscriptions = vec![
            cx.subscribe(&fk_editor, |this, _, e: &ForeignKeyEvent, cx| {
                let ForeignKeyEvent::Add(fk) = e;
                this.stage(vec![AlterTable::AddForeignKey { foreign_key: fk.clone() }], cx);
            }),
            cx.subscribe(&unique_editor, |this, _, e: &UniqueEvent, cx| {
                let UniqueEvent::Add(columns) = e;
                this.stage(vec![AlterTable::AddUnique { columns: columns.clone(), name: None }], cx);
            }),
            cx.subscribe(&check_editor, |this, _, e: &CheckEvent, cx| {
                let CheckEvent::Add(expression) = e;
                this.stage(vec![AlterTable::AddCheck { expression: expression.clone(), name: None }], cx);
            }),
            cx.subscribe(&index_editor, |this, _, e: &IndexEvent, cx| {
                let IndexEvent::Add(index) = e;
                this.add_index(index.name.clone(), index.columns.clone(), index.unique, cx);
            }),
            cx.subscribe(&primary_key, |_, _, _, cx| cx.notify()),
            on_change_notify(cx, &table_name),
        ];
        let mut this = Self {
            shell,
            schema,
            capabilities: None,
            rows,
            new_column,
            table_name,
            primary_key,
            fk_editor,
            unique_editor,
            check_editor,
            index_editor,
            field_settings,
            pending: vec![],
            plan: None,
            plan_error: String::new(),
            plan_generation: 0,
            error: String::new(),
            busy: false,
            locked: false,
            dialog: None,
            _subscriptions: subscriptions,
        };
        this.set_capabilities(capabilities, cx);
        this
    }

    pub fn schema(&self) -> &TableSchema {
        &self.schema
    }

    /// The store's types, its precision limit and referential actions.
    pub fn set_capabilities(&mut self, capabilities: Option<StoreCapabilities>, cx: &mut Context<Self>) {
        let types = store_types(capabilities.as_ref());
        let max_precision = capabilities
            .as_ref()
            .and_then(|c| {
                c.logical_types
                    .iter()
                    .find(|t| t.logical_type.starts_with("decimal"))
                    .and_then(|t| t.max_precision)
            })
            .map_or(38, u32::from);
        for row in &self.rows {
            row.fields.update(cx, |f, cx| f.set_store(types.clone(), max_precision, cx));
        }
        self.new_column
            .update(cx, |f, cx| f.set_store(types.clone(), max_precision, cx));
        if let Some(c) = &capabilities {
            let actions = c.foreign_key_actions.clone();
            self.fk_editor.update(cx, |e, cx| e.set_actions(actions, cx));
        }
        self.capabilities = capabilities;
        cx.notify();
    }

    pub fn set_tables(&mut self, tables: &[TableSchema], cx: &mut Context<Self>) {
        let refs: Vec<TableRef> = tables.iter().map(TableRef::of).collect();
        self.fk_editor.update(cx, |e, cx| e.set_tables(refs, cx));
    }

    pub fn pending(&self) -> &[AlterTable] {
        &self.pending
    }

    pub fn plan(&self) -> Option<&ChangePlan> {
        self.plan.as_ref()
    }

    // ---- Staging -----------------------------------------------------------------

    fn stage(&mut self, ops: Vec<AlterTable>, cx: &mut Context<Self>) {
        self.pending.extend(ops);
        self.refresh_plan(cx);
        cx.notify();
    }

    fn refresh_plan(&mut self, cx: &mut Context<Self>) {
        self.plan_generation += 1;
        if self.pending.is_empty() {
            self.plan = None;
            self.plan_error.clear();
            return;
        }
        let generation = self.plan_generation;
        let label = self.shell.read(cx).label().to_string();
        let (table, ops) = (self.schema.name.clone(), self.pending.clone());
        task::spawn_call(
            cx,
            move || preview_table_changes(label, table, ops),
            move |this, result, cx| {
                if this.plan_generation != generation {
                    return;
                }
                match result {
                    Ok(plan) => {
                        this.plan = Some(plan);
                        this.plan_error.clear();
                    }
                    Err(e) => {
                        this.plan = None;
                        this.plan_error = e.message;
                    }
                }
                cx.notify();
            },
        );
    }

    fn stage_column(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(row) = self.rows.iter().find(|r| r.name == name) else {
            return;
        };
        let before = row.before.clone();
        let after = row.fields.read(cx).draft().clone();
        let target = if after.name.trim().is_empty() {
            name.to_string()
        } else {
            after.name.trim().to_string()
        };
        let mut ops = Vec::new();
        if target != name {
            ops.push(AlterTable::RenameColumn {
                column: name.to_string(),
                new_name: target.clone(),
            });
        }
        let check_changed = after.check.trim() != before.check.trim();
        let changed = after.logical_type != before.logical_type
            || after.required != before.required
            || after.default_expression != before.default_expression
            || after.unique != before.unique
            || check_changed;
        if changed {
            let mut definition: CreateColumn = spec_from_draft(&ColumnDraft { name: target.clone(), ..after.clone() }, 0);
            // An untouched check is sent as none (keep, and follow a rename); empty removes it.
            definition.check = check_changed.then(|| after.check.trim().to_string());
            ops.push(AlterTable::AlterColumn {
                column: target,
                definition,
            });
        }
        if !ops.is_empty() {
            self.stage(ops, cx);
        }
    }

    fn add_column(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.new_column.read(cx).draft().clone();
        if draft.name.trim().is_empty() {
            return;
        }
        self.stage(vec![AlterTable::AddColumn { column: spec_from_draft(&draft, 0) }], cx);
        self.new_column.update(cx, |f, cx| f.set_draft(empty_column(""), window, cx));
    }

    fn final_name(&self) -> String {
        self.pending
            .iter()
            .rev()
            .find_map(|op| match op {
                AlterTable::RenameTable { new_name } => Some(new_name.clone()),
                _ => None,
            })
            .unwrap_or_else(|| self.schema.name.clone())
    }

    // ---- Running -----------------------------------------------------------------

    fn begin(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.error.clear();
        cx.notify();
    }

    fn fail(&mut self, error: AppError, cx: &mut Context<Self>) {
        self.busy = false;
        self.error = error.message.clone();
        if let Some(dialog) = &self.dialog {
            let message = error.message;
            dialog.view.update(cx, |d, cx| d.set_state(false, message, cx));
        }
        cx.notify();
    }

    fn adopt(&mut self, state: &crate::manager::SessionState, cx: &mut Context<Self>) {
        self.shell.update(cx, |s, cx| s.apply_session(state, cx));
    }

    fn open_dialog(&mut self, kind: DialogKind, view: ImpactDialog, cx: &mut Context<Self>) {
        let view = cx.new(|_| view);
        let subscription = cx.subscribe(&view, |this, _, e: &ImpactEvent, cx| match e {
            ImpactEvent::Cancel => {
                this.dialog = None;
                this.busy = false;
                this.error.clear();
                cx.notify();
            }
            ImpactEvent::Confirm => this.confirm(cx),
        });
        self.dialog = Some(Dialog {
            kind,
            view,
            _subscription: subscription,
        });
        self.busy = false;
        cx.notify();
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        self.begin(cx);
        let label = self.shell.read(cx).label().to_string();
        let (table, ops) = (self.schema.name.clone(), self.pending.clone());
        task::spawn_call(
            cx,
            move || preview_table_changes(label, table, ops),
            |this, result, cx| match result {
                Ok(fresh) => {
                    // `impact` is also set when a rename leaves definitions using the old name.
                    if fresh.destructive || fresh.rebuild || fresh.impact.is_some() {
                        let count = fresh.operations.len();
                        let ack = fresh
                            .impact
                            .as_ref()
                            .is_some_and(|i| !i.dependents.is_empty())
                            .then(|| RENAME_ACK.to_string());
                        let title = format!(
                            "Apply {count} change{} to {}?",
                            if count == 1 { "" } else { "s" },
                            this.schema.name
                        );
                        this.open_dialog(
                            DialogKind::Apply,
                            ImpactDialog::new(title, Some(fresh), None, "Apply changes", ack),
                            cx,
                        );
                    } else {
                        this.run_apply(cx);
                    }
                }
                Err(e) => this.fail(e, cx),
            },
        );
    }

    fn run_apply(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        let label = self.shell.read(cx).label().to_string();
        let (table, ops) = (self.schema.name.clone(), self.pending.clone());
        let name = self.final_name();
        task::spawn_call(
            cx,
            move || apply_table_changes(label, table, ops),
            move |this, result, cx| match result {
                Ok(state) => {
                    this.dialog = None;
                    this.busy = false;
                    this.adopt(&state, cx);
                    cx.emit(DesignerEvent::Changed(name));
                    cx.notify();
                }
                Err(e) => this.fail(e, cx),
            },
        );
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = &self.dialog else {
            return;
        };
        dialog.view.update(cx, |d, cx| d.set_state(true, String::new(), cx));
        match &dialog.kind {
            DialogKind::Apply => self.run_apply(cx),
            DialogKind::Drop => {
                let label = self.shell.read(cx).label().to_string();
                let table = self.schema.name.clone();
                task::spawn_call(
                    cx,
                    move || drop_database_table(label, table),
                    |this, result, cx| match result {
                        Ok(state) => {
                            this.dialog = None;
                            this.adopt(&state, cx);
                            cx.emit(DesignerEvent::Dropped);
                            cx.notify();
                        }
                        Err(e) => this.fail(e, cx),
                    },
                );
            }
            DialogKind::DropIndex(name) => {
                let label = self.shell.read(cx).label().to_string();
                let (name, table) = (name.clone(), self.schema.name.clone());
                task::spawn_call(
                    cx,
                    move || drop_index(label, name),
                    move |this, result, cx| match result {
                        Ok(state) => {
                            this.dialog = None;
                            this.adopt(&state, cx);
                            cx.emit(DesignerEvent::Changed(table));
                            cx.notify();
                        }
                        Err(e) => this.fail(e, cx),
                    },
                );
            }
        }
    }

    fn ask_drop_table(&mut self, cx: &mut Context<Self>) {
        self.begin(cx);
        let label = self.shell.read(cx).label().to_string();
        let table = self.schema.name.clone();
        task::spawn_call(
            cx,
            move || table_drop_impact(label, table),
            |this, result, cx| match result {
                Ok(impact) => {
                    let title = format!("Drop table {}?", this.schema.name);
                    this.open_dialog(
                        DialogKind::Drop,
                        ImpactDialog::new(title, None, Some(impact), "Drop table", None),
                        cx,
                    );
                }
                Err(e) => this.fail(e, cx),
            },
        );
    }

    fn ask_drop_index(&mut self, name: &str, cx: &mut Context<Self>) {
        let title = format!("Drop index {name}?");
        let plan = drop_index_plan(&self.schema.name, name);
        self.open_dialog(
            DialogKind::DropIndex(name.to_string()),
            ImpactDialog::new(
                title,
                Some(plan),
                None,
                "Drop index",
                Some("I reviewed the statement; the index can be recreated later.".into()),
            ),
            cx,
        );
    }

    /// Index changes apply on their own and always run in place.
    fn add_index(&mut self, name: String, columns: Vec<String>, unique: bool, cx: &mut Context<Self>) {
        self.begin(cx);
        let label = self.shell.read(cx).label().to_string();
        let table = self.schema.name.clone();
        let spec = CreateIndex {
            name,
            table: table.clone(),
            columns,
            unique,
        };
        task::spawn_call(
            cx,
            move || create_index(label, spec),
            move |this, result, cx| match result {
                Ok(state) => {
                    this.busy = false;
                    this.adopt(&state, cx);
                    cx.emit(DesignerEvent::Changed(table));
                    cx.notify();
                }
                Err(e) => this.fail(e, cx),
            },
        );
    }

    pub fn field_settings(&self) -> &Entity<FieldSettingsEditor> {
        &self.field_settings
    }

    pub fn set_locked(&mut self, locked: bool, cx: &mut Context<Self>) {
        if self.locked != locked {
            self.locked = locked;
            cx.notify();
        }
    }

    pub fn locked(&self) -> bool {
        self.locked
    }

    #[cfg(test)]
    pub fn busy(&self) -> bool {
        self.busy
    }
}

fn on_change_notify(cx: &mut Context<TableSchemaDesigner>, input: &Entity<InputState>) -> Subscription {
    crate::ui::schema::controls::on_change(cx, input, |_, cx| cx.notify())
}

fn same(a: &[String], b: &[String]) -> bool {
    a == b
}

fn badge(id: SharedString, label: &str, destructive: bool, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (bg, fg) = if destructive {
        (theme.danger.opacity(0.15), theme.danger)
    } else {
        (theme.muted, theme.muted_foreground)
    };
    div()
        .px_2()
        .rounded_md()
        .text_xs()
        .bg(bg)
        .text_color(fg)
        .child(widgets::text(id, label.to_string()))
}

impl Render for TableSchemaDesigner {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let schema = self.schema.clone();
        let busy = self.busy || self.locked;
        let store = store_label(self.capabilities.as_ref());
        let theme = cx.theme();
        let (muted, border, danger) = (theme.muted_foreground, theme.border, theme.danger);
        let typed_name = input_text(&self.table_name, cx);
        let primary: Vec<String> = self.primary_key.read(cx).selected().to_vec();
        let schema_primary = if schema.primary_key.is_empty() {
            schema
                .columns
                .iter()
                .filter(|c| c.primary_key_position > 0)
                .map(|c| c.name.clone())
                .collect()
        } else {
            schema.primary_key.clone()
        };
        let new_name_empty = self.new_column.read(cx).draft().name.trim().is_empty();

        let header = h_flex()
            .justify_between()
            .items_center()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(widgets::text("design-heading", format!("Design {}", schema.name))),
                    )
                    .child(
                        div().text_sm().text_color(muted).child(widgets::text(
                            "designer-store",
                            format!(
                                "{} Changes are staged, previewed, and applied together in one transaction.",
                                store
                                    .map(|s| format!("{s} record store."))
                                    .unwrap_or_else(|| "Checking the record store…".into())
                            ),
                        )),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("drop-table")
                            .outline()
                            .small()
                            .icon(IconName::Delete)
                            .label("Drop table")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.ask_drop_table(cx))),
                    )
                    .child(
                        Button::new("close-designer")
                            .outline()
                            .small()
                            .label("Close")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(DesignerEvent::Cancel))),
                    ),
            );

        let table_section = section("Table", cx).child(
            h_flex()
                .gap_3()
                .items_end()
                .child(div().w(px(240.)).child(text_input(&self.table_name, "Table name", "Table name")))
                .child(
                    Button::new("rename-table")
                        .outline()
                        .small()
                        .label("Rename table")
                        .disabled(busy || typed_name.trim().is_empty() || typed_name == schema.name)
                        .on_click(cx.listener(|this, _, _, cx| {
                            let name = input_text(&this.table_name, cx).trim().to_string();
                            this.stage(vec![AlterTable::RenameTable { new_name: name }], cx);
                        })),
                ),
        );

        let column_rows: Vec<AnyElement> = self
            .rows
            .iter()
            .map(|row| {
                let name = row.name.clone();
                let (stage_name, drop_name) = (name.clone(), name.clone());
                h_flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .py_1()
                    .border_b_1()
                    .border_color(border)
                    .child(div().min_w(px(112.)).font_weight(FontWeight::SEMIBOLD).child(name.clone()))
                    .child(row.fields.clone())
                    .child(
                        Button::new(SharedString::from(format!("stage-column:{name}")))
                            .outline()
                            .small()
                            .label(format!("Stage changes to {name}"))
                            .on_click(cx.listener(move |this, _, _, cx| this.stage_column(&stage_name, cx))),
                    )
                    .child(
                        Button::new(SharedString::from(format!("drop-column:{name}")))
                            .outline()
                            .small()
                            .label("Drop")
                            .tooltip(format!("Drop {name}"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.stage(vec![AlterTable::DropColumn { column: drop_name.clone() }], cx)
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        let columns_section = section("Columns", cx)
            .children(column_rows)
            .child(
                h_flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .rounded_md()
                    .bg(theme.muted)
                    .child(self.new_column.clone())
                    .child(
                        Button::new("Add column")
                            .small()
                            .icon(IconName::Plus)
                            .label("Add column")
                            .disabled(new_name_empty || busy)
                            .on_click(cx.listener(|this, _, window, cx| this.add_column(window, cx))),
                    ),
            );

        let primary_section = section("Primary key", cx)
            .child(self.primary_key.clone())
            .child(
                div().child(
                    Button::new("set-primary-key")
                        .outline()
                        .small()
                        .label("Set primary key")
                        .disabled(same(&primary, &schema_primary))
                        .on_click(cx.listener(|this, _, _, cx| {
                            let columns = this.primary_key.read(cx).selected().to_vec();
                            this.stage(vec![AlterTable::SetPrimaryKey { columns }], cx);
                        })),
                ),
            );

        let relationship_rows: Vec<AnyElement> = schema
            .foreign_keys
            .iter()
            .map(|key| {
                let columns = key.from_columns.clone();
                let id = columns.join(",");
                h_flex()
                    .flex_wrap()
                    .items_center()
                    .gap_3()
                    .child(div().text_sm().child(format!(
                        "({}) \u{2192} {} ({}) \u{b7} update {} \u{b7} delete {}",
                        key.from_columns.join(", "),
                        key.target_table,
                        key.target_columns.join(", "),
                        key.on_update,
                        key.on_delete
                    )))
                    .child(
                        Button::new(SharedString::from(format!("remove-fk:{id}")))
                            .outline()
                            .xsmall()
                            .label(format!("Remove relationship ({})", columns.join(", ")))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.stage(vec![AlterTable::DropForeignKey { columns: columns.clone() }], cx)
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        let relationships_section = section("Relationships", cx)
            .children(relationship_rows)
            .child(self.fk_editor.clone());

        let unique_rows: Vec<AnyElement> = schema
            .uniques
            .iter()
            .map(|u| {
                let columns = u.columns.clone();
                let id = columns.join(",");
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_sm().child(format!("Unique ({})", columns.join(", "))))
                    .child(
                        Button::new(SharedString::from(format!("remove-unique:{id}")))
                            .outline()
                            .xsmall()
                            .label(format!("Remove unique ({})", columns.join(", ")))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.stage(vec![AlterTable::DropUnique { columns: columns.clone() }], cx)
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        let check_rows: Vec<AnyElement> = schema
            .checks
            .iter()
            .map(|c| {
                let target = c.name.clone().unwrap_or_else(|| c.expression.clone());
                let expression = c.expression.clone();
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_sm().child(format!("Check ({expression})")))
                    .child(
                        Button::new(SharedString::from(format!("remove-check:{expression}")))
                            .outline()
                            .xsmall()
                            .label(format!("Remove check ({expression})"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.stage(vec![AlterTable::DropCheck { expression: target.clone() }], cx)
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        let constraints_section = section("Constraints", cx)
            .children(unique_rows)
            .children(check_rows)
            .child(self.unique_editor.clone())
            .child(self.check_editor.clone());

        let index_rows: Vec<AnyElement> = schema
            .indexes
            .iter()
            .map(|index| {
                let name = index.name.clone();
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_sm().child(format!(
                        "{} {} ({})",
                        if index.unique { "Unique index" } else { "Index" },
                        index.name,
                        index.columns.join(", ")
                    )))
                    .child(
                        Button::new(SharedString::from(format!("drop-index:{name}")))
                            .outline()
                            .xsmall()
                            .label(format!("Drop index {name}"))
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, _, cx| this.ask_drop_index(&name, cx))),
                    )
                    .into_any_element()
            })
            .collect();
        let indexes_section = section("Indexes", cx)
            .child(note(
                "Index changes are not staged: they apply on their own and always run in place.",
                cx,
            ))
            .children(index_rows)
            .child(self.index_editor.clone());

        let pending_items: Vec<AnyElement> = self
            .pending
            .iter()
            .enumerate()
            .map(|(i, op)| {
                let planned = self.plan.as_ref().and_then(|p| p.operations.get(i));
                let summary = planned.map_or_else(|| operation_name(op), |p| p.summary.clone());
                let mode = planned.map_or("Checking…", |p| mode_label(change_mode_key(&p.mode)));
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_sm().child(widgets::text(SharedString::from(format!("pending-op:{i}")), summary)))
                    .child(badge(SharedString::from(format!("pending-mode:{i}")), mode, false, cx))
                    .when(planned.is_some_and(|p| p.destructive), |el| {
                        el.child(badge(SharedString::from(format!("pending-destructive:{i}")), "Destructive", true, cx))
                    })
                    .child(
                        Button::new(SharedString::from(format!("remove-pending:{i}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Delete)
                            .tooltip(format!("Remove pending change {}", i + 1))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.pending.remove(i);
                                this.refresh_plan(cx);
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        let pending_section = section("Pending changes", cx)
            .when(self.pending.is_empty(), |el| {
                el.child(div().text_sm().text_color(muted).child(widgets::text("pending-empty", "No staged changes.")))
            })
            .children(pending_items)
            .when(!self.plan_error.is_empty(), |el| el.child(error_line("plan-error", &self.plan_error, cx)))
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("discard-changes")
                            .outline()
                            .label("Discard changes")
                            .disabled(self.pending.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pending.clear();
                                this.refresh_plan(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("apply-changes")
                            .label("Apply changes")
                            .disabled(busy || self.pending.is_empty() || !self.plan_error.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| this.apply(cx))),
                    ),
            );

        div()
            .relative()
            .size_full()
            .child(
                v_flex()
                    .id("table-designer")
                    .size_full()
                    .overflow_y_scroll()
                    .gap_3()
                    .p_4()
                    .child(header)
                    .when(!self.error.is_empty() && self.dialog.is_none(), |el| {
                        el.child(div().text_color(danger).child(error_line("designer-error", &self.error, cx)))
                    })
                    .child(table_section)
                    .child(columns_section)
                    .child(self.field_settings.clone())
                    .child(primary_section)
                    .child(relationships_section)
                    .child(constraints_section)
                    .child(indexes_section)
                    .child(pending_section)
                    .test_support(),
            )
            .when_some(self.dialog.as_ref().map(|d| d.view.clone()), |el, view| el.child(view))
    }
}

/// Columns of the table (used by tests to assert what the designer shows).
pub fn column_names(columns: &[Column]) -> Vec<String> {
    columns.iter().map(|c| c.name.clone()).collect()
}
