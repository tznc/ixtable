//! List mode (port of `ListView.tsx`): DuckDB-backed paging with sort and a contains
//! filter; rows open the detail form. Cells take the conditional tone of the control bound to
//! their column, lookups show a related record's display value, and yes/no columns show a
//! check or a dash.
//!
//! Test ids: the region is `list:<form name slug>` (labelled with the form name), the title
//! `list-title`, `search-column` (button; its choices are `search-column:option:<column
//! slug>`), `search` (input), `new-record` ("New <detail name>"), headers `col:<slug>` with a
//! `sort:<slug>` button and `sort-dir:<slug>` ("ascending", "descending" or "none"), rows
//! `row:<n>` ("Open X"), cells `cell:<n>:<slug>` (their text, "Yes"/"No" for booleans, plus
//! `cell:<n>:<slug>:tone` when a tone applies), `list-empty`, `pager-status`, `prev-page`,
//! `next-page`, `list-error`, `list-notice` and the export buttons (see `export.rs`).

use super::{RuntimeEnv, export::ExportMenu};
use crate::data::{DataValue, Filter, FilterOperator, Sort, TableSchema};
use crate::design::{Control, Form, SourceKind, Tone};
use crate::forms::conditions::{TRUNCATED_NOTICE, page_label, tone_for};
use crate::forms::cursor::RecordCursor;
use crate::forms::data::{PageRequest, RecordPage, cached_page, load_page, record_id_for, source_params, table_schema};
use crate::forms::fields::field_settings_for;
use crate::forms::generate::humanize;
use crate::forms::lookups::{LookupLabels, Lookups, column_lookups, label_for, lookup_labels};
use crate::forms::rbac::Operation;
use crate::forms::registry::{is_designed_form, resolve_form};
use crate::forms::state::cell_text;
use crate::forms::values::{RecordValues, boolean_value, is_boolean_column};
use crate::ui::events::DocEvent;
use crate::ui::prelude::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use serde_json::{Map, Value};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

pub type OpenHandler = Rc<dyn Fn(Value, RecordCursor, &mut App)>;

#[derive(Clone)]
pub struct ListProps {
    pub env: RuntimeEnv,
    pub form: Form,
    pub on_open: OpenHandler,
    pub on_create: Rc<dyn Fn(&mut App)>,
    /// Page parameters (navigation or dashboard) in scope for query source bindings.
    pub params: Map<String, Value>,
}

/// What one read produced.
struct Loaded {
    page: Result<RecordPage, String>,
    schema: Option<Arc<TableSchema>>,
    lookups: Lookups,
    labels: LookupLabels,
}

pub struct ListView {
    props: ListProps,
    sorts: Vec<Sort>,
    offset: u64,
    search: Entity<InputState>,
    search_column: String,
    column_menu: bool,
    page: Option<RecordPage>,
    schema: Option<Arc<TableSchema>>,
    error: String,
    lookups: Lookups,
    labels: LookupLabels,
    selected: Option<usize>,
    seq: u64,
    export: Entity<ExportMenu>,
    _subscriptions: Vec<Subscription>,
}

fn slug_of(column: &str) -> String {
    widgets::slug(column)
}

