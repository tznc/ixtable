//! Find and replace panel above the datasheet (port of `src/data/sheet/FindReplace.tsx`).
//! Find next walks the whole filtered, sorted table page by page (wrapping once) and
//! moves the datasheet to the match; Replace all rewrites every match in one batch.
//!
//! Test ids: `find-text`, `replace-text`, `find-scope` (options "All columns" and
//! "Current column"), `find-whole`, `find-case`, `find-next`, `find-replace`,
//! `find-replace-all`, `find-close`, `find-status`.

use crate::ui::controls::{check, input_text, new_input, on_change, select, text_input};
use super::{
    find::{cell_matches, cell_text, next_match, replace_text, CellAt, FindOptions},
    grid::Datasheet,
    rows::{original_values, SheetFields},
};
use crate::data::{DataValue, Filter, NamedValue, Page, Sort};
use crate::manager::AppError;
use crate::records::{self, RecordWrite, RecordWriteMeta};
use crate::ui::{
    dialogs,
    fields::datasheet::datasheet_value,
    prelude::*,
};
use gpui_kit::component::input::{InputEvent, InputState};

/// Largest page the backend serves; replace-all reads the table in pages of this size.
const SCAN_PAGE: u64 = 1000;

pub enum FindEvent {
    Found { offset: u64, cell: CellAt },
    Written,
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    All,
    Column,
}

/// What the datasheet looked like when the user clicked, so background scans need no
/// access to the view.
#[derive(Clone)]
struct Snapshot {
    label: String,
    table: String,
    page: Page,
    page_size: u64,
    sorts: Vec<Sort>,
    filters: Vec<Filter>,
    fields: SheetFields,
    columns: Vec<usize>,
    editable: Vec<usize>,
    active: Option<CellAt>,
}

pub struct FindPanel {
    sheet: Entity<Datasheet>,
    find: Entity<InputState>,
    replacement: Entity<InputState>,
    scope: Scope,
    match_case: bool,
    whole_field: bool,
    status: String,
    busy: bool,
    _subscriptions: Vec<Subscription>,
}
impl EventEmitter<FindEvent> for FindPanel {}

