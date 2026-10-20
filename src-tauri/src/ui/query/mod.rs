//! The query editor of the Data page (PRD §12): the visual builder, SQL, parameters,
//! action queries and results. Port of src/query/. `QueryPicker` is the reusable query
//! selector other modes use.
//!
//! Saved queries are listed in the Data page's object browser; choosing one (or New
//! query) selects it in the shell and `DataMode` shows this view in its workspace. The
//! view is one entity: the draft being edited, the run state and the open drop-down live
//! in `QueryMode`, and the panels are `impl QueryMode` blocks in the sibling files. The
//! draft survives a visit to a table, and a new query that is not saved yet is published
//! to the shell (`Shell::unsaved_query`) so the object browser can list it.
//!
//! Test ids: `create-query`, `query-error`, `query-name`, `query-save`, `query-run`,
//! `query-duplicate`, `query-delete`, `query-mode-badge`, `query-unsaved`, `tab-builder`,
//! `tab-sql`; the panels list theirs in their own files.

mod action;
mod action_text;
mod choice;
mod compile;
mod editor;
mod export;
mod fields;
mod filters;
mod inputs;
mod joins;
mod model;
mod params;
mod picker;
pub(crate) mod results;
mod run;
mod sortgroup;
mod sources;
mod sql;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_builder;
#[cfg(test)]
pub(crate) mod tests_support;
#[cfg(test)]
mod tests_unit;
#[cfg(test)]
mod tests_grouping;
#[cfg(test)]
mod tests_sections;

pub use picker::{QueryPicker, QueryPickerEvent};

use crate::{
    archive::SavedQuery,
    ui::{
        dialogs,
        prelude::*,
        shell::{Selection, SelectionKind, Shell},
        task, widgets,
    },
};
use choice::Opt;
use gpui_kit::component::input::{EditorState, InputEvent};
use inputs::{Field, InputSlot};
use joins::Schemas;
use model::{BuilderModel, empty_model, new_id, normalize_model};
use results::ResultTable;
use run::RunState;
use std::collections::{BTreeMap, HashMap};

/// Rows shown by the builder's live preview.
const PREVIEW_ROWS: u64 = 100;
const PREVIEW_DELAY_MS: u64 = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Builder,
    Sql,
}

/// The query being edited and the saved state it is compared with.
pub struct Draft {
    pub query: SavedQuery,
    pub baseline: SavedQuery,
    /// A fresh draft that is not in the document yet.
    pub is_new: bool,
    pub saving: bool,
}

impl Draft {
    pub fn dirty(&self) -> bool {
        self.query != self.baseline
    }
}

#[derive(Default)]
struct Preview {
    generation: u64,
    key: String,
}

#[derive(Default)]
pub(crate) struct ActionState {
    pub busy: bool,
    /// Text of the last preview or run, and whether it is an error.
    pub result: Option<(String, bool)>,
    /// Rows the last preview would change; offered as "Apply to N rows".
    pub preview_rows: Option<u64>,
}

pub struct QueryMode {
    pub(crate) shell: Entity<Shell>,
    label: String,
    pub(crate) draft: Option<Draft>,
    error: String,
    last_selection: Option<Selection>,
    pub(crate) tab: Tab,
    /// "Run with" values by parameter name.
    pub(crate) values: BTreeMap<String, String>,
    pub(crate) run: RunState,
    pub(crate) results: Entity<ResultTable>,
    pub(crate) action: ActionState,
    pub(crate) export: export::ExportState,
    pub(crate) sql_editor: Entity<EditorState>,
    /// Read-only view of the SQL a builder query generates.
    pub(crate) generated_editor: Entity<EditorState>,
    inputs: HashMap<Field, InputSlot>,
    /// The default each "Run with" input last followed, by parameter name.
    param_defaults: HashMap<String, String>,
    open_choice: Option<SharedString>,
    join_table: String,
    group_choice: String,
    pub(crate) schemas: Schemas,
    schema_key: Option<String>,
    preview: Preview,
    _subscriptions: Vec<Subscription>,
}

