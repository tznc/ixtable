//! Editors for relationships, unique constraints, checks and indexes (port of
//! `src/schema/ConstraintEditors.tsx`). Each editor is a view that emits an `Add`
//! event with the finished definition.
//!
//! Test ids follow the labels the controls have: a picker's checkboxes are
//! `<label>: <column>` (for example `Relationship columns: region`), the relationship
//! selects are `Target table`, `Target column for <column>`, `On update` and
//! `On delete`, and the buttons are the add label (`Add relationship`,
//! `Add unique constraint`, `Add check`, `Add index`). Inputs are `Check expression`
//! and `Index name`; `Unique index` is a checkbox.

use crate::ui::controls::{check, input_text, new_input, on_change, plain_options, select, set_input, text_input};
use crate::data::CreateForeignKey;
use crate::ui::prelude::*;
use gpui_kit::component::input::InputState;
use std::collections::HashMap;

/// Referential actions offered while store capabilities load.
pub const ACTIONS: &[&str] = &["NO ACTION", "RESTRICT", "CASCADE", "SET NULL", "SET DEFAULT"];

/// A table the relationship editor can point at: its name and column names.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableRef {
    pub name: String,
    pub columns: Vec<String>,
}

impl TableRef {
    pub fn of(schema: &crate::data::TableSchema) -> Self {
        Self {
            name: schema.name.clone(),
            columns: schema.columns.iter().map(|c| c.name.clone()).collect(),
        }
    }
}

// ---- Column picker --------------------------------------------------------------

pub enum PickerEvent {
    Changed,
}

/// Ordered multi-column choice: columns are listed in the order they were ticked.
pub struct ColumnPicker {
    label: String,
    columns: Vec<String>,
    selected: Vec<String>,
}
impl EventEmitter<PickerEvent> for ColumnPicker {}

impl ColumnPicker {
    pub fn new(label: &str, columns: Vec<String>, selected: Vec<String>) -> Self {
        Self {
            label: label.to_string(),
            columns,
            selected,
        }
    }

    pub fn selected(&self) -> &[String] {
        &self.selected
    }

    pub fn set_columns(&mut self, columns: Vec<String>, cx: &mut Context<Self>) {
        self.selected.retain(|s| columns.contains(s));
        self.columns = columns;
        cx.notify();
    }

    pub fn set_selected(&mut self, selected: Vec<String>, cx: &mut Context<Self>) {
        self.selected = selected;
        cx.emit(PickerEvent::Changed);
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.set_selected(vec![], cx);
    }

    fn toggle(&mut self, column: &str, on: bool, cx: &mut Context<Self>) {
        let mut next: Vec<String> = self.selected.iter().filter(|c| *c != column).cloned().collect();
        if on {
            next.push(column.to_string());
        }
        self.set_selected(next, cx);
    }
}

impl Render for ColumnPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let muted = cx.theme().muted_foreground;
        v_flex()
            .gap_1()
            .child(div().text_sm().text_color(muted).child(self.label.clone()))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_3()
                    .children(self.columns.iter().map(|c| {
                        let on = self.selected.contains(c);
                        let order = self.selected.iter().position(|x| x == c);
                        let entity = entity.clone();
                        let column = c.clone();
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(check(
                                SharedString::from(format!("{}: {c}", self.label)),
                                c,
                                on,
                                move |value, _, cx| {
                                    entity.update(cx, |this, cx| this.toggle(&column, value, cx))
                                },
                            ))
                            .when(on && self.selected.len() > 1, |el| {
                                el.child(
                                    div()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(format!("#{}", order.unwrap_or(0) + 1)),
                                )
                            })
                    })),
            )
    }
}

// ---- Relationship ---------------------------------------------------------------

/// What a drawn or previously previewed relationship prefills.
#[derive(Clone, Debug, Default)]
pub struct ForeignKeyDraft {
    pub columns: Vec<String>,
    pub target_table: String,
    pub target_columns: Vec<String>,
    pub on_update: Option<String>,
    pub on_delete: Option<String>,
}

