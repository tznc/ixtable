//! The Data mode datasheet (port of `src/data/sheet/Datasheet.tsx`): in-place editing,
//! a new-record row, multi-column sort, filter by selection, find and replace, hidden
//! and frozen columns, a totals row, and multi-cell paste from spreadsheets. Hidden,
//! frozen and totals settings are saved per table in the document's navigation state.
//!
//! The page lives in the workbench, which pushes it with `set_page` and reacts to the
//! events this view emits. Clicking a cell selects it; clicking an editable cell also
//! opens the shared cell editor (`cell-input`) on it. Enter or leaving the editor
//! commits a changed value.
//!
//! Test ids: `cell:<row>:<column index>` (label = the text shown), `cell-input`,
//! `New <column>` (draft inputs), `insert-row`, `delete-row:<row>`, `sort:<column>`
//! and `Column options for <column>` in the header, `head:<column>` (label notes
//! `frozen`), `filter` (menu: Filter by selection / Filter excluding selection), `find`, `totals`,
//! `hidden-columns`, `unfreeze-columns`, `filter-chip:<n>`, `remove-filter:<n>`,
//! `clear-filters`, `Total for <column>` and `total:<column>`,
//! `draft-error`.

use crate::ui::controls::select;
use super::{
    filters::{add_filter, describe_filter, selection_filter, SheetFilter},
    find::CellAt,
    find_panel::{FindEvent, FindPanel},
    layout::{freeze_through, hide_column, read_layout, visible_columns, write_layout, DatasheetLayout},
    paste::{is_multi_cell, parse_clipboard_grid, paste_writes, pasted_message, plan_paste},
    rows::{is_editable, original_values, typed_values, SheetFields},
    sorts::next_sorts,
    totals::{total_from_key, total_key, totals_for},
};
use crate::data::{totals::TotalFunction, totals::TotalSpec, DataValue, Filter, NamedValue, Page, Sort};
use crate::manager::AppError;
use crate::records::{self, RecordWriteMeta};
use crate::ui::{
    dialogs,
    fields::{datasheet::datasheet_text, datasheet::datasheet_value},
    prelude::*,
    schema::{
        logical::{logical_of, show_value},
    },
    shell::Shell,
    store::StoreEvent,
};
use gpui_kit::component::{
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
};
use std::collections::{BTreeMap, HashMap};

const COLUMN_WIDTH: f32 = 170.0;
const ROWNUM_WIDTH: f32 = 56.0;
const ACTIONS_WIDTH: f32 = 72.0;
const HEAD_HEIGHT: f32 = 48.0;
const ROW_HEIGHT: f32 = 32.0;
const TOTAL_HEIGHT: f32 = 40.0;

pub enum SheetEvent {
    /// A write committed: mark the document dirty and reload the page.
    Written,
    Refresh,
    /// The message to show in the workbench's banner ("" clears it).
    Error(String),
    Sorts(Vec<Sort>),
    Filters(Vec<SheetFilter>),
    Offset(u64),
}

pub struct Datasheet {
    pub shell: Entity<Shell>,
    pub table: String,
    pub read_only: bool,
    pub page: Page,
    pub page_size: u64,
    pub revision: u64,
    pub sorts: Vec<Sort>,
    /// Every filter the page was read with: the search box plus `filters`.
    pub read_filters: Vec<Filter>,
    /// The filters applied from the datasheet (filter by selection).
    pub filters: Vec<SheetFilter>,
    pub fields: SheetFields,
    pub layout: DatasheetLayout,
    pub active: Option<CellAt>,
    pub editing: bool,
    pub draft_error: Option<String>,
    editor: Entity<InputState>,
    draft: Vec<Entity<InputState>>,
    finding: Option<Entity<FindPanel>>,
    totals: HashMap<String, DataValue>,
    totals_error: Option<String>,
    totals_key: String,
    seen: (String, String, u64),
    pending_focus: Option<(u64, CellAt)>,
    _subscriptions: Vec<Subscription>,
    _draft_subscriptions: Vec<Subscription>,
}
impl EventEmitter<SheetEvent> for Datasheet {}

fn key_of(parts: &[String]) -> String {
    parts.join("\u{1}")
}