impl QueryMode {
    pub fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sql_editor = cx.new(|cx| EditorState::new(window, cx).language("sql"));
        let generated_editor = cx.new(|cx| EditorState::new(window, cx).language("sql"));
        let results = cx.new(|_| ResultTable::new());
        let subscriptions = vec![
            cx.observe_in(&shell, window, |this, _, window, cx| {
                this.follow_selection(window, cx);
                this.follow_config(window, cx);
                cx.notify();
            }),
            cx.subscribe(&sql_editor, |this, state, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = state.read(cx).value().to_string();
                    this.sql_edited(text, cx);
                }
            }),
            cx.observe(&results, |_, _, cx| cx.notify()),
        ];
        let this = Self {
            shell,
            label: String::new(),
            draft: None,
            error: String::new(),
            last_selection: None,
            tab: Tab::Sql,
            values: BTreeMap::new(),
            run: RunState::default(),
            results,
            action: ActionState::default(),
            export: export::ExportState::default(),
            sql_editor,
            generated_editor,
            inputs: HashMap::new(),
            param_defaults: HashMap::new(),
            open_choice: None,
            join_table: String::new(),
            group_choice: String::new(),
            schemas: Schemas::new(),
            schema_key: None,
            preview: Preview::default(),
            _subscriptions: subscriptions,
        };
        // The shell is mid-update while it builds its mode view, so read it afterwards.
        cx.defer_in(window, |this, window, cx| {
            this.label = this.shell.read(cx).label().to_string();
            this.follow_selection(window, cx);
            cx.notify();
        });
        this
    }

    // ---- State access ----------------------------------------------------------

    pub fn queries<'a>(&self, cx: &'a App) -> &'a [SavedQuery] {
        &self.shell.read(cx).config(cx).saved_queries
    }

    pub fn draft(&self) -> Option<&Draft> {
        self.draft.as_ref()
    }

    pub fn is_dirty(&self) -> bool {
        self.draft.as_ref().is_some_and(Draft::dirty)
    }

    pub fn error_text(&self) -> &str {
        &self.error
    }

    /// The builder model of the draft, normalized (None for a SQL query).
    pub(crate) fn model(&self) -> Option<BuilderModel> {
        let builder = self.draft.as_ref()?.query.builder.as_ref()?;
        (!builder.is_null()).then(|| normalize_model(builder))
    }

    pub(crate) fn tables(&self, cx: &App) -> Vec<String> {
        self.shell
            .read(cx)
            .objects
            .iter()
            .filter(|o| o.object_type == "table")
            .map(|o| o.name.clone())
            .collect()
    }

    pub(crate) fn tables_and_views(&self, cx: &App) -> Vec<String> {
        self.shell
            .read(cx)
            .objects
            .iter()
            .filter(|o| o.object_type == "table" || o.object_type == "view")
            .map(|o| o.name.clone())
            .collect()
    }

    /// `(sql, error)` the draft would run.
    pub(crate) fn compiled(&self) -> (String, String) {
        self.draft
            .as_ref()
            .map(|d| sql::effective_sql(&d.query))
            .unwrap_or_default()
    }

    pub(crate) fn choice_is_open(&self, id: &str) -> bool {
        self.open_choice.as_deref() == Some(id)
    }

    /// A drop-down whose open state lives in this view.
    pub(crate) fn choice(
        &self,
        id: impl Into<SharedString>,
        current: impl Into<String>,
        options: Vec<Opt>,
        on_pick: impl Fn(&mut Self, String, &mut Window, &mut Context<Self>) + 'static,
        _: &mut Context<Self>,
    ) -> choice::Choice<Self> {
        let id: SharedString = id.into();
        let open = self.choice_is_open(&id);
        let key = id.clone();
        choice::Choice::new(
            id,
            current,
            options,
            open,
            move |this: &mut Self, open, cx| {
                this.open_choice = open.then(|| key.clone());
                cx.notify();
            },
            on_pick,
        )
    }

    pub(crate) fn error_line(&self, id: &'static str, message: &str, cx: &App) -> AnyElement {
        let danger = cx.theme().danger;
        widgets::styled_text(id, message.to_string(), move |d| {
            d.text_sm().text_color(danger).py_1()
        })
        .into_any_element()
    }

    // ---- Editing the draft -------------------------------------------------------

    /// Edits the draft's query; the preview and view refresh.
    pub(crate) fn edit_query(&mut self, f: impl FnOnce(&mut SavedQuery), cx: &mut Context<Self>) {
        if let Some(d) = &mut self.draft {
            f(&mut d.query);
        }
        self.touched(cx);
    }

    /// Edits the builder model; the SQL follows it (`sql` keeps its old text while the
    /// model does not compile).
    pub(crate) fn edit_model(&mut self, f: impl FnOnce(&mut BuilderModel), cx: &mut Context<Self>) {
        let Some(mut model) = self.model() else {
            return;
        };
        f(&mut model);
        let (sql, _) = compile::try_compile(&model);
        self.edit_query(
            |q| {
                q.builder = Some(model.to_value());
                if !sql.is_empty() {
                    q.sql = sql;
                }
            },
            cx,
        );
    }

    pub(crate) fn sql_edited(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(d) = &self.draft else {
            return;
        };
        if d.query.sql == text && d.query.builder.as_ref().is_none_or(|b| b.is_null()) {
            return;
        }
        self.edit_query(
            |q| {
                q.builder = None;
                q.sql = text;
            },
            cx,
        );
    }

    pub(crate) fn parameters_changed(&mut self, cx: &mut Context<Self>) {
        self.touched(cx);
    }

    /// After any change: refresh the view and the debounced builder preview.
    pub(crate) fn touched(&mut self, cx: &mut Context<Self>) {
        self.refresh_preview(cx);
        self.publish_unsaved(cx);
        cx.notify();
    }

    /// Tells the shell about the new query that is not saved yet (the object browser
    /// lists it).
    fn publish_unsaved(&mut self, cx: &mut Context<Self>) {
        let unsaved = self.draft.as_ref().filter(|d| d.is_new).map(|d| {
            let name = if d.query.name.trim().is_empty() { "Untitled query" } else { d.query.name.trim() };
            (d.query.id.clone(), name.to_string())
        });
        self.shell.update(cx, |s, cx| s.set_unsaved_query(unsaved, cx));
    }

    /// Drops the draft (its query was deleted).
    fn close_draft(&mut self, cx: &mut Context<Self>) {
        self.draft = None;
        self.reset_run(cx);
        self.publish_unsaved(cx);
    }

    /// Live preview of the builder (debounced, first `PREVIEW_ROWS` rows).
    fn refresh_preview(&mut self, cx: &mut Context<Self>) {
        let key = self.preview_key();
        if key == self.preview.key {
            return;
        }
        self.preview.key = key.clone();
        if key.is_empty() {
            return;
        }
        self.preview.generation += 1;
        let generation = self.preview.generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(PREVIEW_DELAY_MS))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.preview.generation != generation {
                    return;
                }
                let Some(d) = &this.draft else {
                    return;
                };
                let (sql, _) = this.compiled();
                let parameters = d.query.parameters.clone();
                let values = sql::run_params(&this.values);
                this.run_sql(sql, &values, parameters, Some(PREVIEW_ROWS), cx);
            });
        })
        .detach();
    }

    fn preview_key(&self) -> String {
        let Some(d) = &self.draft else {
            return String::new();
        };
        let (sql, _) = self.compiled();
        let visual = self.model().is_some_and(|m| !m.is_empty());
        if !(visual && self.tab == Tab::Builder && d.query.action.is_none() && !sql.is_empty()) {
            return String::new();
        }
        serde_json::json!([sql, d.query.parameters, sql::run_params(&self.values)]).to_string()
    }

    // ---- Opening, creating, discarding --------------------------------------------

    /// Runs `then` now, or once the user agrees to discard unsaved changes.
    pub(crate) fn guard(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let Some(d) = self.draft.as_ref().filter(|d| d.dirty()) else {
            then(self, window, cx);
            return;
        };
        let name = if d.query.name.is_empty() {
            "this query"
        } else {
            &d.query.name
        };
        let message = format!("Discard unsaved changes to {name}?");
        let answer = dialogs::confirm(cx, "Discard changes", &message);
        task::after_in(window, cx, answer, move |this, ok, window, cx| {
            if ok {
                then(this, window, cx);
            } else {
                // Keep the sidebar on the query that stays open.
                this.select_draft(cx);
            }
        });
    }

    /// Selects the open draft in the shell.
    fn select_draft(&mut self, cx: &mut Context<Self>) {
        let Some(d) = &self.draft else {
            return;
        };
        let kind = if d.is_new { SelectionKind::NewQuery } else { SelectionKind::Query };
        let selection = Selection { kind, id: d.query.id.clone() };
        self.last_selection = Some(selection.clone());
        self.shell.update(cx, |s, cx| s.select(Some(selection), cx));
    }

    /// Makes `query` the draft. The tab, run values and results are kept when it is the
    /// query already open (a save, or an outside edit).
    pub(crate) fn open(&mut self, query: SavedQuery, fresh: bool, cx: &mut Context<Self>) {
        let same = self.draft.as_ref().is_some_and(|d| d.query.id == query.id);
        self.draft = Some(Draft {
            query: query.clone(),
            baseline: query,
            is_new: fresh,
            saving: false,
        });
        self.error.clear();
        self.inputs.clear();
        self.param_defaults.clear();
        self.open_choice = None;
        if !same {
            self.reset_session(cx);
        }
        self.touched(cx);
    }

    fn reset_session(&mut self, cx: &mut Context<Self>) {
        self.tab = if self.model().is_some() { Tab::Builder } else { Tab::Sql };
        self.values.clear();
        self.action = ActionState::default();
        self.export = export::ExportState::default();
        self.preview = Preview::default();
        self.reset_run(cx);
    }

    pub fn new_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_query_on(None, window, cx);
    }

    /// Starts a builder query that reads `table` ("Query this table" on a datasheet).
    pub fn new_query_from(&mut self, table: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.new_query_on(Some(table.to_string()), window, cx);
    }

    fn new_query_on(&mut self, table: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.guard(window, cx, move |this, _, cx| {
            let mut model = empty_model();
            let name = match &table {
                Some(table) => {
                    model.sources.push(model::BuilderSource {
                        id: new_id(),
                        alias: model::alias_for(table, &[]),
                        table: table.clone(),
                    });
                    unique_name(&format!("{table} query"), this.queries(cx))
                }
                None => unique_name("Untitled query", this.queries(cx)),
            };
            let (sql, _) = compile::try_compile(&model);
            let query = SavedQuery {
                id: new_id(),
                name,
                sql,
                parameters: vec![],
                builder: Some(model.to_value()),
                ..Default::default()
            };
            this.open(query, true, cx);
            this.select_draft(cx);
        });
    }

    /// Opens a saved query from the list and selects it in the shell.
    pub fn open_id(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(query) = self.queries(cx).iter().find(|q| q.id == id).cloned() else {
            return;
        };
        if self.draft.as_ref().is_some_and(|d| d.query.id == id) {
            return;
        }
        self.guard(window, cx, move |this, _, cx| {
            let id = query.id.clone();
            this.open(query, false, cx);
            this.select_in_shell(Some(id), cx);
        });
    }

    fn select_in_shell(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        let selection = id.map(|id| Selection {
            kind: SelectionKind::Query,
            id,
        });
        self.last_selection = selection.clone();
        self.shell.update(cx, |s, cx| s.select(selection, cx));
    }

    /// Follows the shell selection (the object browser opens queries here).
    fn follow_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selection = self.shell.read(cx).selection.clone();
        if selection == self.last_selection {
            return;
        }
        self.last_selection = selection.clone();
        match selection {
            // "New query" starts a draft; the unsaved draft's own row just shows it again.
            Some(Selection {
                kind: SelectionKind::NewQuery,
                id,
            }) if self.draft.as_ref().is_none_or(|d| !d.is_new || d.query.id != id) => {
                self.new_query(window, cx);
            }
            Some(Selection {
                kind: SelectionKind::Query,
                id,
            }) if self.draft.as_ref().is_none_or(|d| d.query.id != id) => {
                if let Some(query) = self.queries(cx).iter().find(|q| q.id == id).cloned() {
                    self.guard(window, cx, move |this, _, cx| this.open(query, false, cx));
                }
            }
            _ => {}
        }
    }

    /// Keeps a clean draft in step with outside edits (undo/redo, YAML).
    fn follow_config(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some((id, baseline)) = self
            .draft
            .as_ref()
            .filter(|d| !d.is_new && !d.dirty())
            .map(|d| (d.query.id.clone(), d.baseline.clone()))
        else {
            return;
        };
        let saved = self.queries(cx).iter().find(|q| q.id == id).cloned();
        match saved {
            None => self.close_draft(cx),
            Some(saved) if saved != baseline => self.open(saved, false, cx),
            Some(_) => {}
        }
    }

    // ---- Save and list operations ------------------------------------------------

    fn store(&self, cx: &App) -> Entity<crate::ui::store::ConfigStore> {
        self.shell.read(cx).store.clone()
    }

    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = &self.draft else {
            return;
        };
        let query = d.query.clone();
        self.error.clear();
        let name = query.name.trim().to_string();
        if name.is_empty() {
            self.error = "Give the query a name".into();
            return cx.notify();
        }
        let taken = self
            .queries(cx)
            .iter()
            .any(|q| q.id != query.id && q.name.to_lowercase() == name.to_lowercase());
        if taken {
            self.error = format!("Another query is already named {name}");
            return cx.notify();
        }
        // An unfinished builder model may be saved (Problems lists it); SQL must be
        // one read-only statement DuckDB can prepare.
        let (sql, compile_error) = sql::effective_sql(&query);
        if query.action.as_ref().is_some_and(|a| a.table.is_empty()) {
            self.error = "Choose the table this action query changes".into();
            return cx.notify();
        }
        if let Some(d) = &mut self.draft {
            d.saving = true;
        }
        let label = self.label.clone();
        let (action, builder) = (
            query.action.clone(),
            query.builder.as_ref().is_some_and(|b| !b.is_null()),
        );
        let check_sql = sql.clone();
        task::spawn_call_in(
            window,
            cx,
            move || -> Result<(), crate::manager::AppError> {
                if let Some(action) = action {
                    crate::queries::action::check_action_query_sql(label, check_sql, action)
                } else if !builder {
                    tauri::async_runtime::block_on(crate::queries::check_query_sql(label, check_sql))
                        .map(|_| ())
                } else {
                    Ok(())
                }
            },
            move |this, checked, _, cx| {
                if let Some(d) = &mut this.draft {
                    d.saving = false;
                }
                if let Err(e) = checked {
                    this.error = e.message;
                    return cx.notify();
                }
                let saved = SavedQuery {
                    name: name.clone(),
                    sql: if compile_error.is_empty() { sql } else { String::new() },
                    ..query
                };
                let stored = saved.clone();
                let result = this.store(cx).update(cx, |s, cx| {
                    s.update(
                        &format!("Save query {name}"),
                        |c| match c.saved_queries.iter_mut().find(|q| q.id == stored.id) {
                            Some(q) => *q = stored,
                            None => c.saved_queries.push(stored),
                        },
                        cx,
                    )
                });
                match result {
                    Ok(_) => {
                        let id = saved.id.clone();
                        this.open(saved, false, cx);
                        this.select_in_shell(Some(id), cx);
                    }
                    Err(e) => {
                        this.error = e.message;
                        cx.notify();
                    }
                }
            },
        );
        cx.notify();
    }

    pub fn duplicate(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(source) = self.queries(cx).iter().find(|q| q.id == id).cloned() else {
            return;
        };
        self.guard(window, cx, move |this, _, cx| {
            let copy = SavedQuery {
                id: new_id(),
                name: unique_name(&format!("{} copy", source.name), this.queries(cx)),
                ..source.clone()
            };
            let (stored, name) = (copy.clone(), source.name.clone());
            let result = this.store(cx).update(cx, |s, cx| {
                s.update(
                    &format!("Duplicate query {name}"),
                    |c| c.saved_queries.push(stored),
                    cx,
                )
            });
            match result {
                Ok(_) => {
                    let id = copy.id.clone();
                    this.open(copy, false, cx);
                    this.select_in_shell(Some(id), cx);
                }
                Err(e) => {
                    this.error = e.message;
                    cx.notify();
                }
            }
        });
    }

    pub fn delete(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(query) = self.queries(cx).iter().find(|q| q.id == id).cloned() else {
            return;
        };
        let answer = dialogs::confirm(cx, "Delete query", &format!("Delete query {}?", query.name));
        task::after_in(window, cx, answer, move |this, ok, _, cx| {
            if !ok {
                return;
            }
            let (target, name) = (query.id.clone(), query.name.clone());
            let result = this.store(cx).update(cx, |s, cx| {
                s.update(
                    &format!("Delete query {name}"),
                    |c| c.saved_queries.retain(|q| q.id != target),
                    cx,
                )
            });
            match result {
                Ok(_) => {
                    if this.draft.as_ref().is_some_and(|d| d.query.id == query.id) {
                        this.close_draft(cx);
                    }
                    if this.last_selection.as_ref().is_some_and(|s| s.id == query.id) {
                        this.select_in_shell(None, cx);
                    }
                }
                Err(e) => this.error = e.message,
            }
            cx.notify();
        });
    }
}