pub enum ForeignKeyEvent {
    Add(CreateForeignKey),
}

/// Builds a (possibly composite) foreign key with referential actions.
pub struct ForeignKeyEditor {
    picker: Entity<ColumnPicker>,
    tables: Vec<TableRef>,
    actions: Vec<String>,
    target: String,
    targets: HashMap<String, String>,
    on_update: String,
    on_delete: String,
    add_label: String,
    _subscription: Subscription,
}
impl EventEmitter<ForeignKeyEvent> for ForeignKeyEditor {}

impl ForeignKeyEditor {
    pub fn new(
        columns: Vec<String>,
        tables: Vec<TableRef>,
        actions: Option<Vec<String>>,
        initial: Option<ForeignKeyDraft>,
        add_label: &str,
        cx: &mut Context<Self>,
    ) -> Self {
        let actions = actions.unwrap_or_else(|| ACTIONS.iter().map(|a| a.to_string()).collect());
        let initial = initial.unwrap_or_default();
        let picker = cx.new(|_| {
            ColumnPicker::new("Relationship columns", columns, initial.columns.clone())
        });
        let subscription = cx.subscribe(&picker, |_, _, _: &PickerEvent, cx| cx.notify());
        let first = actions.first().cloned().unwrap_or_else(|| "NO ACTION".into());
        Self {
            picker,
            tables,
            targets: initial
                .columns
                .iter()
                .enumerate()
                .map(|(i, c)| (c.clone(), initial.target_columns.get(i).cloned().unwrap_or_default()))
                .collect(),
            target: initial.target_table,
            on_update: initial.on_update.unwrap_or_else(|| first.clone()),
            on_delete: initial.on_delete.unwrap_or(first),
            actions,
            add_label: add_label.to_string(),
            _subscription: subscription,
        }
    }

    pub fn set_columns(&mut self, columns: Vec<String>, cx: &mut Context<Self>) {
        self.picker.update(cx, |p, cx| p.set_columns(columns, cx));
        cx.notify();
    }

    pub fn set_tables(&mut self, tables: Vec<TableRef>, cx: &mut Context<Self>) {
        self.tables = tables;
        cx.notify();
    }

    pub fn set_actions(&mut self, actions: Vec<String>, cx: &mut Context<Self>) {
        self.actions = actions;
        cx.notify();
    }

    fn ready(&self, cx: &App) -> bool {
        let source = self.picker.read(cx).selected();
        !source.is_empty()
            && !self.target.is_empty()
            && source.iter().all(|c| self.targets.get(c).is_some_and(|t| !t.is_empty()))
    }

    fn add(&mut self, cx: &mut Context<Self>) {
        let source: Vec<String> = self.picker.read(cx).selected().to_vec();
        cx.emit(ForeignKeyEvent::Add(CreateForeignKey {
            name: None,
            target_columns: source
                .iter()
                .map(|c| self.targets.get(c).cloned().unwrap_or_default())
                .collect(),
            columns: source,
            target_table: self.target.clone(),
            on_update: Some(self.on_update.clone()),
            on_delete: Some(self.on_delete.clone()),
        }));
        self.picker.update(cx, |p, cx| p.clear(cx));
        self.targets.clear();
        cx.notify();
    }
}

