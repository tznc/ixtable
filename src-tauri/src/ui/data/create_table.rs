//! Create-table form: logical types, keys (including composite), relationships,
//! constraints and indexes (port of `src/data/CreateTableForm.tsx`).
//!
//! Test ids: `create-table-heading`; `Table name` (input); per column `Column N:`
//! `name|type|precision|scale|required|unique|default|check` (see `ColumnFields`),
//! `Column N primary key` (checkbox) and `Remove column N`; `Add column`; the
//! constraint editors' ids (see `schema::constraints`); `Remove relationship N`;
//! `submit-create-table` and `cancel-create-table`; `create-table-error` for the alert.

use crate::ui::controls::{check, error_line, input_text, new_input, on_change, section, text_input};
use crate::data::{CreateForeignKey, CreateTable, IndexSpec};
use crate::recordstore::capabilities::StoreCapabilities;
use crate::ui::{
    prelude::*,
    schema::{
        column_fields::{ColumnFields, ColumnFieldsEvent},
        columns::{empty_column, spec_from_draft},
        constraints::{
            CheckEditor, CheckEvent, ForeignKeyEditor, ForeignKeyEvent, IndexEditor, IndexEvent,
            TableRef, UniqueEditor, UniqueEvent,
        },
        logical::store_types,
    },
    shell::Shell,
};
use gpui_kit::component::input::InputState;

pub enum CreateTableEvent {
    /// The table was created; carries its name.
    Created(String),
    Cancelled,
}

struct Row {
    fields: Entity<ColumnFields>,
    pk: bool,
}

pub struct CreateTableForm {
    shell: Entity<Shell>,
    tables: Vec<TableRef>,
    capabilities: Option<StoreCapabilities>,
    name: Entity<InputState>,
    rows: Vec<Row>,
    foreign_keys: Vec<CreateForeignKey>,
    uniques: Vec<Vec<String>>,
    checks: Vec<String>,
    indexes: Vec<IndexSpec>,
    fk_editor: Entity<ForeignKeyEditor>,
    unique_editor: Entity<UniqueEditor>,
    check_editor: Entity<CheckEditor>,
    index_editor: Entity<IndexEditor>,
    error: Option<String>,
    busy: bool,
    _subscriptions: Vec<Subscription>,
}
impl EventEmitter<CreateTableEvent> for CreateTableForm {}

impl CreateTableForm {
    pub fn new(
        shell: Entity<Shell>,
        tables: Vec<TableRef>,
        capabilities: Option<StoreCapabilities>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = new_input(window, cx, "", "");
        let actions = capabilities.as_ref().map(|c| c.foreign_key_actions.clone());
        let fk_editor = {
            let tables = tables.clone();
            cx.new(|cx| ForeignKeyEditor::new(vec![], tables, actions, None, "Add relationship", cx))
        };
        let unique_editor = cx.new(|cx| UniqueEditor::new(vec![], cx));
        let check_editor = cx.new(|cx| CheckEditor::new(window, cx));
        let index_editor = cx.new(|cx| IndexEditor::new("table", vec![], window, cx));
        let subscriptions = vec![
            on_change(cx, &name, |this, cx| this.refresh_editors(cx)),
            cx.subscribe(&fk_editor, |this, _, e: &ForeignKeyEvent, cx| {
                let ForeignKeyEvent::Add(fk) = e;
                let own = this.table_name(cx);
                let mut fk = fk.clone();
                if fk.target_table == "this table" {
                    fk.target_table = own;
                }
                this.foreign_keys.push(fk);
                cx.notify();
            }),
            cx.subscribe(&unique_editor, |this, _, e: &UniqueEvent, cx| {
                let UniqueEvent::Add(columns) = e;
                this.uniques.push(columns.clone());
                cx.notify();
            }),
            cx.subscribe(&check_editor, |this, _, e: &CheckEvent, cx| {
                let CheckEvent::Add(expression) = e;
                this.checks.push(expression.clone());
                cx.notify();
            }),
            cx.subscribe(&index_editor, |this, _, e: &IndexEvent, cx| {
                let IndexEvent::Add(index) = e;
                this.indexes.push(IndexSpec {
                    name: index.name.clone(),
                    columns: index.columns.clone(),
                    unique: index.unique,
                });
                cx.notify();
            }),
        ];
        let mut this = Self {
            shell,
            tables,
            capabilities,
            name,
            rows: vec![],
            foreign_keys: vec![],
            uniques: vec![],
            checks: vec![],
            indexes: vec![],
            fk_editor,
            unique_editor,
            check_editor,
            index_editor,
            error: None,
            busy: false,
            _subscriptions: subscriptions,
        };
        let mut id = empty_column("id");
        id.logical_type = "integer".into();
        id.required = true;
        this.push_row(id, true, window, cx);
        this.push_row(empty_column("name"), false, window, cx);
        this.name.update(cx, |s, cx| s.focus(window, cx));
        this
    }