impl Datasheet {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        shell: Entity<Shell>,
        table: String,
        page: Page,
        page_size: u64,
        read_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let editor = cx.new(|cx| InputState::new(window, cx));
        let store = shell.read(cx).store.clone();
        let subscriptions = vec![
            cx.subscribe_in(&editor, window, |this, _, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => this.finish_edit(window, cx),
                InputEvent::Blur => {
                    if this.editing {
                        this.commit_pending(cx);
                        this.editing = false;
                        cx.notify();
                    }
                }
                _ => {}
            }),
            cx.subscribe(&store, |this, _, e: &StoreEvent, cx| {
                if matches!(e, StoreEvent::Changed) {
                    this.reload_settings(cx);
                    this.load_totals(cx);
                    cx.notify();
                }
            }),
        ];
        let mut this = Self {
            shell,
            table,
            read_only,
            page,
            page_size,
            revision: 0,
            sorts: vec![],
            read_filters: vec![],
            filters: vec![],
            fields: vec![],
            layout: DatasheetLayout::default(),
            active: None,
            editing: false,
            draft_error: None,
            editor,
            draft: vec![],
            finding: None,
            totals: HashMap::new(),
            totals_error: None,
            totals_key: String::new(),
            seen: (String::new(), String::new(), 0),
            pending_focus: None,
            _subscriptions: subscriptions,
            _draft_subscriptions: vec![],
        };
        this.reload_settings(cx);
        this.rebuild_draft(window, cx);
        this.seen = this.view_key();
        this.load_totals(cx);
        this
    }

    fn view_key(&self) -> (String, String, u64) {
        let sorts: Vec<String> = self
            .sorts
            .iter()
            .map(|s| format!("{}{}", s.column, s.descending))
            .collect();
        let filters: Vec<String> = self
            .read_filters
            .iter()
            .map(|f| format!("{}{:?}{:?}", f.column, f.operator, f.value))
            .collect();
        (
            format!("{}\u{2}{}", self.table, key_of(&sorts)),
            key_of(&filters),
            self.page.offset,
        )
    }

    /// Field settings and the saved layout come from the document config.
    fn reload_settings(&mut self, cx: &mut Context<Self>) {
        let shell = self.shell.read(cx);
        let config = shell.config(cx);
        self.fields = self
            .page
            .columns
            .iter()
            .map(|c| crate::forms::fields::field_settings_for(&config.entities, Some(&self.table), Some(&c.name)).cloned())
            .collect();
        self.layout = read_layout(config, &self.table);
    }

    fn rebuild_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft.clear();
        self._draft_subscriptions.clear();
        for column in self.page.columns.iter() {
            let placeholder = if column.default_value.is_some() { "Default" } else { "Enter value" };
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            let subscription = cx.subscribe_in(&input, window, |this, _, e: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = e {
                    this.insert(window, cx);
                }
            });
            self._draft_subscriptions.push(subscription);
            self.draft.push(input);
        }
    }

    /// The workbench pushes a freshly read page and the state it was read with.
    #[allow(clippy::too_many_arguments)]
    pub fn set_page(
        &mut self,
        table: String,
        page: Page,
        revision: u64,
        sorts: Vec<Sort>,
        read_filters: Vec<Filter>,
        filters: Vec<SheetFilter>,
        read_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let columns_changed = self.table != table
            || self.page.columns.len() != page.columns.len()
            || self
                .page
                .columns
                .iter()
                .zip(&page.columns)
                .any(|(a, b)| a.name != b.name);
        self.table = table;
        self.page = page;
        self.revision = revision;
        self.sorts = sorts;
        self.read_filters = read_filters;
        self.filters = filters;
        self.read_only = read_only;
        self.reload_settings(cx);
        if columns_changed {
            self.rebuild_draft(window, cx);
        }
        // A new table, filter, sort or page puts other records under the old cell position.
        let key = self.view_key();
        if key != self.seen {
            self.seen = key;
            self.active = None;
            self.editing = false;
        }
        if let Some((offset, cell)) = self.pending_focus {
            if offset == self.page.offset {
                self.pending_focus = None;
                self.activate(cell, window, cx);
            }
        }
        if self.editing {
            if let Some(cell) = self.active {
                let text = self.shown_at(cell);
                set_text(&self.editor, &text, window, cx);
            }
        }
        self.load_totals(cx);
        cx.notify();
    }

    // ---- Cells -------------------------------------------------------------------

    fn shown(&self, column: usize, value: &DataValue) -> String {
        datasheet_text(value, self.fields.get(column).and_then(Option::as_ref))
            .unwrap_or_else(|| show_value(value))
    }

    fn shown_at(&self, cell: CellAt) -> String {
        self.page
            .rows
            .get(cell.row)
            .and_then(|r| r.get(cell.column))
            .map(|v| self.shown(cell.column, v))
            .unwrap_or_default()
    }

    pub fn editable(&self, column: usize) -> bool {
        !self.read_only
            && self
                .page
                .columns
                .get(column)
                .is_some_and(|c| is_editable(c, self.fields.get(column).and_then(Option::as_ref)))
    }

    pub fn visible(&self) -> Vec<usize> {
        visible_columns(&self.page.columns, &self.layout)
    }

    fn editable_visible(&self) -> Vec<usize> {
        self.visible().into_iter().filter(|j| self.editable(*j)).collect()
    }

    /// Selects a cell; an editable one opens the cell editor.
    pub fn activate(&mut self, cell: CellAt, window: &mut Window, cx: &mut Context<Self>) {
        if self.active == Some(cell) && self.editing {
            return;
        }
        self.commit_pending(cx);
        self.active = Some(cell);
        if self.editable(cell.column) && cell.row < self.page.rows.len() {
            self.editing = true;
            let text = self.shown_at(cell);
            set_text(&self.editor, &text, window, cx);
            self.editor.update(cx, |s, cx| {
                s.focus(window, cx);
                s.select_all(window, cx);
            });
        } else {
            self.editing = false;
        }
        cx.notify();
    }

    /// Commits the cell editor's text when it differs from what the cell shows.
    fn commit_pending(&mut self, cx: &mut Context<Self>) {
        if !self.editing {
            return;
        }
        let Some(cell) = self.active else {
            return;
        };
        let text = self.editor.read(cx).value().to_string();
        if text != self.shown_at(cell) {
            self.commit(cell, text, cx);
        }
    }

    fn finish_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.editing {
            return;
        }
        self.commit_pending(cx);
        self.editing = false;
        window.blur(cx);
        cx.notify();
    }

    fn fail(&mut self, error: &AppError, cx: &mut Context<Self>) {
        cx.emit(SheetEvent::Error(error.message.clone()));
        if error.code == "CONFLICT" {
            cx.emit(SheetEvent::Refresh);
        }
    }

    fn commit(&mut self, cell: CellAt, text: String, cx: &mut Context<Self>) {
        let Some(column) = self.page.columns.get(cell.column).cloned() else {
            return;
        };
        let field = self.fields.get(cell.column).and_then(Option::as_ref);
        let value = match datasheet_value(&text, &column, field) {
            Ok(value) => value,
            Err(message) => {
                self.fail(&AppError::new("VALIDATION_ERROR", message), cx);
                return;
            }
        };
        let label = self.shell.read(cx).label().to_string();
        let table = self.table.clone();
        let identity = self.page.identities[cell.row].clone();
        let meta = RecordWriteMeta {
            expected: Some(original_values(&self.page.columns, &self.page.rows[cell.row])),
            ..Default::default()
        };
        let values = vec![NamedValue {
            column: column.name,
            value,
        }];
        task::spawn_call(
            cx,
            move || records::update(&label, &table, values, identity, meta),
            |this, result, cx| match result {
                Ok(_) => {
                    cx.emit(SheetEvent::Error(String::new()));
                    cx.emit(SheetEvent::Written);
                }
                Err(e) => {
                    if e.committed {
                        cx.emit(SheetEvent::Written);
                    }
                    this.fail(&e.error, cx);
                }
            },
        );
    }

    // ---- Records -----------------------------------------------------------------

    fn remove(&mut self, row: usize, cx: &mut Context<Self>) {
        let ask = dialogs::confirm(cx, "Delete record", "Delete this record?");
        task::after(cx, ask, move |this, yes, cx| {
            if !yes || row >= this.page.rows.len() {
                return;
            }
            let label = this.shell.read(cx).label().to_string();
            let table = this.table.clone();
            let identity = this.page.identities[row].clone();
            let meta = RecordWriteMeta {
                expected: Some(original_values(&this.page.columns, &this.page.rows[row])),
                ..Default::default()
            };
            let last_on_page = this.page.rows.len() == 1 && this.page.offset > 0;
            let (offset, size) = (this.page.offset, this.page_size);
            task::spawn_call(
                cx,
                move || records::delete(&label, &table, identity, meta),
                move |_, result, cx| match result {
                    Ok(_) => {
                        if last_on_page {
                            cx.emit(SheetEvent::Offset(offset.saturating_sub(size)));
                        }
                        cx.emit(SheetEvent::Written);
                    }
                    Err(e) => cx.emit(SheetEvent::Error(e.error.message)),
                },
            );
        });
    }

    fn draft_cells(&self, cx: &App) -> BTreeMap<usize, String> {
        self.draft
            .iter()
            .enumerate()
            .map(|(j, input)| (j, input.read(cx).value().to_string()))
            .filter(|(_, text)| !text.is_empty())
            .collect()
    }

    fn clear_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for input in &self.draft {
            set_text(input, "", window, cx);
        }
    }

    fn insert(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft_error = None;
        let cells = self.draft_cells(cx);
        let values = match typed_values(&self.page.columns, &cells, false, &self.fields) {
            Ok(values) => values,
            Err(message) => {
                self.draft_error = Some(message);
                cx.notify();
                return;
            }
        };
        let label = self.shell.read(cx).label().to_string();
        let table = self.table.clone();
        task::spawn_call_in(window, cx, move || records::insert(&label, &table, values), |this, result, window, cx| {
            match result {
                Ok(_) => {
                    this.clear_draft(window, cx);
                    cx.emit(SheetEvent::Written);
                }
                Err(e) => {
                    if e.committed {
                        this.clear_draft(window, cx);
                        cx.emit(SheetEvent::Written);
                    }
                    this.draft_error = Some(e.error.message);
                }
            }
            cx.notify();
        });
    }

    /// Pastes a block copied from a spreadsheet, starting at `start`, as one batch.
    /// Returns false when the text is a single value the input should take itself.
    pub fn paste(&mut self, text: &str, start: CellAt, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !is_multi_cell(text) {
            return false;
        }
        let plan = plan_paste(
            &parse_clipboard_grid(text),
            &self.editable_visible(),
            (start.row, start.column),
            self.page.rows.len(),
        );
        let last_page = self.page.offset + self.page.rows.len() as u64 >= self.page.total;
        if !plan.inserts.is_empty() && start.row < self.page.rows.len() && !last_page {
            let text = "The pasted rows run past this page. Paste into the new record row to add records, or start higher up.";
            self.shell.update(cx, |shell, cx| shell.toast(crate::ui::toast::Toast::info(text), cx));
            cx.notify();
            return true;
        }
        let writes = match paste_writes(&self.table, &self.page, &plan, &self.fields) {
            Ok(writes) => writes,
            Err(message) => {
                cx.emit(SheetEvent::Error(format!("{message} Nothing was pasted.")));
                cx.notify();
                return true;
            }
        };
        if writes.is_empty() {
            return true;
        }
        let label = self.shell.read(cx).label().to_string();
        let (updated, added) = (plan.updates.len(), plan.inserts.len());
        task::spawn_call_in(window, cx, move || records::write_batch(&label, writes), move |this, result, window, cx| {
            match result {
                Ok(_) => {
                    this.clear_draft(window, cx);
                    let text = pasted_message(updated, added);
                    this.shell.update(cx, |shell, cx| shell.toast(crate::ui::toast::Toast::success(text), cx));
                    cx.emit(SheetEvent::Error(String::new()));
                    cx.emit(SheetEvent::Written);
                }
                Err(e) => {
                    cx.emit(SheetEvent::Error(format!("{} Nothing was pasted.", e.error.message)));
                }
            }
            cx.notify();
        });
        true
    }

    // ---- Filters, layout, find ---------------------------------------------------

    fn filter_selection(&mut self, exclude: bool, cx: &mut Context<Self>) {
        let Some(cell) = self.active else {
            return;
        };
        let Some(value) = self.page.rows.get(cell.row).and_then(|r| r.get(cell.column)) else {
            return;
        };
        let name = &self.page.columns[cell.column].name;
        if let Some(filter) = selection_filter(name, value, exclude) {
            cx.emit(SheetEvent::Filters(add_filter(&self.filters, filter)));
        }
    }

    pub fn can_filter(&self) -> bool {
        self.active
            .and_then(|c| self.page.rows.get(c.row).and_then(|r| r.get(c.column)))
            .is_some_and(|v| !matches!(v, DataValue::Blob(_)))
    }

    fn save_layout(&mut self, next: DatasheetLayout, label: &str, cx: &mut Context<Self>) {
        let table = self.table.clone();
        let store = self.shell.read(cx).store.clone();
        let result = store.update(cx, |s, cx| s.update(label, |c| write_layout(c, &table, &next), cx));
        if let Err(e) = result {
            cx.emit(SheetEvent::Error(e.message));
        }
    }

    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.finding.is_some() {
            return;
        }
        let sheet = cx.entity();
        let panel = cx.new(|cx| FindPanel::new(sheet, window, cx));
        let subscription = cx.subscribe_in(&panel, window, |this, _, e: &FindEvent, window, cx| match e {
            FindEvent::Found { offset, cell } => {
                this.pending_focus = Some((*offset, *cell));
                if *offset != this.page.offset {
                    cx.emit(SheetEvent::Offset(*offset));
                } else {
                    this.pending_focus = None;
                    this.activate(*cell, window, cx);
                }
            }
            FindEvent::Written => cx.emit(SheetEvent::Written),
            FindEvent::Close => {
                this.finding = None;
                cx.notify();
            }
        });
        self._subscriptions.push(subscription);
        self.finding = Some(panel);
        cx.notify();
    }

    pub fn is_finding(&self) -> bool {
        self.finding.is_some()
    }

    // ---- Totals ------------------------------------------------------------------

    fn load_totals(&mut self, cx: &mut Context<Self>) {
        let Some(totals) = self.layout.totals.clone() else {
            self.totals.clear();
            self.totals_key.clear();
            return;
        };
        let specs: Vec<(String, TotalFunction)> = totals
            .into_iter()
            .filter(|(column, _)| self.page.columns.iter().any(|c| &c.name == column))
            .collect();
        let key = format!(
            "{}|{:?}|{:?}|{}",
            self.table,
            specs.iter().map(|(c, f)| format!("{c}{}", total_key(*f))).collect::<Vec<_>>(),
            self.read_filters.iter().map(|f| format!("{}{:?}{:?}", f.column, f.operator, f.value)).collect::<Vec<_>>(),
            self.revision
        );
        if key == self.totals_key {
            return;
        }
        self.totals_key = key.clone();
        let label = self.shell.read(cx).label().to_string();
        let (table, filters) = (self.table.clone(), self.read_filters.clone());
        let names: Vec<String> = specs.iter().map(|(c, _)| c.clone()).collect();
        let request: Vec<TotalSpec> = specs
            .into_iter()
            .map(|(column, function)| TotalSpec { column, function })
            .collect();
        task::spawn_call(
            cx,
            move || crate::read_table_totals(label, table, filters, request),
            move |this, result, cx| {
                if this.totals_key != key {
                    return;
                }
                match result {
                    Ok(values) => {
                        this.totals_error = None;
                        this.totals = names.into_iter().zip(values).collect();
                    }
                    Err(e) => this.totals_error = Some(e.message),
                }
                cx.notify();
            },
        );
    }

    fn change_total(&mut self, column: &str, key: &str, cx: &mut Context<Self>) {
        let mut totals = self.layout.totals.clone().unwrap_or_default();
        match total_from_key(key) {
            Some(function) => {
                totals.insert(column.to_string(), function);
            }
            None => {
                totals.remove(column);
            }
        }
        let next = DatasheetLayout {
            totals: Some(totals),
            ..self.layout.clone()
        };
        self.save_layout(next, "Change total", cx);
    }
}

