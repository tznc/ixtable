//! The Data page (port of `src/data/DatabaseWorkbench.tsx`): the relationship diagram, and
//! below it the selected table's datasheet, its design form or the new-table form. A
//! saved or new query chosen in the object browser fills the workspace with the query
//! editor (`ui::query::QueryMode`), which is kept while other objects are shown so its
//! draft survives.
//!
//! The workbench owns what the datasheet reads with (offset, sorts, the first-column
//! search and the filters applied from the sheet) and pushes each freshly read page into
//! the `Datasheet`. It reloads the schemas and the page when the document announces a
//! database or record change, dropping answers that a newer request has overtaken.
//!
//! After a table is created the workbench shows the Design module's `CreateFormsOffer` over
//! that table's datasheet until it is dismissed or the selection moves.
//!
//! Test ids: `relationship-title` ("N tables · M relationships"), `import-records`,
//! `new-relationship`, `table-name`, `read-only-badge`, `record-count`, `Filter first
//! column` (the filter input; labelled "Filter records"), `Refresh`, `design-table`, `data-error`, `select-table` (empty state),
//! `design-loading`, `design-refreshing`, `footer-range`, `prev-page`, `next-page`,
//! `query-table` ("Query this table").

use crate::ui::controls::{input_text, new_input, set_input, text_input};
use super::{
    create_table::{CreateTableEvent, CreateTableForm},
    designer::{DesignerEvent, TableSchemaDesigner},
    diagram::{read_positions, write_positions, DiagramEvent, RelationshipBrowser},
    relate::{RelateDialog, RelateEvent},
    relationships::DrawnRelationship,
    sheet::{
        filters::SheetFilter,
        grid::{Datasheet, SheetEvent},
    },
};
use crate::data::{DbObject, Filter, Page, Sort, TableSchema};
use crate::ui::design::{CreateFormsOffer, OfferEvent};
use crate::ui::query::QueryMode;
use crate::manager::AppError;
use crate::recordstore::capabilities::StoreCapabilities;
use crate::ui::{
    events::DocEvent,
    export::{
        menu::{table_export, ExportMenu},
        names::first_column_filter,
    },
    import::wizard::{ImportEvent, ImportWizard},
    prelude::*,
    schema::{
        constraints::TableRef,
    },
    shell::{Selection, SelectionKind, Shell},
    store::StoreEvent,
};
use gpui_kit::component::input::{InputEvent, InputState};

pub const PAGE_SIZE: u64 = 100;

pub struct DataMode {
    shell: Entity<Shell>,
    capabilities: Option<StoreCapabilities>,
    schemas: Vec<TableSchema>,
    schemas_fresh: bool,
    schema_generation: u64,
    seen_objects: Vec<(String, String)>,
    seen_selection: Option<Selection>,
    diagram: Entity<RelationshipBrowser>,
    // Whether the diagram stays open while a table is open; kept for the session.
    diagram_expanded: bool,
    sheet: Option<Entity<Datasheet>>,
    page: Option<Page>,
    // Records in the table with no filter applied; set only while a filter is active.
    unfiltered_total: Option<u64>,
    page_generation: u64,
    offset: u64,
    sorts: Vec<Sort>,
    revision: u64,
    filter: Entity<InputState>,
    applied: (String, Vec<SheetFilter>),
    error: Option<AppError>,
    designing: bool,
    designer: Option<Entity<TableSchemaDesigner>>,
    designer_key: String,
    create_form: Option<Entity<CreateTableForm>>,
    // The table just created and the offer to build its forms.
    created: Option<(String, Entity<CreateFormsOffer>)>,
    relating: Option<Entity<RelateDialog>>,
    importing: Option<Entity<ImportWizard>>,
    export: Entity<ExportMenu>,
    query: Option<Entity<QueryMode>>,
    _subscriptions: Vec<Subscription>,
    _child_subscriptions: Vec<Subscription>,
}

fn schema_key(schema: &TableSchema) -> String {
    serde_json::to_string(schema).unwrap_or_default()
}