impl Render for ForeignKeyEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let source: Vec<String> = self.picker.read(cx).selected().to_vec();
        let target_columns: Vec<String> = self
            .tables
            .iter()
            .find(|t| t.name == self.target)
            .map(|t| t.columns.clone())
            .unwrap_or_default();
        let ready = self.ready(cx);
        let muted = cx.theme().muted_foreground;
        let labelled = |label: &str, control: AnyElement| {
            v_flex()
                .gap_1()
                .child(div().text_sm().text_color(muted).child(label.to_string()))
                .child(control)
        };
        v_flex()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .child(div().font_weight(FontWeight::MEDIUM).child("New relationship"))
            .child(self.picker.clone())
            .child(labelled(
                "Target table",
                select(
                    "Target table",
                    &self.target,
                    self.tables.iter().map(|t| (t.name.clone(), t.name.clone())).collect(),
                    "Choose a table",
                    {
                        let entity = entity.clone();
                        move |value, _, cx| {
                            let value = value.to_string();
                            entity.update(cx, |this, cx| {
                                this.target = value;
                                this.targets.clear();
                                cx.notify();
                            })
                        }
                    },
                )
                .into_any_element(),
            ))
            .children(source.iter().map(|c| {
                let entity = entity.clone();
                let column = c.clone();
                labelled(
                    &format!("Target column for {c}"),
                    select(
                        SharedString::from(format!("Target column for {c}")),
                        self.targets.get(c).map_or("", String::as_str),
                        plain_options(&target_columns),
                        "Choose a column",
                        move |value, _, cx| {
                            let value = value.to_string();
                            let column = column.clone();
                            entity.update(cx, |this, cx| {
                                this.targets.insert(column, value);
                                cx.notify();
                            })
                        },
                    )
                    .into_any_element(),
                )
            }))
            .child(labelled(
                "On update",
                select("On update", &self.on_update, plain_options(&self.actions), "Action", {
                    let entity = entity.clone();
                    move |value, _, cx| {
                        let value = value.to_string();
                        entity.update(cx, |this, cx| {
                            this.on_update = value;
                            cx.notify();
                        })
                    }
                })
                .into_any_element(),
            ))
            .child(labelled(
                "On delete",
                select("On delete", &self.on_delete, plain_options(&self.actions), "Action", {
                    let entity = entity.clone();
                    move |value, _, cx| {
                        let value = value.to_string();
                        entity.update(cx, |this, cx| {
                            this.on_delete = value;
                            cx.notify();
                        })
                    }
                })
                .into_any_element(),
            ))
            .child(
                Button::new(SharedString::from(self.add_label.clone()))
                    .outline()
                    .small()
                    .icon(IconName::Plus)
                    .label(self.add_label.clone())
                    .disabled(!ready)
                    .on_click(cx.listener(|this, _, _, cx| this.add(cx))),
            )
    }
}

// ---- Unique ---------------------------------------------------------------------

pub enum UniqueEvent {
    Add(Vec<String>),
}

/// Multi-column unique constraint builder.
pub struct UniqueEditor {
    picker: Entity<ColumnPicker>,
    _subscription: Subscription,
}
impl EventEmitter<UniqueEvent> for UniqueEditor {}

impl UniqueEditor {
    pub fn new(columns: Vec<String>, cx: &mut Context<Self>) -> Self {
        let picker = cx.new(|_| ColumnPicker::new("Unique columns", columns, vec![]));
        let subscription = cx.subscribe(&picker, |_, _, _: &PickerEvent, cx| cx.notify());
        Self {
            picker,
            _subscription: subscription,
        }
    }

    pub fn set_columns(&mut self, columns: Vec<String>, cx: &mut Context<Self>) {
        self.picker.update(cx, |p, cx| p.set_columns(columns, cx));
    }
}

impl Render for UniqueEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = self.picker.read(cx).selected().is_empty();
        v_flex()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .child(div().font_weight(FontWeight::MEDIUM).child("New unique constraint"))
            .child(self.picker.clone())
            .child(
                Button::new("Add unique constraint")
                    .outline()
                    .small()
                    .icon(IconName::Plus)
                    .label("Add unique constraint")
                    .disabled(empty)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let selected = this.picker.read(cx).selected().to_vec();
                        cx.emit(UniqueEvent::Add(selected));
                        this.picker.update(cx, |p, cx| p.clear(cx));
                    })),
            )
    }
}

// ---- Check ----------------------------------------------------------------------

pub enum CheckEvent {
    Add(String),
}

pub struct CheckEditor {
    input: Entity<InputState>,
    _subscription: Subscription,
}
impl EventEmitter<CheckEvent> for CheckEditor {}