    /// Capabilities arrived or changed: the store's types and referential actions.
    pub fn set_capabilities(
        &mut self,
        capabilities: Option<StoreCapabilities>,
        cx: &mut Context<Self>,
    ) {
        self.capabilities = capabilities;
        let (types, max) = self.store();
        for row in &self.rows {
            row.fields.update(cx, |f, cx| f.set_store(types.clone(), max, cx));
        }
        let actions = self.capabilities.as_ref().map(|c| c.foreign_key_actions.clone());
        if let Some(actions) = actions {
            self.fk_editor.update(cx, |e, cx| e.set_actions(actions, cx));
        }
        cx.notify();
    }

    pub fn set_tables(&mut self, tables: Vec<TableRef>, cx: &mut Context<Self>) {
        self.tables = tables;
        self.refresh_editors(cx);
    }

    fn store(&self) -> (Option<Vec<String>>, u32) {
        let max = self
            .capabilities
            .as_ref()
            .and_then(|c| {
                c.logical_types
                    .iter()
                    .find(|t| t.logical_type.starts_with("decimal"))
                    .and_then(|t| t.max_precision)
            })
            .map_or(38, u32::from);
        (store_types(self.capabilities.as_ref()), max)
    }

    fn table_name(&self, cx: &App) -> String {
        input_text(&self.name, cx).trim().to_string()
    }

    fn column_names(&self, cx: &App) -> Vec<String> {
        self.rows
            .iter()
            .map(|r| r.fields.read(cx).draft().name.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect()
    }

    fn push_row(
        &mut self,
        draft: crate::ui::schema::columns::ColumnDraft,
        pk: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = format!("Column {}", self.rows.len() + 1);
        let fields = cx.new(|cx| ColumnFields::new(&label, draft, true, window, cx));
        let (types, max) = self.store();
        fields.update(cx, |f, cx| f.set_store(types, max, cx));
        self.rows.push(Row { fields, pk });
        self.subscribe_row(cx);
        self.refresh_editors(cx);
    }

    fn subscribe_row(&mut self, cx: &mut Context<Self>) {
        let fields = self.rows.last().expect("row").fields.clone();
        let subscription = cx.subscribe(&fields, |this, _, _: &ColumnFieldsEvent, cx| {
            this.refresh_editors(cx)
        });
        self._subscriptions.push(subscription);
    }

    /// The editors offer the form's own columns, and its table as a relationship target.
    fn refresh_editors(&mut self, cx: &mut Context<Self>) {
        let names = self.column_names(cx);
        let own = self.table_name(cx);
        let mut targets = self.tables.clone();
        targets.push(TableRef {
            name: if own.is_empty() { "this table".into() } else { own.clone() },
            columns: names.clone(),
        });
        self.fk_editor.update(cx, |e, cx| {
            e.set_columns(names.clone(), cx);
            e.set_tables(targets, cx);
        });
        self.unique_editor.update(cx, |e, cx| e.set_columns(names.clone(), cx));
        self.index_editor.update(cx, |e, cx| {
            e.set_columns(names.clone(), cx);
            e.set_table(if own.is_empty() { "table" } else { &own }, cx);
        });
        cx.notify();
    }

    fn remove_row(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.rows.len() {
            self.rows.remove(index);
            self.refresh_editors(cx);
        }
    }

    fn set_pk(&mut self, index: usize, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get_mut(index) else {
            return;
        };
        row.pk = on;
        if on {
            let mut draft = row.fields.read(cx).draft().clone();
            draft.required = true;
            row.fields.update(cx, |f, cx| f.set_draft(draft, window, cx));
        }
        cx.notify();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let name = self.table_name(cx);
        let drafts: Vec<_> = self.rows.iter().map(|r| r.fields.read(cx).draft().clone()).collect();
        if name.is_empty() || drafts.is_empty() || drafts.iter().any(|d| d.name.trim().is_empty()) {
            self.error = Some("Table and column names are required.".into());
            cx.notify();
            return;
        }
        self.busy = true;
        self.error = None;
        let mut position = 0;
        let columns = drafts
            .iter()
            .zip(&self.rows)
            .map(|(d, row)| {
                if row.pk {
                    position += 1;
                }
                spec_from_draft(d, if row.pk { position } else { 0 })
            })
            .collect();
        let spec = CreateTable {
            name: name.clone(),
            columns,
            foreign_keys: self.foreign_keys.clone(),
            checks: self.checks.clone(),
            without_rowid: false,
            uniques: self.uniques.clone(),
            indexes: self.indexes.clone(),
        };
        let label = self.shell.read(cx).label().to_string();
        task::spawn_call(
            cx,
            move || crate::create_database_table(label, spec),
            move |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(state) => {
                        this.shell.update(cx, |s, cx| s.apply_session(&state, cx));
                        cx.emit(CreateTableEvent::Created(name));
                    }
                    Err(e) => this.error = Some(e.message),
                }
                cx.notify();
            },
        );
        cx.notify();
    }
}