impl ListView {
    pub fn new(props: ListProps, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search…"));
        let mut subscriptions = vec![cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.search_changed(cx);
            }
        })];
        let label = props.env.label.to_string();
        let bus = props.env.bus.clone();
        subscriptions.push(cx.subscribe(&bus, move |this, _, event: &DocEvent, cx| match event {
            DocEvent::DatabaseChanged => {
                crate::forms::data::invalidate(&label, true);
                this.schema = None;
                this.reload(0, cx);
            }
            DocEvent::RecordsChanged => this.reload(0, cx),
        }));
        let export = cx.new(|_| ExportMenu::new(props.env.label.to_string()));
        let mut view = Self {
            props,
            sorts: vec![],
            offset: 0,
            search,
            search_column: String::new(),
            column_menu: false,
            page: None,
            schema: None,
            error: String::new(),
            lookups: vec![],
            labels: LookupLabels::new(),
            selected: None,
            seq: 0,
            export,
            _subscriptions: subscriptions,
        };
        view.reload(0, cx);
        view
    }

    pub fn page(&self) -> Option<&RecordPage> {
        self.page.as_ref()
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    fn limit(&self) -> u64 {
        u64::from(self.props.form.page_size).max(1)
    }

    pub fn can_prev(&self) -> bool {
        self.offset > 0
    }

    pub fn can_next(&self) -> bool {
        self.page.as_ref().is_some_and(|p| self.offset + self.limit() < p.total)
    }

    /// Marks the row at this list position as selected (split forms).
    pub fn set_selected(&mut self, selected: Option<usize>, cx: &mut Context<Self>) {
        if self.selected != selected {
            self.selected = selected;
            cx.notify();
        }
    }

    /// Re-reads the current page (split forms, after a save in the detail pane).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.reload(0, cx);
    }

    fn detail_form(&self, cx: &App) -> Form {
        self.props
            .form
            .detail_form_id
            .as_deref()
            .filter(|id| !id.is_empty())
            .and_then(|id| resolve_form(self.props.env.config(cx), id))
            .unwrap_or_else(|| self.props.form.clone())
    }

    fn table(&self) -> Option<String> {
        self.props
            .form
            .source
            .as_ref()
            .filter(|s| s.kind == SourceKind::Table)
            .and_then(|s| s.table.clone())
            .filter(|t| !t.is_empty())
    }

    fn filters(&self, cx: &App) -> Vec<Filter> {
        let text = self.search.read(cx).value().trim().to_string();
        if text.is_empty() || self.search_column.is_empty() {
            return vec![];
        }
        vec![Filter {
            column: self.search_column.clone(),
            operator: FilterOperator::Contains,
            value: Some(DataValue::Text(text)),
            values: None,
        }]
    }

    fn scope(&self, cx: &App) -> Map<String, Value> {
        let mut scope = Map::new();
        scope.insert("app".into(), Value::Object(self.props.env.app(cx)));
        scope.insert("params".into(), Value::Object(self.props.params.clone()));
        scope
    }

    /// The columns shown: the list columns, else the bound columns, else the page's.
    fn columns(&self) -> Vec<String> {
        let form = &self.props.form;
        if !form.list_columns.is_empty() {
            return form.list_columns.clone();
        }
        let bound: Vec<String> = form
            .controls
            .iter()
            .filter_map(|c| c.binding.as_ref().map(|b| b.column.clone()).filter(|c| !c.is_empty()))
            .collect();
        if bound.is_empty() {
            self.page.as_ref().map(|p| p.columns.clone()).unwrap_or_default()
        } else {
            bound
        }
    }

    fn control_for(&self, column: &str, cx: &App) -> Option<Control> {
        let detail = self.detail_form(cx);
        self.props
            .form
            .controls
            .iter()
            .chain(detail.controls.iter())
            .find(|c| c.binding.as_ref().is_some_and(|b| b.column == column))
            .cloned()
    }

    fn label_of(&self, column: &str, cx: &App) -> String {
        self.control_for(column, cx).map_or_else(|| humanize(column), |c| c.label)
    }

    fn search_changed(&mut self, cx: &mut Context<Self>) {
        if self.search_column.is_empty() {
            self.search_column = self.columns().first().cloned().unwrap_or_default();
        }
        self.offset = 0;
        // Typing waits for a pause, so each key does not start a read.
        self.reload(200, cx);
    }

    /// Starts a read of the current page after `delay_ms`; a newer read supersedes it.
    fn reload(&mut self, delay_ms: u64, cx: &mut Context<Self>) {
        self.seq += 1;
        let seq = self.seq;
        let scope = self.scope(cx);
        let form = self.props.form.clone();
        let bound = match source_params(&form, &scope) {
            Ok(bound) => bound,
            Err(message) => {
                self.error = message;
                self.page = None;
                cx.notify();
                return;
            }
        };
        let request = PageRequest {
            offset: self.offset,
            limit: self.limit(),
            sorts: self.sorts.clone(),
            filters: self.filters(cx),
        };
        let label = self.props.env.label.to_string();
        if let Some(hit) = cached_page(&label, &form, &request, &scope, &bound) {
            self.page = Some(hit);
        }
        let table = self.table();
        let detail = self.detail_form(cx);
        let work = move || {
            let page = load_page(&label, &form, &request, &scope, &bound).map_err(|e| e.message);
            let schema = table.as_deref().and_then(|t| table_schema(&label, t).ok());
            let (lookups, labels) = match &page {
                Ok(page) => {
                    let columns: Vec<String> = if form.list_columns.is_empty() {
                        let bound: Vec<String> = form
                            .controls
                            .iter()
                            .filter_map(|c| c.binding.as_ref().map(|b| b.column.clone()).filter(|c| !c.is_empty()))
                            .collect();
                        if bound.is_empty() { page.columns.clone() } else { bound }
                    } else {
                        form.list_columns.clone()
                    };
                    let lookups = column_lookups(&label, table.as_deref(), &columns, |column| {
                        form.controls
                            .iter()
                            .chain(detail.controls.iter())
                            .find(|c| c.binding.as_ref().is_some_and(|b| b.column == column))
                            .and_then(|c| c.relationship.clone())
                    });
                    let labels = lookup_labels(&label, &lookups, &page.rows);
                    (lookups, labels)
                }
                Err(_) => (vec![], LookupLabels::new()),
            };
            Loaded { page, schema, lookups, labels }
        };
        let apply = move |this: &mut Self, loaded: Loaded, cx: &mut Context<Self>| {
            if this.seq != seq {
                return;
            }
            this.schema = loaded.schema;
            match loaded.page {
                Ok(page) => {
                    this.page = Some(page);
                    this.error.clear();
                    this.lookups = loaded.lookups;
                    this.labels = loaded.labels;
                }
                Err(message) => this.error = message,
            }
            cx.notify();
        };
        if delay_ms == 0 {
            task::spawn_call(cx, work, apply);
        } else {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(delay_ms)).await;
                let _ = this.update(cx, |this, cx| {
                    if this.seq == seq {
                        task::spawn_call(cx, work, apply);
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    fn toggle_sort(&mut self, column: &str, cx: &mut Context<Self>) {
        self.offset = 0;
        self.sorts = match self.sorts.iter().find(|s| s.column == column) {
            None => vec![Sort { column: column.to_string(), descending: false }],
            Some(s) if !s.descending => vec![Sort { column: column.to_string(), descending: true }],
            Some(_) => vec![],
        };
        self.reload(0, cx);
    }

    fn page_by(&mut self, forward: bool, cx: &mut Context<Self>) {
        let limit = self.limit();
        if forward && self.can_next() {
            self.offset += limit;
        } else if !forward && self.can_prev() {
            self.offset = self.offset.saturating_sub(limit);
        } else {
            return;
        }
        self.reload(0, cx);
    }

    fn select_search_column(&mut self, column: String, cx: &mut Context<Self>) {
        self.search_column = column;
        self.column_menu = false;
        self.offset = 0;
        self.reload(0, cx);
    }

    /// Opens the row at page position `index`; waits for the schema when a database change
    /// cleared it.
    pub fn open_row(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(page) = &self.page else { return };
        let Some(record) = page.rows.get(index).cloned() else { return };
        let cursor = RecordCursor {
            form_id: self.props.form.id.clone(),
            index: (self.offset as usize) + index,
            sorts: self.sorts.clone(),
            filters: self.filters(cx),
            params: self.props.params.clone(),
        };
        let on_open = self.props.on_open.clone();
        let identity = page.identities.as_ref().and_then(|ids| ids.get(index)).cloned();
        let (Some(table), Some(identity)) = (self.table(), identity) else {
            on_open(Value::Object(record), cursor, cx);
            return;
        };
        let label = self.props.env.label.to_string();
        if let Some(schema) = self.schema.clone() {
            let id = record_id_for(&label, &schema, &record, &identity);
            on_open(id, cursor, cx);
            return;
        }
        task::spawn_call(
            cx,
            {
                let label = label.clone();
                move || table_schema(&label, &table).map(|schema| record_id_for(&label, &schema, &record, &identity))
            },
            move |_, result, cx| {
                if let Ok(id) = result {
                    on_open(id, cursor, cx);
                }
            },
        );
    }

    fn tone_of(&self, record: &RecordValues, column: &str, cx: &App) -> Option<Tone> {
        let control = self.control_for(column, cx)?;
        let mut scope = Map::new();
        scope.insert("record".into(), Value::Object(record.clone()));
        scope.insert("form".into(), Value::Object(Map::new()));
        scope.insert("app".into(), Value::Object(self.props.env.app(cx)));
        scope.insert("value".into(), record.get(column).cloned().unwrap_or(Value::Null));
        tone_for(&control.styles, &scope, None)
    }

    fn boolean_columns(&self, columns: &[String], cx: &App) -> Vec<bool> {
        let detail = self.detail_form(cx);
        columns
            .iter()
            .map(|column| {
                let has = self
                    .props
                    .form
                    .controls
                    .iter()
                    .chain(detail.controls.iter())
                    .filter(|c| c.binding.as_ref().is_some_and(|b| &b.column == column))
                    .any(|c| c.kind == crate::design::ControlKind::Boolean);
                let col = self.schema.as_ref().and_then(|s| s.columns.iter().find(|c| &c.name == column));
                is_boolean_column(has, col)
            })
            .collect()
    }

    fn cell_text(&self, record: &RecordValues, column: &str, cx: &App) -> String {
        if let Some(text) = label_for(&self.lookups, &self.labels, column, record) {
            return text;
        }
        let table = self.table();
        let format = field_settings_for(&self.props.env.config(cx).entities, table.as_deref(), Some(column))
            .and_then(|f| f.format.clone());
        cell_text(
            record.get(column).unwrap_or(&Value::Null),
            self.control_for(column, cx).as_ref(),
            format.as_deref(),
        )
    }

    fn can_create(&self, cx: &App) -> bool {
        let detail = self.detail_form(cx);
        let config = self.props.env.config(cx);
        let subject = if is_designed_form(config, &detail) {
            ("form", detail.id.clone())
        } else {
            ("table", self.table().unwrap_or_default())
        };
        self.table().is_some()
            && detail.modes.contains(&crate::design::FormMode::Create)
            && self.props.env.can(subject.0, &subject.1, Operation::Create, cx)
    }
}

impl Render for ListView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let form = self.props.form.clone();
        let columns = self.columns();
        let booleans = self.boolean_columns(&columns, cx);
        let theme = cx.theme().clone();
        let effective = if self.search_column.is_empty() {
            columns.first().cloned().unwrap_or_default()
        } else {
            self.search_column.clone()
        };
        let detail = self.detail_form(cx);
        let total = self.page.as_ref().map_or(0, |p| p.total);
        let shown = self.page.as_ref().map_or(0, |p| p.rows.len() as u64);
        let status = match &self.page {
            Some(_) => page_label(self.offset, shown, total),
            None => "Loading…".to_string(),
        };

        let header = h_flex().gap_2().border_b_1().border_color(theme.border).children(columns.iter().map(|column| {
            let sort = self.sorts.iter().find(|s| &s.column == column).cloned();
            let slug = slug_of(column);
            let label = self.label_of(column, cx);
            let direction = match &sort {
                Some(s) if s.descending => "descending",
                Some(_) => "ascending",
                None => "none",
            };
            let target = column.clone();
            h_flex()
                .id(SharedString::from(format!("col:{slug}")))
                .aria_label(label.clone())
                .flex_1()
                .min_w_0()
                .child(
                    Button::new(SharedString::from(format!("sort:{slug}")))
                        .ghost()
                        .small()
                        .label(label)
                        .selected(sort.is_some())
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_sort(&target, cx))),
                )
                .child(widgets::styled_text(
                    SharedString::from(format!("sort-dir:{slug}")),
                    direction,
                    |d| d.text_xs().text_color(gpui_kit::transparent_black()),
                ))
                .test_support()
        }));

        let mut rows = v_flex();
        if let Some(page) = &self.page {
            for (index, record) in page.rows.iter().enumerate() {
                let first = columns.first().map(String::as_str).unwrap_or("");
                let name = match record.get(first) {
                    None | Some(Value::Null) => format!("row {}", self.offset as usize + index + 1),
                    Some(_) => self.cell_text(record, first, cx),
                };
                let selected = self.selected == Some(self.offset as usize + index);
                let row_id = SharedString::from(format!("row:{index}"));
                let focus = window.use_keyed_state((ElementId::from(row_id.clone()), "focus"), cx, |_, cx| cx.focus_handle());
                let handle = focus.read(cx).clone();
                let cells = columns.iter().enumerate().map(|(slot, column)| {
                    let cell_id = SharedString::from(format!("cell:{index}:{}", slug_of(column)));
                    let tone = self.tone_of(record, column, cx);
                    let color = tone.as_ref().and_then(|t| widgets::tone_color(t, cx));
                    let tone_id = tone.as_ref().map(|t| (SharedString::from(format!("{cell_id}:tone")), t.as_str().to_string()));
                    let content: AnyElement = if booleans[slot] {
                        match boolean_value(record.get(column).unwrap_or(&Value::Null)) {
                            Some(on) => widgets::styled_text(cell_id.clone(), if on { "Yes" } else { "No" }, move |d| {
                                d.text_sm().when_some(color, |d, c| d.text_color(c))
                            })
                            .into_any_element(),
                            None => div().id(cell_id.clone()).into_any_element(),
                        }
                    } else {
                        let text = self.cell_text(record, column, cx);
                        widgets::styled_text(cell_id.clone(), text, move |d| {
                            d.text_sm().when_some(color, |d, c| d.text_color(c))
                        })
                        .into_any_element()
                    };
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(content)
                        .children(tone_id.map(|(id, name)| widgets::text(id, name)))
                });
                rows = rows.child(
                    h_flex()
                        .id(row_id)
                        .aria_label(format!("Open {name}"))
                        .track_focus(&handle)
                        .gap_2()
                        .py_1()
                        .border_b_1()
                        .border_color(theme.border)
                        .when(selected, |d| d.bg(theme.accent))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| this.open_row(index, cx)))
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.open_row(index, cx);
                            }
                        }))
                        .children(cells)
                        .test_support(),
                );
            }
            if page.rows.is_empty() {
                rows = rows.child(widgets::empty_state("list-empty", "No records yet", "Records you add or that match your search will show here.", cx));
            }
        }

        let column_menu = self.column_menu.then(|| {
            v_flex().gap_0p5().children(columns.iter().map(|column| {
                let target = column.clone();
                Button::new(SharedString::from(format!("search-column:option:{}", slug_of(column))))
                    .ghost()
                    .small()
                    .start_label(None, self.label_of(column, cx))
                    .selected(column == &effective)
                    .on_click(cx.listener(move |this, _, _, cx| this.select_search_column(target.clone(), cx)))
            }))
        });
        let can_create = self.can_create(cx);
        let create_label = format!("New {}", widgets::singular(&detail.name.to_lowercase()));
        let name = form.name.clone();
        let export = self.export.clone();
        let (filters, scope) = (self.filters(cx), self.scope(cx));
        let (export_form, sorts) = (self.props.form.clone(), self.sorts.clone());
        self.export.update(cx, |menu, _| menu.configure(export_form, sorts, filters, scope));

        v_flex()
            .id(SharedString::from(format!("list:{}", widgets::slug(&form.name))))
            .aria_label(form.name.clone())
            .size_full()
            .gap_2()
            .p_4()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(widgets::styled_text("list-title", name.clone(), |d| d.text_xl().font_weight(FontWeight::SEMIBOLD)))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            // No columns to pick from means no empty column button beside Search.
                            .when(!effective.is_empty(), |d| {
                                d.child(
                                    Button::new("search-column")
                                        .outline()
                                        .small()
                                        .label(self.label_of(&effective, cx))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.column_menu = !this.column_menu;
                                            cx.notify();
                                        })),
                                )
                            })
                            .child(div().w(px(220.)).child(
                                Input::new(&self.search).id("search").aria_label(format!("Search {name}")),
                            ))
                            .when(can_create, |d| {
                                let on_create = self.props.on_create.clone();
                                d.child(
                                    Button::new("new-record")
                                        .primary()
                                        .small()
                                        .label(create_label)
                                        .on_click(move |_, _, cx| on_create(cx)),
                                )
                            }),
                    ),
            )
            .children(column_menu)
            .when(!self.error.is_empty(), |d| d.child(super::message("list-error", self.error.clone(), true, cx)))
            .when(self.page.as_ref().is_some_and(|p| p.truncated), |d| {
                d.child(widgets::callout("list-notice", widgets::Severity::Warning, TRUNCATED_NOTICE, None, cx))
            })
            .child(header)
            .child(rows)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(widgets::text("pager-status", status))
                    .child(
                        Button::new("prev-page")
                            .small()
                            .label("Previous page")
                            .disabled(!self.can_prev())
                            .on_click(cx.listener(|this, _, _, cx| this.page_by(false, cx))),
                    )
                    .child(
                        Button::new("next-page")
                            .small()
                            .label("Next page")
                            .disabled(!self.can_next())
                            .on_click(cx.listener(|this, _, _, cx| this.page_by(true, cx))),
                    )
                    .child(export),
            )
            .test_support()
    }
}