impl CheckEditor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = new_input(window, cx, "e.g. end_date >= start_date", "");
        let subscription = on_change(cx, &input, |_, cx| cx.notify());
        Self {
            input,
            _subscription: subscription,
        }
    }
}

impl Render for CheckEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = input_text(&self.input, cx).trim().is_empty();
        h_flex()
            .gap_2()
            .items_center()
            .child("Check expression")
            .child(
                div()
                    .w(px(280.))
                    .child(text_input(&self.input, "Check expression", "Check expression")),
            )
            .child(
                Button::new("Add check")
                    .outline()
                    .small()
                    .icon(IconName::Plus)
                    .label("Add check")
                    .disabled(empty)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let expression = input_text(&this.input, cx).trim().to_string();
                        cx.emit(CheckEvent::Add(expression));
                        set_input(&this.input, "", window, cx);
                        cx.notify();
                    })),
            )
    }
}

// ---- Index ----------------------------------------------------------------------

pub struct IndexDraft {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

pub enum IndexEvent {
    Add(IndexDraft),
}

/// Unique or non-unique, single or multi-column index builder.
pub struct IndexEditor {
    table: String,
    name: Entity<InputState>,
    picker: Entity<ColumnPicker>,
    unique: bool,
    _subscriptions: Vec<Subscription>,
}
impl EventEmitter<IndexEvent> for IndexEditor {}

impl IndexEditor {
    pub fn new(table: &str, columns: Vec<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = new_input(window, cx, "Defaults to a name made from the columns", "");
        let picker = cx.new(|_| ColumnPicker::new("Index columns", columns, vec![]));
        let subscriptions = vec![
            on_change(cx, &name, |_, cx| cx.notify()),
            cx.subscribe(&picker, |_, _, _: &PickerEvent, cx| cx.notify()),
        ];
        Self {
            table: table.to_string(),
            name,
            picker,
            unique: false,
            _subscriptions: subscriptions,
        }
    }

    pub fn set_table(&mut self, table: &str, cx: &mut Context<Self>) {
        self.table = table.to_string();
        cx.notify();
    }

    pub fn set_columns(&mut self, columns: Vec<String>, cx: &mut Context<Self>) {
        self.picker.update(cx, |p, cx| p.set_columns(columns, cx));
    }

    /// The name used when none is typed: table and columns, lower-cased.
    pub fn suggested(&self, cx: &App) -> String {
        let selected = self.picker.read(cx).selected().join("_");
        let base = format!("{}_{}", self.table, if selected.is_empty() { "idx" } else { &selected });
        base.to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
            .collect()
    }
}

impl Render for IndexEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = self.picker.read(cx).selected().is_empty();
        let suggested = self.suggested(cx);
        let entity = cx.entity();
        v_flex()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .child(div().font_weight(FontWeight::MEDIUM).child("New index"))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child("Index name")
                    .child(div().w(px(240.)).child(text_input(&self.name, "Index name", "Index name")))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("Default: {suggested}")),
                    ),
            )
            .child(self.picker.clone())
            .child(check("Unique index", "Unique index", self.unique, move |value, _, cx| {
                entity.update(cx, |this, cx| {
                    this.unique = value;
                    cx.notify();
                })
            }))
            .child(
                Button::new("Add index")
                    .outline()
                    .small()
                    .icon(IconName::Plus)
                    .label("Add index")
                    .disabled(empty)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let typed = input_text(&this.name, cx).trim().to_string();
                        let draft = IndexDraft {
                            name: if typed.is_empty() { this.suggested(cx) } else { typed },
                            columns: this.picker.read(cx).selected().to_vec(),
                            unique: this.unique,
                        };
                        cx.emit(IndexEvent::Add(draft));
                        set_input(&this.name, "", window, cx);
                        this.picker.update(cx, |p, cx| p.clear(cx));
                        this.unique = false;
                        cx.notify();
                    })),
            )
    }
}