impl DataMode {
    pub fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = new_input(window, cx, "Filter records", "");
        let diagram = cx.new(|_| RelationshipBrowser::new());
        // The real runner (with the window label) is set once a table is chosen.
        let runner = crate::ui::export::menu::table_runner(String::new(), String::new(), vec![], vec![]);
        let export = cx.new(|_| ExportMenu::new("", runner, shell.clone()));
        let subscriptions = vec![
            cx.observe_in(&shell, window, |this, _, window, cx| this.sync(window, cx)),
            cx.subscribe_in(&filter, window, |this, _, e: &InputEvent, window, cx| {
                if matches!(e, InputEvent::Change) {
                    this.offset = 0;
                    this.load_page(window, cx);
                    cx.notify();
                }
            }),
            cx.subscribe_in(&diagram, window, |this, _, e: &DiagramEvent, window, cx| match e {
                DiagramEvent::Select(name) => {
                    this.offset = 0;
                    let selection = Selection {
                        kind: SelectionKind::Table,
                        id: name.clone(),
                    };
                    this.shell.update(cx, |s, cx| s.select(Some(selection), cx));
                    this.sync(window, cx);
                }
                DiagramEvent::Arrange(positions) => {
                    let positions = positions.clone();
                    let store = this.shell.read(cx).store.clone();
                    let result = store.update(cx, |s, cx| {
                        s.update("Arrange relationship diagram", |c| write_positions(c, &positions), cx)
                    });
                    if let Err(e) = result {
                        this.error = Some(e);
                        cx.notify();
                    }
                }
                DiagramEvent::Relate(drawn) => this.open_relate(window, Some(drawn.clone()), cx),
            }),
        ];
        let this = Self {
            shell,
            capabilities: None,
            schemas: vec![],
            schemas_fresh: false,
            schema_generation: 0,
            seen_objects: vec![],
            seen_selection: None,
            diagram,
            diagram_expanded: false,
            sheet: None,
            page: None,
            unfiltered_total: None,
            page_generation: 0,
            offset: 0,
            sorts: vec![],
            revision: 0,
            filter,
            applied: (String::new(), vec![]),
            error: None,
            designing: false,
            designer: None,
            designer_key: String::new(),
            create_form: None,
            created: None,
            relating: None,
            importing: None,
            export,
            query: None,
            _subscriptions: subscriptions,
            _child_subscriptions: vec![],
        };
        // The shell is being rendered while this view is built, so it cannot be read yet.
        cx.defer_in(window, |this, window, cx| this.start(window, cx));
        this
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bus = self.shell.read(cx).bus.clone();
        let store = self.shell.read(cx).store.clone();
        let subscriptions = [
            cx.subscribe_in(&bus, window, |this, _, e: &DocEvent, window, cx| match e {
                DocEvent::DatabaseChanged => {
                    this.reload_schemas(window, cx);
                    this.refresh(window, cx);
                }
                DocEvent::RecordsChanged => this.refresh(window, cx),
            }),
            cx.subscribe(&store, |this, _, e: &StoreEvent, cx| {
                if matches!(e, StoreEvent::Changed) {
                    this.update_diagram(cx);
                }
            }),
        ];
        self._subscriptions.extend(subscriptions);
        self.sync(window, cx);
        self.reload_schemas(window, cx);
    }

    // ---- State derived from the shell ------------------------------------------------

    /// The table or view chosen in the object browser ("" when none).
    pub fn selected(&self, cx: &App) -> String {
        match &self.shell.read(cx).selection {
            Some(Selection { kind: SelectionKind::Table | SelectionKind::View, id }) => id.clone(),
            _ => String::new(),
        }
    }

    fn read_only(&self, cx: &App) -> bool {
        matches!(&self.shell.read(cx).selection, Some(Selection { kind: SelectionKind::View, .. }))
    }

    /// A saved or new query is chosen.
    fn querying(&self, cx: &App) -> bool {
        matches!(
            &self.shell.read(cx).selection,
            Some(Selection { kind: SelectionKind::Query | SelectionKind::NewQuery, .. })
        )
    }

    /// The query editor, once a query has been chosen.
    pub fn query_view(&self) -> Option<&Entity<QueryMode>> {
        self.query.as_ref()
    }

    /// Builds the query editor the first time a query is chosen; it follows the shell
    /// selection from then on.
    fn ensure_query(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<QueryMode> {
        if let Some(query) = &self.query {
            return query.clone();
        }
        let shell = self.shell.clone();
        let query = cx.new(|cx| QueryMode::new(shell, window, cx));
        self.query = Some(query.clone());
        query
    }

    fn query_table(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let table = self.selected(cx);
        let query = self.ensure_query(window, cx);
        query.update(cx, |q, cx| q.new_query_from(&table, window, cx));
    }

    fn creating(&self, cx: &App) -> bool {
        matches!(&self.shell.read(cx).selection, Some(Selection { kind: SelectionKind::NewTable, .. }))
    }

    pub fn page(&self) -> Option<&Page> {
        self.page.as_ref()
    }

    pub fn sheet(&self) -> Option<&Entity<Datasheet>> {
        self.sheet.as_ref()
    }

    pub fn designer(&self) -> Option<&Entity<TableSchemaDesigner>> {
        self.designer.as_ref()
    }

    pub fn schemas(&self) -> &[TableSchema] {
        &self.schemas
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn is_designing(&self) -> bool {
        self.designing
    }

    /// Reacts to the shell: other objects, or another selection.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (objects, selection) = {
            let shell = self.shell.read(cx);
            let objects: Vec<(String, String)> = shell
                .objects
                .iter()
                .filter(|o| o.object_type == "table")
                .map(|o| (o.name.clone(), o.object_type.clone()))
                .collect();
            (objects, shell.selection.clone())
        };
        if objects != self.seen_objects {
            self.seen_objects = objects;
            self.reload_schemas(window, cx);
        }
        if self.querying(cx) {
            self.ensure_query(window, cx);
        }
        if selection != self.seen_selection {
            let table_changed = self.table_of(&selection) != self.table_of(&self.seen_selection);
            self.seen_selection = selection;
            self.designing = false;
            self.designer = None;
            self.designer_key.clear();
            if table_changed {
                self.offset = 0;
                self.sorts.clear();
                self.error = None;
                self.page = None;
                set_input(&self.filter, "", window, cx);
            }
            self.sync_create_form(window, cx);
            self.update_diagram(cx);
            if table_changed {
                self.load_page(window, cx);
            }
        }
        cx.notify();
    }

    fn table_of(&self, selection: &Option<Selection>) -> String {
        match selection {
            Some(Selection { kind: SelectionKind::Table | SelectionKind::View, id }) => id.clone(),
            _ => String::new(),
        }
    }

    // ---- Schemas ----------------------------------------------------------------------

    fn reload_schemas(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.schema_generation += 1;
        self.schemas_fresh = false;
        self.lock_designer(true, cx);
        let generation = self.schema_generation;
        let label = self.shell.read(cx).label().to_string();
        let names: Vec<String> = self
            .shell
            .read(cx)
            .objects
            .iter()
            .filter(|o: &&DbObject| o.object_type == "table")
            .map(|o| o.name.clone())
            .collect();
        {
            let label = label.clone();
            task::spawn_call(
                cx,
                move || crate::recordstore::commands::store_capabilities(label),
                move |this, result, cx| {
                    this.capabilities = result.ok();
                    this.push_capabilities(cx);
                    cx.notify();
                },
            );
        }
        task::spawn_call_in(
            window,
            cx,
            move || {
                names
                    .into_iter()
                    .map(|name| crate::inspect_table(label.clone(), name, None))
                    .collect::<Result<Vec<_>, _>>()
            },
            move |this, result, window, cx| {
                if this.schema_generation != generation {
                    return;
                }
                match result {
                    Ok(schemas) => {
                        this.schemas = schemas;
                        this.schemas_fresh = true;
                        this.lock_designer(false, cx);
                        this.update_diagram(cx);
                        this.push_tables(cx);
                        this.sync_designer(window, cx);
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
    }

    fn lock_designer(&mut self, locked: bool, cx: &mut Context<Self>) {
        if let Some(designer) = &self.designer {
            designer.update(cx, |d, cx| d.set_locked(locked, cx));
        }
    }

    #[cfg(test)]
    pub fn diagram(&self) -> &Entity<RelationshipBrowser> {
        &self.diagram
    }

    pub fn relating(&self) -> Option<&Entity<RelateDialog>> {
        self.relating.as_ref()
    }

    pub fn schemas_fresh(&self) -> bool {
        self.schemas_fresh
    }

    fn push_capabilities(&mut self, cx: &mut Context<Self>) {
        let caps = self.capabilities.clone();
        if let Some(form) = &self.create_form {
            form.update(cx, |f, cx| f.set_capabilities(caps.clone(), cx));
        }
        if let Some(designer) = &self.designer {
            designer.update(cx, |d, cx| d.set_capabilities(caps, cx));
        }
    }

    fn push_tables(&mut self, cx: &mut Context<Self>) {
        let refs: Vec<TableRef> = self.schemas.iter().map(TableRef::of).collect();
        if let Some(form) = &self.create_form {
            form.update(cx, |f, cx| f.set_tables(refs, cx));
        }
        if let Some(designer) = &self.designer {
            let schemas = self.schemas.clone();
            designer.update(cx, |d, cx| d.set_tables(&schemas, cx));
        }
    }

    fn update_diagram(&mut self, cx: &mut Context<Self>) {
        let selected = self.selected(cx);
        let positions = read_positions(self.shell.read(cx).config(cx));
        let schemas = self.schemas.clone();
        self.diagram.update(cx, |d, cx| d.set_data(schemas, &selected, positions, cx));
    }

    // ---- Designer and new-table form --------------------------------------------------

    fn open_designer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.designing = true;
        self.sync_designer(window, cx);
        cx.notify();
    }

    /// Opens the designer once the schemas match the object list; once open it stays
    /// through a reload and is rebuilt only when the reloaded schema differs, so a reload
    /// that changes nothing keeps its drafts.
    fn sync_designer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selected(cx);
        if !self.designing || selected.is_empty() {
            self.designer = None;
            self.designer_key.clear();
            return;
        }
        if !self.schemas_fresh {
            return;
        }
        let Some(schema) = self.schemas.iter().find(|s| s.name == selected).cloned() else {
            return;
        };
        let key = schema_key(&schema);
        if self.designer.is_some() && self.designer_key == key {
            return;
        }
        let (shell, tables, caps) = (self.shell.clone(), self.schemas.clone(), self.capabilities.clone());
        let designer = cx.new(|cx| TableSchemaDesigner::new(shell, schema, tables, caps, window, cx));
        let subscription = cx.subscribe_in(&designer, window, |this, _, e: &DesignerEvent, window, cx| match e {
            DesignerEvent::Changed(name) => this.after_schema_change(Some(name.clone()), window, cx),
            DesignerEvent::Dropped => this.after_schema_change(None, window, cx),
            DesignerEvent::Cancel => {
                this.designing = false;
                this.designer = None;
                this.designer_key.clear();
                cx.notify();
            }
        });
        self._child_subscriptions.push(subscription);
        self.designer = Some(designer);
        self.designer_key = key;
    }

    fn sync_create_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.creating(cx) {
            self.create_form = None;
            return;
        }
        if self.create_form.is_some() {
            return;
        }
        let refs: Vec<TableRef> = self.schemas.iter().map(TableRef::of).collect();
        let (shell, caps) = (self.shell.clone(), self.capabilities.clone());
        let form = cx.new(|cx| CreateTableForm::new(shell, refs, caps, window, cx));
        let subscription = cx.subscribe_in(&form, window, |this, _, e: &CreateTableEvent, window, cx| match e {
            CreateTableEvent::Created(name) => {
                this.offer_forms(name, window, cx);
                this.after_schema_change(Some(name.clone()), window, cx)
            }
            CreateTableEvent::Cancelled => {
                this.shell.update(cx, |s, cx| s.select(None, cx));
            }
        });
        self._child_subscriptions.push(subscription);
        self.create_form = Some(form);
    }

    fn offer_forms(&mut self, table: &str, window: &mut Window, cx: &mut Context<Self>) {
        let shell = self.shell.clone();
        let offer = cx.new(|cx| CreateFormsOffer::new(shell, table, cx));
        let subscription = cx.subscribe_in(&offer, window, |this, _, _: &OfferEvent, _, cx| {
            this.created = None;
            cx.notify();
        });
        self._child_subscriptions.push(subscription);
        self.created = Some((table.to_string(), offer));
    }

    /// After DDL: entities may have changed in the backend config, and metadata is stale.
    /// The config reload adds no undo step: undo cannot revert the schema, so it must not
    /// revert the config that describes it either.
    fn after_schema_change(&mut self, table: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let store = self.shell.read(cx).store.clone();
        let reloaded = store.update(cx, |s, cx| s.reload(None, cx).map(|_| ()));
        if let Err(e) = reloaded {
            self.error = Some(e);
        }
        self.designing = false;
        self.designer = None;
        self.designer_key.clear();
        self.offset = 0;
        self.revision += 1;
        self.shell.update(cx, |shell, cx| {
            shell.mark_dirty(cx);
            shell.select(
                table.map(|id| Selection {
                    kind: SelectionKind::Table,
                    id,
                }),
                cx,
            );
            shell.database_changed(cx);
        });
        self.sync(window, cx);
    }

    // ---- Dialogs ----------------------------------------------------------------------

    pub fn open_relate(&mut self, window: &mut Window, drawn: Option<DrawnRelationship>, cx: &mut Context<Self>) {
        let selected = self.selected(cx);
        let child = if !selected.is_empty() && self.schemas.iter().any(|s| s.name == selected) {
            selected
        } else {
            self.schemas.first().map(|s| s.name.clone()).unwrap_or_default()
        };
        let (shell, schemas, caps) = (self.shell.clone(), self.schemas.clone(), self.capabilities.clone());
        let dialog = cx.new(|cx| RelateDialog::new(shell, schemas, caps, drawn, child, cx));
        let subscription = cx.subscribe_in(&dialog, window, |this, _, e: &RelateEvent, window, cx| match e {
            RelateEvent::Cancel => {
                this.relating = None;
                cx.notify();
            }
            RelateEvent::Applied(table) => {
                this.relating = None;
                this.after_schema_change(Some(table.clone()), window, cx);
            }
        });
        self._child_subscriptions.push(subscription);
        self.relating = Some(dialog);
        cx.notify();
    }

    pub fn open_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tables: Vec<TableSchema> = self.schemas.iter().filter(|s| s.object_type != "view").cloned().collect();
        let shell = self.shell.clone();
        let wizard = cx.new(|cx| ImportWizard::new(shell, tables, window, cx));
        let subscription = cx.subscribe_in(&wizard, window, |this, _, e: &ImportEvent, window, cx| match e {
            ImportEvent::Close => {
                this.importing = None;
                cx.notify();
            }
            ImportEvent::Imported(table) => {
                let current = this.selected(cx);
                let target = table.clone().or_else(|| {
                    (this.shell.read(cx).selection.as_ref().is_some_and(|s| s.kind == SelectionKind::Table))
                        .then_some(current)
                });
                this.after_schema_change(target, window, cx);
            }
        });
        self._child_subscriptions.push(subscription);
        self.importing = Some(wizard);
        cx.notify();
    }

    // ---- Reading the page -------------------------------------------------------------

    fn sheet_filters(&self, selected: &str) -> Vec<SheetFilter> {
        if self.applied.0 == selected {
            self.applied.1.clone()
        } else {
            vec![]
        }
    }

    fn read_filters(&self, selected: &str, cx: &App) -> Vec<Filter> {
        let first = self.page.as_ref().and_then(|p| p.columns.first()).map(|c| c.name.as_str());
        let mut filters = first_column_filter(first, &input_text(&self.filter, cx));
        filters.extend(self.sheet_filters(selected).iter().map(SheetFilter::to_backend));
        filters
    }

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.revision += 1;
        self.load_page(window, cx);
    }

    fn load_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selected(cx);
        self.page_generation += 1;
        if selected.is_empty() {
            self.page = None;
            return;
        }
        let generation = self.page_generation;
        let label = self.shell.read(cx).label().to_string();
        let (offset, sorts) = (self.offset, self.sorts.clone());
        let filters = self.read_filters(&selected, cx);
        let read_only = self.read_only(cx);
        let table = selected.clone();
        let (work_sorts, work_filters) = (sorts.clone(), filters.clone());
        task::spawn_call_in(
            window,
            cx,
            move || {
                let filtered = !work_filters.is_empty();
                let page = crate::read_table_page(label.clone(), selected.clone(), offset, PAGE_SIZE, work_sorts, work_filters, None)?;
                // The unfiltered total comes from a one-row page of the same table.
                let total = if filtered {
                    crate::read_table_page(label, selected, 0, 1, vec![], vec![], None).ok().map(|p| p.total)
                } else {
                    None
                };
                Ok((page, total))
            },
            move |this, result: Result<(Page, Option<u64>), AppError>, window, cx| {
                if this.page_generation != generation {
                    return;
                }
                match result {
                    Ok((page, total)) => {
                        this.unfiltered_total = total;
                        this.show_page(table, page, sorts, filters, read_only, window, cx)
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn show_page(
        &mut self,
        table: String,
        page: Page,
        sorts: Vec<Sort>,
        filters: Vec<Filter>,
        read_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let applied = self.sheet_filters(&table);
        let revision = self.revision;
        self.page = Some(page.clone());
        match &self.sheet {
            Some(sheet) => {
                sheet.update(cx, |s, cx| {
                    s.set_page(table.clone(), page, revision, sorts.clone(), filters.clone(), applied, read_only, window, cx)
                });
            }
            None => self.create_sheet(table.clone(), page, sorts.clone(), filters.clone(), applied, read_only, window, cx),
        }
        let (name, runner) = table_export(&self.shell, &table, sorts, filters, cx);
        self.export.update(cx, |e, cx| e.set_target(&name, runner, cx));
    }

    #[allow(clippy::too_many_arguments)]
    fn create_sheet(
        &mut self,
        table: String,
        page: Page,
        sorts: Vec<Sort>,
        filters: Vec<Filter>,
        applied: Vec<SheetFilter>,
        read_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shell = self.shell.clone();
        let revision = self.revision;
        let sheet = cx.new(|cx| {
            let mut sheet = Datasheet::new(shell, table.clone(), page.clone(), PAGE_SIZE, read_only, window, cx);
            sheet.set_page(table, page, revision, sorts, filters, applied, read_only, window, cx);
            sheet
        });
        let subscription = cx.subscribe_in(&sheet, window, |this, _, e: &SheetEvent, window, cx| match e {
            SheetEvent::Written => {
                this.shell.update(cx, |s, cx| {
                    s.mark_dirty(cx);
                    s.records_changed(cx);
                });
            }
            SheetEvent::Refresh => this.refresh(window, cx),
            SheetEvent::Error(message) => {
                this.error = Some(AppError::new("", message));
                cx.notify();
            }
            SheetEvent::Sorts(sorts) => {
                this.sorts = sorts.clone();
                this.load_page(window, cx);
            }
            SheetEvent::Filters(filters) => {
                let selected = this.selected(cx);
                this.applied = (selected, filters.clone());
                this.offset = 0;
                this.load_page(window, cx);
            }
            SheetEvent::Offset(offset) => {
                this.offset = *offset;
                this.load_page(window, cx);
            }
        });
        self._child_subscriptions.push(subscription);
        self.sheet = Some(sheet);
    }

    fn go_to(&mut self, offset: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.offset = offset;
        self.load_page(window, cx);
        cx.notify();
    }
}

/// The count line: "6 records", or "3 of 6 records (filtered)" while a filter hides some.
fn record_count_text(shown: Option<u64>, unfiltered: Option<u64>) -> String {
    match (shown, unfiltered) {
        (None, _) => "\u{2014} records".to_string(),
        (Some(t), Some(all)) => format!("{t} of {} (filtered)", widgets::plural(all as usize, "record")),
        (Some(t), None) => widgets::plural(t as usize, "record"),
    }
}

impl Render for DataMode {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(query) = self.query.clone().filter(|_| self.querying(cx)) {
            return div().size_full().child(query).into_any_element();
        }
        let selected = self.selected(cx);
        let creating = self.creating(cx);
        let read_only = self.read_only(cx);
        let inspector_visible = creating || !selected.is_empty();
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        let relationships: usize = self.schemas.iter().map(|s| s.foreign_keys.len()).sum();
        let title = format!(
            "{} \u{b7} {}",
            widgets::plural(self.schemas.len(), "table"),
            widgets::plural(relationships, "relationship")
        );
        // With a table open the diagram collapses to its header so the datasheet gets the space.
        let diagram_open = !inspector_visible || self.diagram_expanded;
        let relationship_pane = v_flex()
            .id("relationship-pane")
            .when(inspector_visible && diagram_open, |el| el.h(px(300.)).flex_none())
            .when(inspector_visible && !diagram_open, |el| el.flex_none())
            .when(!inspector_visible, |el| el.flex_1())
            .min_h_0()
            .gap_2()
            .p_3()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .when(inspector_visible, |el| {
                        el.child(
                            Button::new("relationship-toggle")
                                .ghost()
                                .xsmall()
                                .icon(if diagram_open { IconName::ChevronDown } else { IconName::ChevronRight })
                                .tooltip(if diagram_open { "Collapse the diagram" } else { "Expand the diagram" })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.diagram_expanded = !this.diagram_expanded;
                                    cx.notify();
                                })),
                        )
                    })
                    .child(div().ty(Type::Section).child("Relationships"))
                    .child(div().ty(Type::Body).text_color(muted).child(widgets::text("relationship-title", title)))
                    .child(div().flex_1())
                    .child(
                        Button::new("import-records")
                            .outline()
                            .small()
                            .icon(IconName::FolderOpen)
                            .label("Import records")
                            .on_click(cx.listener(|this, _, window, cx| this.open_import(window, cx))),
                    )
                    .when(!self.schemas.is_empty(), |el| {
                        el.child(
                            Button::new("new-relationship")
                                .outline()
                                .small()
                                .icon(IconName::Plus)
                                .label("New relationship")
                                .on_click(cx.listener(|this, _, window, cx| this.open_relate(window, None, cx))),
                        )
                    }),
            )
            .when(diagram_open, |el| el.child(div().flex_1().min_h_0().child(self.diagram.clone())));

        let page_total = self.page.as_ref().map(|p| p.total);
        let page_rows = self.page.as_ref().map_or(0, |p| p.rows.len() as u64);
        let offset = self.offset;
        let data_pane: Option<AnyElement> = if !inspector_visible {
            None
        } else if self.designing && self.designer.is_none() {
            Some(
                div()
                    .p_4()
                    .child(widgets::text("design-loading", "Loading table design…"))
                    .into_any_element(),
            )
        } else if self.designing && self.designer.is_some() {
            let designer = self.designer.clone().unwrap();
            Some(
                v_flex()
                    .size_full()
                    // Fixed-height slot so the designer does not jump when the notice comes and goes.
                    .child(div().h(px(24.)).flex_none().px_4().ty(Type::Body).text_color(muted).when(!self.schemas_fresh, |el| {
                        el.child(widgets::text("design-refreshing", "Refreshing schema…"))
                    }))
                    .child(div().flex_1().min_h_0().child(designer))
                    .into_any_element(),
            )
        } else if creating {
            self.create_form.clone().map(|f| div().size_full().child(f).into_any_element())
        } else {
            let entity = cx.entity();
            let range = match page_total {
                Some(total) if total > 0 => {
                    format!("{}\u{2013}{} of {}", offset + 1, (offset + page_rows).min(total), total)
                }
                _ => "0 records".to_string(),
            };
            let can_next = page_total.is_some_and(|t| offset + PAGE_SIZE < t);
            let (prev, next) = (entity.clone(), entity);
            let offer = self.created.as_ref().filter(|(table, _)| *table == selected).map(|(_, offer)| offer.clone());
            Some(
                v_flex()
                    .size_full()
                    .min_h_0()
                    .children(offer.map(|offer| div().px_3().pt_2().child(offer)))
                    .child(
                        h_flex()
                            .gap_3()
                            .items_center()
                            .px_3()
                            .py_2()
                            .flex_wrap()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(widgets::text("table-name", selected.clone())),
                            )
                            .when(read_only, |el| {
                                el.child(div().ty(Type::Body).text_color(muted).child(widgets::text("read-only-badge", "Read-only view")))
                            })
                            .child(div().ty(Type::Body).text_color(muted).child(widgets::text(
                                "record-count",
                                record_count_text(page_total, self.unfiltered_total),
                            )))
                            .child(div().w(px(220.)).child(text_input(&self.filter, "Filter first column", "Filter records")))
                            .child(
                                Button::new("Refresh")
                                    .outline()
                                    .small()
                                    .icon(IconName::RefreshCw)
                                    .label("Refresh")
                                    .on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx))),
                            )
                            .child(self.export.clone())
                            .child(
                                Button::new("query-table")
                                    .outline()
                                    .small()
                                    .icon(IconName::CodeXml)
                                    .label("Query this table")
                                    .on_click(cx.listener(|this, _, window, cx| this.query_table(window, cx))),
                            )
                            .when(!read_only, |el| {
                                el.child(
                                    Button::new("design-table")
                                        .outline()
                                        .small()
                                        .icon(IconName::Settings)
                                        .label("Design table")
                                        .on_click(cx.listener(|this, _, window, cx| this.open_designer(window, cx))),
                                )
                            }),
                    )
                    .when_some(self.error.as_ref(), |el, error| {
                        el.child(div().px_3().child(widgets::error_banner("data-error", error, None, cx)))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .when_some(self.sheet.clone().filter(|_| self.page.is_some()), |el, sheet| el.child(sheet)),
                    )
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .px_3()
                            .py_1()
                            .border_t_1()
                            .border_color(border)
                            .child(div().ty(Type::Body).child(widgets::text("footer-range", range)))
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(
                                        Button::new("prev-page")
                                            .ghost()
                                            .small()
                                            .label("\u{2039}")
                                            .tooltip("Previous page")
                                            .disabled(offset == 0)
                                            .on_click(move |_, window, cx| {
                                                prev.update(cx, |this, cx| {
                                                    let to = this.offset.saturating_sub(PAGE_SIZE);
                                                    this.go_to(to, window, cx)
                                                })
                                            }),
                                    )
                                    .child(
                                        Button::new("next-page")
                                            .ghost()
                                            .small()
                                            .label("\u{203a}")
                                            .tooltip("Next page")
                                            .disabled(!can_next)
                                            .on_click(move |_, window, cx| {
                                                next.update(cx, |this, cx| {
                                                    let to = this.offset + PAGE_SIZE;
                                                    this.go_to(to, window, cx)
                                                })
                                            }),
                                    ),
                            ),
                    )
                    .into_any_element(),
            )
        };
        div()
            .relative()
            .size_full()
            .child(
                v_flex()
                    .size_full()
                    .child(relationship_pane)
                    .when_some(data_pane, |el, pane| {
                        el.child(
                            div()
                                .id("data-pane")
                                .flex_1()
                                .min_h_0()
                                .border_t_1()
                                .border_color(border)
                                .child(pane)
                                .test_support(),
                        )
                    }),
            )
            .when_some(self.importing.clone(), |el, wizard| el.child(wizard))
            .when_some(self.relating.clone(), |el, dialog| el.child(dialog))
            .into_any_element()
    }
}