fn set_text(input: &Entity<InputState>, text: &str, window: &mut Window, cx: &mut App) {
    let text = text.to_string();
    input.update(cx, |s, cx| s.set_value(text, window, cx));
}

// ---- Rendering -------------------------------------------------------------------

impl Datasheet {
    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let can_filter = self.can_filter();
        let hidden: Vec<String> = self
            .layout
            .hidden
            .iter()
            .filter(|n| self.page.columns.iter().any(|c| &c.name == *n))
            .cloned()
            .collect();
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        h_flex()
            .id("sheet-toolbar")
            .flex_wrap()
            .gap_2()
            .items_center()
            .px_3()
            .py_1()
            .child({
                let entity = entity.clone();
                Button::new("filter")
                    .outline()
                    .small()
                    .label("Filter")
                    .tooltip("Filter by the selected value")
                    .disabled(!can_filter)
                    .dropdown_menu(move |menu, _, _| {
                        let (by, excluding) = (entity.clone(), entity.clone());
                        menu.item(PopupMenuItem::new("Filter by selection").on_click(move |_, _, cx| {
                            by.update(cx, |this, cx| this.filter_selection(false, cx))
                        }))
                        .item(PopupMenuItem::new("Filter excluding selection").on_click(move |_, _, cx| {
                            excluding.update(cx, |this, cx| this.filter_selection(true, cx))
                        }))
                    })
            })
            .child(
                Button::new("find")
                    .outline()
                    .small()
                    .icon(IconName::Search)
                    .label("Find")
                    .tooltip("Find and replace (Ctrl+F)")
                    .on_click(cx.listener(|this, _, window, cx| this.open_find(window, cx))),
            )
            .child(
                Button::new("totals")
                    .outline()
                    .small()
                    .label("Totals")
                    .selected(self.layout.totals.is_some())
                    .on_click(cx.listener(|this, _, _, cx| {
                        let next = DatasheetLayout {
                            totals: if this.layout.totals.is_some() { None } else { Some(BTreeMap::new()) },
                            ..this.layout.clone()
                        };
                        let label = if this.layout.totals.is_some() { "Hide totals row" } else { "Show totals row" };
                        this.save_layout(next, label, cx);
                    })),
            )
            .when(!hidden.is_empty(), |el| {
                let entity = entity.clone();
                let names = hidden.clone();
                el.child(
                    Button::new("hidden-columns")
                        .outline()
                        .small()
                        .icon(IconName::EyeOff)
                        .label(format!("{} hidden", hidden.len()))
                        .dropdown_menu(move |menu, _, _| {
                            let mut menu = menu;
                            for name in names.clone() {
                                let entity = entity.clone();
                                let shown = name.clone();
                                menu = menu.item(PopupMenuItem::new(format!("Show {name}")).on_click(
                                    move |_, _, cx| {
                                        let shown = shown.clone();
                                        entity.update(cx, |this, cx| this.show_columns(&[shown], cx))
                                    },
                                ));
                            }
                            let entity = entity.clone();
                            let all = names.clone();
                            menu.item(PopupMenuItem::new("Show all columns").on_click(move |_, _, cx| {
                                let all = all.clone();
                                entity.update(cx, |this, cx| this.show_columns(&all, cx))
                            }))
                        }),
                )
            })
            .when(self.layout.frozen > 0, |el| {
                el.child(
                    Button::new("unfreeze-columns")
                        .outline()
                        .small()
                        .label("Unfreeze columns")
                        .on_click(cx.listener(|this, _, _, cx| {
                            let next = DatasheetLayout { frozen: 0, ..this.layout.clone() };
                            this.save_layout(next, "Unfreeze columns", cx);
                        })),
                )
            })
            .when(!self.filters.is_empty(), |el| {
                el.child(
                    h_flex()
                        .id("active-filters")
                        .gap_2()
                        .items_center()
                        .children(self.filters.iter().enumerate().map(|(i, filter)| {
                            let label = describe_filter(filter);
                            h_flex()
                                .gap_1()
                                .items_center()
                                .px_2()
                                .rounded_md()
                                .bg(theme.muted)
                                .child(widgets::styled_text(
                                    SharedString::from(format!("filter-chip:{i}")),
                                    label.clone(),
                                    |d| d.text_sm(),
                                ))
                                .child(
                                    Button::new(SharedString::from(format!("remove-filter:{i}")))
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Close)
                                        .tooltip(format!("Remove filter {label}"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let mut next = this.filters.clone();
                                            next.remove(i);
                                            cx.emit(SheetEvent::Filters(next));
                                        })),
                                )
                        }))
                        .child(
                            Button::new("clear-filters")
                                .ghost()
                                .small()
                                .label("Clear filters")
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(SheetEvent::Filters(vec![])))),
                        ),
                )
            })
            .child(div().flex_1())
            .child(div().text_xs().text_color(muted).child(format!("{} columns", self.visible().len())))
    }

    fn show_columns(&mut self, names: &[String], cx: &mut Context<Self>) {
        let next = DatasheetLayout {
            hidden: self.layout.hidden.iter().filter(|n| !names.contains(n)).cloned().collect(),
            ..self.layout.clone()
        };
        self.save_layout(next, "Show columns", cx);
    }

    fn header_cell(&self, j: usize, frozen: bool, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity();
        let column = &self.page.columns[j];
        let name = column.name.clone();
        let at = self.sorts.iter().position(|s| s.column == name);
        let arrow = match at {
            Some(i) if self.sorts[i].descending => " ↓",
            Some(_) => " ↑",
            None => "",
        };
        let sup = match at {
            Some(i) if self.sorts.len() > 1 => format!(" {}", i + 1),
            _ => String::new(),
        };
        let label = format!("{name}{arrow}{sup}");
        let can_hide = self.visible().len() > 1;
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let menu_entity = entity.clone();
        let menu_name = name.clone();
        let sort_name = name.clone();
        v_flex()
            .id(SharedString::from(format!("head:{name}")))
            .aria_label(format!("{name}{}", if frozen { " (frozen)" } else { "" }))
            .w(px(COLUMN_WIDTH))
            .h(px(HEAD_HEIGHT))
            .flex_none()
            .px_2()
            .justify_center()
            .border_b_1()
            .border_r_1()
            .border_color(theme.border)
            .bg(theme.secondary)
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        Button::new(SharedString::from(format!("sort:{name}")))
                            .ghost()
                            .xsmall()
                            .label(label)
                            .tooltip("Click to sort, Shift-click to add to the sort")
                            .on_click({
                                let entity = entity.clone();
                                move |event, _, cx| {
                                    let additive = event.modifiers().shift;
                                    let name = sort_name.clone();
                                    entity.update(cx, |this, cx| {
                                        let next = next_sorts(&this.sorts, &name, additive);
                                        cx.emit(SheetEvent::Sorts(next));
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new(SharedString::from(format!("Column options for {name}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::EllipsisVertical)
                            .dropdown_menu(move |menu, _, _| {
                                let (e1, e2, e3, e4) = (
                                    menu_entity.clone(),
                                    menu_entity.clone(),
                                    menu_entity.clone(),
                                    menu_entity.clone(),
                                );
                                let (n1, n2, n3, n4) =
                                    (menu_name.clone(), menu_name.clone(), menu_name.clone(), menu_name.clone());
                                menu.item(PopupMenuItem::new("Sort ascending").on_click(move |_, _, cx| {
                                    let n = n1.clone();
                                    e1.update(cx, |_, cx| {
                                        cx.emit(SheetEvent::Sorts(vec![Sort { column: n, descending: false }]))
                                    })
                                }))
                                .item(PopupMenuItem::new("Sort descending").on_click(move |_, _, cx| {
                                    let n = n2.clone();
                                    e2.update(cx, |_, cx| {
                                        cx.emit(SheetEvent::Sorts(vec![Sort { column: n, descending: true }]))
                                    })
                                }))
                                .item(
                                    PopupMenuItem::new("Hide column")
                                        .disabled(!can_hide)
                                        .on_click(move |_, _, cx| {
                                            let n = n3.clone();
                                            e3.update(cx, |this, cx| {
                                                let next = hide_column(&this.layout, &this.page.columns, &n);
                                                this.save_layout(next, "Hide column", cx);
                                            })
                                        }),
                                )
                                .item(
                                    PopupMenuItem::new(if frozen {
                                        "Unfreeze columns"
                                    } else {
                                        "Freeze through this column"
                                    })
                                    .on_click(move |_, _, cx| {
                                        let n = n4.clone();
                                        e4.update(cx, |this, cx| {
                                            let (next, label) = if frozen {
                                                (DatasheetLayout { frozen: 0, ..this.layout.clone() }, "Unfreeze columns")
                                            } else {
                                                (freeze_through(&this.layout, &this.page.columns, &n), "Freeze columns")
                                            };
                                            this.save_layout(next, label, cx);
                                        })
                                    }),
                                )
                            }),
                    ),
            )
            .child(div().text_xs().text_color(muted).child(logical_of(column)))
            .test_support().into_any_element()
    }

    fn body_cell(&self, i: usize, j: usize, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity();
        let theme = cx.theme();
        let value = &self.page.rows[i][j];
        let text = self.shown(j, value);
        let cell = CellAt { row: i, column: j };
        let is_active = self.active == Some(cell);
        let primary = self.page.columns[j].primary_key_position > 0;
        let editing_here = is_active && self.editing;
        let base = div()
            .id(SharedString::from(format!("cell:{i}:{j}")))
            .aria_label(text.clone())
            .w(px(COLUMN_WIDTH))
            .h(px(ROW_HEIGHT))
            .flex_none()
            .px_2()
            .flex()
            .items_center()
            .border_b_1()
            .border_r_1()
            .border_color(theme.border)
            .when(primary, |el| el.font_weight(FontWeight::SEMIBOLD))
            .when(is_active, |el| el.bg(theme.accent))
            .on_click(move |_, window, cx| {
                entity.update(cx, |this, cx| this.activate(cell, window, cx));
            });
        let sheet = cx.entity();
        if editing_here {
            base.child(
                Input::new(&self.editor)
                    .id("cell-input")
                    .aria_label(format!("{}, row {}", self.page.columns[j].name, self.page.offset as usize + i + 1))
                    .appearance(false)
                    .on_paste(move |item, window, cx| {
                        let Some(text) = item.text() else { return false };
                        sheet.update(cx, |this, cx| this.paste(&text, cell, window, cx))
                    }),
            )
            .test_support()
            .into_any_element()
        } else {
            base.child(div().truncate().child(text)).test_support().into_any_element()
        }
    }

    fn draft_cell(&self, j: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let column = &self.page.columns[j];
        let field = self.fields.get(j).and_then(Option::as_ref);
        let cell = div()
            .w(px(COLUMN_WIDTH))
            .h(px(ROW_HEIGHT))
            .flex_none()
            .px_1()
            .flex()
            .items_center()
            .border_b_1()
            .border_r_1()
            .border_color(theme.border);
        if column.generated || field.is_some_and(|f| f.format.as_deref().is_some_and(|x| !x.is_empty())) {
            return cell
                .text_color(theme.muted_foreground)
                .child(if column.generated { "Generated" } else { "Use a form" })
                .into_any_element();
        }
        let sheet = cx.entity();
        let row = self.page.rows.len();
        cell.child(
            Input::new(&self.draft[j])
                .id(SharedString::from(format!("New {}", column.name)))
                .aria_label(format!("New {}", column.name))
                .appearance(false)
                .on_paste(move |item, window, cx| {
                    let Some(text) = item.text() else { return false };
                    sheet.update(cx, |this, cx| this.paste(&text, CellAt { row, column: j }, window, cx))
                }),
        )
        .into_any_element()
    }

    fn total_cell(&self, j: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let column = &self.page.columns[j];
        let name = column.name.clone();
        let chosen = self.layout.totals.as_ref().and_then(|t| t.get(&name)).copied();
        let mut options = vec![(String::new(), "None".to_string())];
        options.extend(totals_for(column).into_iter().map(|f| (total_key(f).to_string(), total_label(f).to_string())));
        let entity = cx.entity();
        let pick_name = name.clone();
        let value = self.totals.get(&name).filter(|_| chosen.is_some()).map(show_total);
        h_flex()
            .w(px(COLUMN_WIDTH))
            .h(px(TOTAL_HEIGHT))
            .flex_none()
            .px_1()
            .gap_1()
            .items_center()
            .border_r_1()
            .border_color(theme.border)
            .child(select(
                SharedString::from(format!("Total for {name}")),
                chosen.map_or("", total_key),
                options,
                "None",
                move |key, _, cx| {
                    let (name, key) = (pick_name.clone(), key.to_string());
                    entity.update(cx, |this, cx| this.change_total(&name, &key, cx));
                },
            ))
            .when_some(value, |el, text| {
                el.child(widgets::styled_text(SharedString::from(format!("total:{name}")), text, |d| d.text_sm()))
            })
            .into_any_element()
    }

    /// One vertical block of the grid: a row-number column or a run of data columns.
    fn block(&self, columns: &[usize], frozen: bool, actions: bool, cx: &mut Context<Self>) -> Div {
        let (border, secondary) = (cx.theme().border, cx.theme().secondary);
        let rows = self.page.rows.len();
        let totals = self.layout.totals.is_some();
        let entity = cx.entity();
        let mut block = v_flex().flex_none();
        // Header
        let mut head = h_flex().flex_none();
        for &j in columns {
            head = head.child(self.header_cell(j, frozen, cx));
        }
        if actions {
            head = head.child(
                div()
                    .w(px(ACTIONS_WIDTH))
                    .h(px(HEAD_HEIGHT))
                    .border_b_1()
                    .border_color(border)
                    .bg(secondary),
            );
        }
        block = block.child(head);
        // Records
        for i in 0..rows {
            let mut row = h_flex().flex_none();
            for &j in columns {
                row = row.child(self.body_cell(i, j, cx));
            }
            if actions {
                let entity = entity.clone();
                row = row.child(
                    div().w(px(ACTIONS_WIDTH)).h(px(ROW_HEIGHT)).px_1().flex().items_center().child(
                        Button::new(SharedString::from(format!("delete-row:{i}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Trash)
                            .tooltip(format!("Delete record {}", self.page.offset as usize + i + 1))
                            .on_click(move |_, _, cx| entity.update(cx, |this, cx| this.remove(i, cx))),
                    ),
                );
            }
            block = block.child(row);
        }
        // New-record row
        if !self.read_only {
            let mut row = h_flex().flex_none();
            for &j in columns {
                row = row.child(self.draft_cell(j, cx));
            }
            if actions {
                row = row.child(
                    div().w(px(ACTIONS_WIDTH)).h(px(ROW_HEIGHT)).px_1().flex().items_center().child(
                        Button::new("insert-row")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip("Insert row")
                            .on_click(cx.listener(|this, _, window, cx| this.insert(window, cx))),
                    ),
                );
            }
            block = block.child(row);
        }
        // Totals
        if totals {
            let mut row = h_flex().flex_none().border_t_1().border_color(border);
            for &j in columns {
                row = row.child(self.total_cell(j, cx));
            }
            if actions {
                row = row.child(div().w(px(ACTIONS_WIDTH)));
            }
            block = block.child(row);
        }
        block
    }

    /// The row-number block: header, record numbers, the new-record marker and Total.
    fn rownums(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let cell = |content: String| {
            div()
                .w(px(ROWNUM_WIDTH))
                .h(px(ROW_HEIGHT))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .border_b_1()
                .border_r_1()
                .border_color(theme.border)
                .text_color(theme.muted_foreground)
                .child(content)
        };
        let mut block = v_flex().flex_none().child(
            div()
                .w(px(ROWNUM_WIDTH))
                .h(px(HEAD_HEIGHT))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .border_b_1()
                .border_r_1()
                .border_color(theme.border)
                .bg(theme.secondary)
                .child("#"),
        );
        for i in 0..self.page.rows.len() {
            block = block.child(cell(format!("{}", self.page.offset as usize + i + 1)));
        }
        if !self.read_only {
            block = block.child(cell("+".into()));
        }
        if self.layout.totals.is_some() {
            block = block.child(
                div()
                    .w(px(ROWNUM_WIDTH))
                    .h(px(TOTAL_HEIGHT))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_t_1()
                    .border_color(theme.border)
                    .child("Total"),
            );
        }
        block
    }
}

fn total_label(function: TotalFunction) -> &'static str {
    match function {
        TotalFunction::Sum => "Sum",
        TotalFunction::Avg => "Average",
        TotalFunction::Count => "Count",
        TotalFunction::Min => "Minimum",
        TotalFunction::Max => "Maximum",
        TotalFunction::Stdev => "Std dev",
        TotalFunction::Var => "Variance",
    }
}

fn show_total(value: &DataValue) -> String {
    match value {
        DataValue::Real(v) => {
            let rounded = (v * 10_000.0).round() / 10_000.0;
            let text = format!("{rounded}");
            let (whole, fraction) = text.split_once('.').map_or((text.as_str(), None), |(a, b)| (a, Some(b)));
            let negative = whole.starts_with('-');
            let digits: Vec<char> = whole.trim_start_matches('-').chars().collect();
            let mut grouped = String::new();
            for (i, c) in digits.iter().enumerate() {
                if i > 0 && (digits.len() - i) % 3 == 0 {
                    grouped.push(',');
                }
                grouped.push(*c);
            }
            format!(
                "{}{}{}",
                if negative { "-" } else { "" },
                grouped,
                fraction.map_or(String::new(), |f| format!(".{f}"))
            )
        }
        other => show_value(other),
    }
}

impl Render for Datasheet {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let visible = self.visible();
        let frozen: Vec<usize> = visible.iter().take(self.layout.frozen).copied().collect();
        let rest: Vec<usize> = visible.iter().skip(self.layout.frozen).copied().collect();
        let left_columns = frozen.clone();
        let left = h_flex()
            .flex_none()
            .child(self.rownums(cx))
            .child(self.block(&left_columns, true, false, cx));
        let right = self.block(&rest, false, !self.read_only, cx);
        let empty = self.page.rows.is_empty();
        let read_only = self.read_only;
        v_flex()
            .size_full()
            .min_h_0()
            .child(self.toolbar(cx))
            .when_some(self.finding.clone(), |el, panel| el.child(panel))
            .when_some(self.draft_error.clone(), |el, message| {
                el.child(div().px_3().py_1().child(widgets::error_callout("draft-error", "This record can't be added as it is.", &AppError::new("VALIDATION_ERROR", message), None, cx)))
            })
            .child(
                div()
                    .id("sheet-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        h_flex()
                            .items_start()
                            .child(left)
                            .child(div().id("sheet-columns").flex_1().min_w_0().overflow_x_scroll().child(right)),
                    )
                    .when(empty, |el| {
                        el.child(widgets::empty_state(
                            "sheet-empty",
                            "No records yet",
                            if read_only {
                                "This view returned no records."
                            } else {
                                "Use the new row above to add the first record."
                            },
                            cx,
                        ))
                    }),
            )
    }
}