impl Render for CreateTableForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let muted = cx.theme().muted_foreground;
        let rows: Vec<AnyElement> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let n = i + 1;
                let entity = entity.clone();
                let entity2 = entity.clone();
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .items_center()
                    .child(row.fields.clone())
                    .child(check(
                        SharedString::from(format!("Column {n} primary key")),
                        "Primary key",
                        row.pk,
                        move |value, window, cx| {
                            entity.update(cx, |this, cx| this.set_pk(i, value, window, cx))
                        },
                    ))
                    .child(
                        Button::new(SharedString::from(format!("Remove column {n}")))
                            .ghost()
                            .small()
                            .icon(IconName::Trash)
                            .tooltip(format!("Remove column {n}"))
                            .on_click(move |_, _, cx| {
                                entity2.update(cx, |this, cx| this.remove_row(i, cx))
                            }),
                    )
                    .into_any_element()
            })
            .collect();
        let summary = |text: String| div().text_sm().child(text);
        let body = v_flex()
            .id("create-table-body")
            .size_full()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap_3()
            .p_4()
            .child(widgets::section_heading("create-table-heading", "Create table"))
            .when_some(self.error.clone(), |el, e| {
                el.child(error_line("create-table-error", &e, cx))
            })
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_sm().text_color(muted).child("Table name"))
                    .child(div().w(px(280.)).child(text_input(&self.name, "Table name", "Table name"))),
            )
            .child(
                section("Columns", cx)
                    .children(rows)
                    .child(
                        Button::new("Add column")
                            .outline()
                            .small()
                            .icon(IconName::Plus)
                            .label("Add column")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.push_row(empty_column(""), false, window, cx)
                            })),
                    ),
            )
            .child({
                let fks = self.foreign_keys.len();
                crate::ui::section::section("create-relationships", "Relationships", false, window, cx)
                    .count(fks)
                    .summary(if fks == 0 { "None yet. Expand to link this table to another." } else { "" })
                    .child(v_flex().gap_2().children(self.foreign_keys.iter().enumerate().map(|(i, f)| {
                        let entity = entity.clone();
                        h_flex()
                            .gap_2()
                            .items_center()
                            .text_sm()
                            .child(format!(
                                "({}) → {} ({}) · update {} · delete {}",
                                f.columns.join(", "),
                                f.target_table,
                                f.target_columns.join(", "),
                                f.on_update.clone().unwrap_or_default(),
                                f.on_delete.clone().unwrap_or_default()
                            ))
                            .child(
                                Button::new(SharedString::from(format!("Remove relationship {}", i + 1)))
                                    .danger()
                                    .outline()
                                    .xsmall()
                                    .label("Remove")
                                    .on_click(move |_, _, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.foreign_keys.remove(i);
                                            cx.notify();
                                        })
                                    }),
                            )
                    })))
                    .child(self.fk_editor.clone())
                    .render(cx)
            })
            .child({
                let n = self.uniques.len() + self.checks.len() + self.indexes.len();
                crate::ui::section::section("create-constraints", "Constraints and indexes", false, window, cx)
                    .count(n)
                    .summary(if n == 0 { "None yet. Expand to add unique rules, checks or indexes." } else { "" })
                    .child(v_flex().gap_2().children(self.uniques.iter().map(|u| {
                        summary(format!("Unique ({})", u.join(", ")))
                    })))
                    .child(v_flex().gap_2().children(
                        self.checks
                            .iter()
                            .map(|c| summary(format!("Check ({c})"))),
                    ))
                    .child(v_flex().gap_2().children(self.indexes.iter().map(|x| {
                        summary(
                            format!(
                                "{} {} ({})",
                                if x.unique { "Unique index" } else { "Index" },
                                x.name,
                                x.columns.join(", ")
                            ),
                        )
                    })))
                    .child(self.unique_editor.clone())
                    .child(self.check_editor.clone())
                    .child(self.index_editor.clone())
                    .render(cx)
            })
            .test_support();
        v_flex()
            .id("create-table-form")
            .size_full()
            .child(body)
            .child(div().p_3().border_t_1().border_color(cx.theme().border).child(
h_flex()
                    .gap_2()
                    .child(
                        Button::new("cancel-create-table")
                            .outline()
                            .label("Cancel")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(CreateTableEvent::Cancelled))),
                    )
                    .child(
                        Button::new("submit-create-table")
                            .primary()
                            .label("Create table")
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                    ),
            ))
    }
}