/// A bordered group of controls under a heading (`<id>-heading`).
pub(crate) fn panel(id: &'static str, title: &'static str, cx: &App) -> Div {
    v_flex()
        .gap_2()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .child(widgets::styled_text(
            SharedString::from(format!("{id}-heading")),
            title,
            |d| d.text_sm().font_weight(FontWeight::SEMIBOLD),
        ))
}

/// `base`, or `base 2`, `base 3`... when a query already has the name.
pub fn unique_name(base: &str, queries: &[SavedQuery]) -> String {
    let taken = |name: &str| queries.iter().any(|q| q.name.to_lowercase() == name.to_lowercase());
    if !taken(base) {
        return base.to_string();
    }
    let mut n = 2;
    while taken(&format!("{base} {n}")) {
        n += 1;
    }
    format!("{base} {n}")
}

impl Render for QueryMode {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_sql_editor(window, cx);
        let pane = match self.draft.is_some() {
            true => self.render_editor(window, cx).into_any_element(),
            false => v_flex()
                .items_center()
                .child(widgets::empty_state(
                    "query-empty",
                    "Select or create a query",
                    "Build a query visually, write read-only SQL, or write an action query \
                     that changes rows.",
                    cx,
                ))
                .child(
                    Button::new("create-query")
                        .primary()
                        .label("Create a query")
                        .on_click(cx.listener(|this, _, window, cx| this.new_query(window, cx))),
                )
                .into_any_element(),
        };
        let error = (!self.error.is_empty()).then(|| self.error_line("query-error", &self.error, cx));
        widgets::observed(
            v_flex()
                .id("query-pane")
                .aria_label("Query pane")
                .size_full()
                .overflow_y_scroll()
                .p_3()
                .gap_3()
                .children(error)
                .child(pane),
        )
    }
}