impl FindPanel {
    pub fn new(sheet: Entity<Datasheet>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let find = new_input(window, cx, "", "");
        let replacement = new_input(window, cx, "", "");
        let subscriptions = vec![
            on_change(cx, &find, |_, cx| cx.notify()),
            cx.subscribe(&find, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::PressEnter { .. }) && !this.options(cx).find.is_empty() {
                    this.find_next(cx);
                }
            }),
        ];
        find.update(cx, |s, cx| s.focus(window, cx));
        Self {
            sheet,
            find,
            replacement,
            scope: Scope::All,
            match_case: false,
            whole_field: false,
            status: String::new(),
            busy: false,
            _subscriptions: subscriptions,
        }
    }

    fn options(&self, cx: &App) -> FindOptions {
        FindOptions {
            find: input_text(&self.find, cx),
            match_case: self.match_case,
            whole_field: self.whole_field,
        }
    }

    fn snapshot(&self, cx: &App) -> Snapshot {
        let sheet = self.sheet.read(cx);
        let columns = match (self.scope, sheet.active) {
            (Scope::Column, Some(active)) => vec![active.column],
            _ => sheet.visible(),
        };
        let editable = columns.iter().copied().filter(|j| sheet.editable(*j)).collect();
        Snapshot {
            label: sheet.shell.read(cx).label().to_string(),
            table: sheet.table.clone(),
            page: sheet.page.clone(),
            page_size: sheet.page_size,
            sorts: sheet.sorts.clone(),
            filters: sheet.read_filters.clone(),
            fields: sheet.fields.clone(),
            columns,
            editable,
            active: sheet.active,
        }
    }

    fn read_only(&self, cx: &App) -> bool {
        self.sheet.read(cx).read_only
    }

    fn begin(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.status.clear();
        cx.notify();
    }

    fn end(&mut self, error: Option<AppError>, cx: &mut Context<Self>) {
        self.busy = false;
        if let Some(e) = error {
            self.status = e.message;
        }
        cx.notify();
    }

    // ---- Find next ----------------------------------------------------------------

    fn find_next(&mut self, cx: &mut Context<Self>) {
        self.begin(cx);
        self.locate(cx);
    }

    /// Finds the next match on this page, else scans the other pages (wrapping once).
    fn locate(&mut self, cx: &mut Context<Self>) {
        let options = self.options(cx);
        let snap = self.snapshot(cx);
        if let Some(here) = next_match(&snap.page.rows, &snap.columns, &options, snap.active, &snap.fields) {
            let offset = snap.page.offset;
            cx.emit(FindEvent::Found { offset, cell: here });
            self.end(None, cx);
            return;
        }
        task::spawn_call(
            cx,
            move || scan_for_match(&snap, &options),
            move |this, result, cx| match result {
                Ok(Some((offset, cell))) => {
                    cx.emit(FindEvent::Found { offset, cell });
                    this.end(None, cx);
                }
                Ok(None) => {
                    this.status = format!("No matches for \u{201c}{}\u{201d}.", input_text(&this.find, cx));
                    this.end(None, cx);
                }
                Err(e) => this.end(Some(e), cx),
            },
        );
    }

    // ---- Replace ------------------------------------------------------------------

    fn replace(&mut self, cx: &mut Context<Self>) {
        self.begin(cx);
        let options = self.options(cx);
        let replacement = input_text(&self.replacement, cx);
        let snap = self.snapshot(cx);
        let target = snap.active.and_then(|active| {
            let value = snap.page.rows.get(active.row)?.get(active.column)?;
            let field = snap.fields.get(active.column).and_then(Option::as_ref);
            (snap.editable.contains(&active.column) && cell_matches(value, &options, field))
                .then(|| (active, value.clone()))
        });
        let Some((active, value)) = target else {
            self.locate(cx);
            return;
        };
        let write = match replacement_write(&snap, &options, &replacement, active.row, active.column, &value) {
            Ok(write) => write,
            Err(message) => {
                self.end(Some(AppError::new("VALIDATION_ERROR", message)), cx);
                return;
            }
        };
        let label = snap.label.clone();
        task::spawn_call(
            cx,
            move || records::update(&label, &write.table, write.values, write.identity.unwrap_or_default(), write.meta),
            move |this, result, cx| match result {
                Ok(_) => {
                    cx.emit(FindEvent::Written);
                    if let Some(next) =
                        next_match(&snap.page.rows, &snap.columns, &options, Some(active), &snap.fields)
                    {
                        cx.emit(FindEvent::Found { offset: snap.page.offset, cell: next });
                    }
                    this.end(None, cx);
                }
                Err(e) => {
                    if e.committed {
                        cx.emit(FindEvent::Written);
                    }
                    this.end(Some(e.error), cx);
                }
            },
        );
    }

    fn replace_all(&mut self, cx: &mut Context<Self>) {
        self.begin(cx);
        let options = self.options(cx);
        let replacement = input_text(&self.replacement, cx);
        let snap = self.snapshot(cx);
        let total = snap.page.total;
        let find_text = options.find.clone();
        task::spawn_call(
            cx,
            move || collect_replacements(&snap, &options, &replacement, total),
            move |this, result, cx| {
                let (writes, cells) = match result {
                    Ok(found) => found,
                    Err(e) => return this.end(Some(e), cx),
                };
                if writes.is_empty() {
                    this.status = format!("No matches for \u{201c}{find_text}\u{201d}.");
                    return this.end(None, cx);
                }
                let records_text = if writes.len() == 1 {
                    "1 record".to_string()
                } else {
                    format!("{} records", writes.len())
                };
                let message = format!("Replace {cells} matches in {records_text}? This cannot be undone.");
                let label = this.sheet.read(cx).shell.read(cx).label().to_string();
                let ask = dialogs::confirm(cx, "Replace all", &message);
                task::after(cx, ask, move |this, yes, cx| {
                    if !yes {
                        return this.end(None, cx);
                    }
                    task::spawn_call(
                        cx,
                        move || records::write_batch(&label, writes),
                        move |this, result, cx| match result {
                            Ok(_) => {
                                cx.emit(FindEvent::Written);
                                this.status = format!("Replaced {cells} matches in {records_text}.");
                                this.end(None, cx);
                            }
                            Err(e) => this.end(Some(e.error), cx),
                        },
                    );
                });
            },
        );
    }

    #[cfg(test)]
    pub fn status(&self) -> &str {
        &self.status
    }
}

/// The record write that replaces the matches in one cell.
fn replacement_write(
    snap: &Snapshot,
    options: &FindOptions,
    replacement: &str,
    row: usize,
    column: usize,
    value: &DataValue,
) -> Result<RecordWrite, String> {
    let meta = &snap.page.columns[column];
    let field = snap.fields.get(column).and_then(Option::as_ref);
    let text = cell_text(value, field).unwrap_or_default();
    let next = datasheet_value(&replace_text(&text, replacement, options), meta, field)?;
    let expected = original_values(&snap.page.columns, &snap.page.rows[row]);
    Ok(RecordWrite::update(
        &snap.table,
        vec![NamedValue { column: meta.name.clone(), value: next }],
        snap.page.identities[row].clone(),
    )
    .with_meta(RecordWriteMeta { expected: Some(expected), ..Default::default() }))
}

fn read_rows(snap: &Snapshot, offset: u64, limit: u64) -> Result<Page, AppError> {
    crate::read_table_page(
        snap.label.clone(),
        snap.table.clone(),
        offset,
        limit,
        snap.sorts.clone(),
        snap.filters.clone(),
        None,
    )
}

/// Later pages, then around to the first page through the current one.
fn scan_for_match(snap: &Snapshot, options: &FindOptions) -> Result<Option<(u64, CellAt)>, AppError> {
    let size = snap.page_size.max(1);
    let pages = snap.page.total.div_ceil(size);
    let current = snap.page.offset / size;
    for k in 1..=pages {
        let offset = ((current + k) % pages) * size;
        let hit = if offset == snap.page.offset {
            next_match(&snap.page.rows, &snap.columns, options, None, &snap.fields)
        } else {
            let page = read_rows(snap, offset, size)?;
            next_match(&page.rows, &snap.columns, options, None, &snap.fields)
        };
        if let Some(cell) = hit {
            return Ok(Some((offset, cell)));
        }
    }
    Ok(None)
}

/// Every record with a matching editable cell, as update writes, and the cell count.
fn collect_replacements(
    snap: &Snapshot,
    options: &FindOptions,
    replacement: &str,
    total: u64,
) -> Result<(Vec<RecordWrite>, usize), AppError> {
    let mut writes = Vec::new();
    let mut cells = 0;
    let mut offset = 0;
    while offset < total {
        let chunk = read_rows(snap, offset, SCAN_PAGE)?;
        let scan = Snapshot { page: chunk.clone(), ..snap.clone() };
        for (i, row) in chunk.rows.iter().enumerate() {
            let mut values = Vec::new();
            let mut expected = None;
            for &j in snap.editable.iter().filter(|j| {
                let field = snap.fields.get(**j).and_then(Option::as_ref);
                cell_matches(&row[**j], options, field)
            }) {
                let write = replacement_write(&scan, options, replacement, i, j, &row[j])
                    .map_err(|m| AppError::new("VALIDATION_ERROR", m))?;
                expected = write.meta.expected.clone();
                values.extend(write.values);
            }
            if values.is_empty() {
                continue;
            }
            cells += values.len();
            writes.push(
                RecordWrite::update(&snap.table, values, chunk.identities[i].clone())
                    .with_meta(RecordWriteMeta { expected, ..Default::default() }),
            );
        }
        offset += SCAN_PAGE;
    }
    Ok((writes, cells))
}

impl Render for FindPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = self.options(cx).find.is_empty();
        let busy = self.busy;
        let read_only = self.read_only(cx);
        let panel = cx.entity();
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        let scope = match self.scope {
            Scope::All => "all",
            Scope::Column => "column",
        };
        let pick = {
            let panel = panel.clone();
            move |value: &str, _: &mut Window, cx: &mut App| {
                let scope = if value == "column" { Scope::Column } else { Scope::All };
                panel.update(cx, |this, cx| {
                    this.scope = scope;
                    cx.notify();
                });
            }
        };
        let toggle_case = panel.clone();
        let toggle_whole = panel.clone();
        h_flex()
            .id("find-replace")
            .aria_label("Find and replace")
            .flex_wrap()
            .gap_2()
            .items_center()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(border)
            .child(div().w(px(200.)).child(text_input(&self.find, "find-text", "Find")))
            .when(!read_only, |el| {
                el.child(div().w(px(200.)).child(text_input(&self.replacement, "replace-text", "Replace with")))
            })
            .child(select(
                "find-scope",
                scope,
                vec![("all".into(), "All columns".into()), ("column".into(), "Current column".into())],
                "All columns",
                pick,
            ))
            .child(check("find-whole", "Whole field", self.whole_field, move |value, _, cx| {
                toggle_whole.update(cx, |this, cx| {
                    this.whole_field = value;
                    cx.notify();
                })
            }))
            .child(check("find-case", "Match case", self.match_case, move |value, _, cx| {
                toggle_case.update(cx, |this, cx| {
                    this.match_case = value;
                    cx.notify();
                })
            }))
            .child(
                Button::new("find-next")
                    .small()
                    .icon(IconName::Search)
                    .label("Find next")
                    .disabled(empty || busy)
                    .on_click(cx.listener(|this, _, _, cx| this.find_next(cx))),
            )
            .when(!read_only, |el| {
                el.child(
                    Button::new("find-replace-one")
                        .outline()
                        .small()
                        .label("Replace")
                        .disabled(empty || busy)
                        .on_click(cx.listener(|this, _, _, cx| this.replace(cx))),
                )
                .child(
                    Button::new("find-replace-all")
                        .outline()
                        .small()
                        .label("Replace all")
                        .disabled(empty || busy)
                        .on_click(cx.listener(|this, _, _, cx| this.replace_all(cx))),
                )
            })
            .child(
                Button::new("find-close")
                    .ghost()
                    .small()
                    .icon(IconName::Close)
                    .tooltip("Close find and replace")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(FindEvent::Close))),
            )
            .when(!self.status.is_empty(), |el| {
                el.child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(widgets::text("find-status", self.status.clone())),
                )
            })
    }
}
