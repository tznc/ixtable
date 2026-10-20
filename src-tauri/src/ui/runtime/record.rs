//! Detail, create and edit modes of a form (port of `RecordView.tsx`): loading, validation,
//! the save flow with form events, committed-write handling, delete, buttons and the notice
//! lines.
//!
//! Test ids: the region is `form:<title slug>` (labelled with the title: "New X", "Edit X"
//! or X), `record-title`, `edit`, `delete`, `close` (embedded detail), `cancel`, `save`
//! (Edit mode, "Save") and `create` (Create mode, "Create"), `record-loading`, the lines
//! `status:error` / `status:info` (load and save problems) and `notice:error` /
//! `notice:info` (the last action's message), and `form-errors` with `form-error:<n>`.
//! Whether Save/Create can be pressed is `can_save()`; Edit/Delete enabled is
//! `can_edit()` / `can_delete()`.

use super::{
    BodyKey, Notice, RuntimeEnv,
    body::{BodyContext, control_grid},
    host::{HostEvent, UiHost, context, run_action_job},
    related::{RelatedProps, related_list},
};
use crate::data::{DataValue, TableSchema};
use crate::design::{Control, ControlKind, Form, FormMode};
use crate::forms::actions::{ActionResult, NavigationTarget, NoticeTone, StateScope, run_action};
use crate::forms::data::{load_record, record_id_for, table_schema};
use crate::forms::events::{
    EventState, FormEvent, after_update_problem, event_action, events_to_raise, run_form_event, veto_message,
};
use crate::forms::fields::with_field_defaults;
use crate::forms::rbac::Operation;
use crate::forms::registry::is_designed_form;
use crate::forms::row_writes::{WriteOutcome, create_row, delete_row, update_row, values_to_write, write_message};
use crate::forms::state::{
    FormErrors, FormScope, default_record, enabled_controls, has_errors, validate_control, validate_form,
    visible_controls,
};
use crate::forms::values::{RecordValues, same_value};
use crate::manager::AppError;
use crate::ui::prelude::*;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

pub type NotifyFn = Rc<dyn Fn(String, NoticeTone, &mut App)>;

#[derive(Clone)]
pub struct RecordProps {
    pub env: RuntimeEnv,
    pub form: Form,
    /// Detail, Create or Edit.
    pub mode: FormMode,
    pub record_id: Option<Value>,
    /// Column values fixed by a parent record (a related list's foreign-key columns).
    pub link: Map<String, Value>,
    pub embedded: bool,
    pub depth: usize,
    pub params: Map<String, Value>,
    pub on_mode: Rc<dyn Fn(FormMode, Option<Value>, &mut App)>,
    pub on_close: Rc<dyn Fn(&mut App)>,
    pub on_navigate: Rc<dyn Fn(NavigationTarget, &mut App)>,
    pub on_notify: Option<NotifyFn>,
    pub on_dirty: Option<Rc<dyn Fn(bool, &mut App)>>,
    pub on_saved: Option<Rc<dyn Fn(RecordValues, &mut App)>>,
    /// A message to show on opening (the one the previous view of this form ended with).
    pub initial_notice: Option<Notice>,
    /// Receives the view's message just before it hands over to the next view of the form.
    pub on_carry: Option<Rc<dyn Fn(Notice, &mut App)>>,
    /// Event bookkeeping shared by the views of one form session (detail, edit and create of
    /// the same record do not raise on load or on current again).
    pub events: Option<Rc<std::cell::RefCell<EventState>>>,
}

/// Runs `f` once the current update has finished, so a callback may rebuild this view.
fn later(cx: &mut App, f: impl FnOnce(&mut App) + 'static) {
    cx.defer(f);
}

pub struct RecordView {
    props: RecordProps,
    form: Rc<Form>,
    table: Option<String>,
    schema: Option<Arc<TableSchema>>,
    record: RecordValues,
    original: RecordValues,
    identity: Option<Vec<DataValue>>,
    form_state: Map<String, Value>,
    errors: FormErrors,
    /// Load and save problems; reset whenever the record (re)loads.
    status: Option<Notice>,
    /// Result of the last button action; independent of record loads.
    notice: Option<Notice>,
    busy: bool,
    running: bool,
    loaded: bool,
    event_state: Rc<std::cell::RefCell<EventState>>,
    navigated: bool,
    ignore_refresh: bool,
    reported_dirty: bool,
    seq: u64,
    _observers: Vec<Subscription>,
}

fn error_notice(message: impl Into<String>) -> Notice {
    Notice { message: message.into(), tone: NoticeTone::Error }
}

impl RecordView {
    pub fn new(props: RecordProps, cx: &mut Context<Self>) -> Self {
        let entities = props.env.config(cx).entities.clone();
        let form = with_field_defaults(&props.form, &entities).unwrap_or_else(|| props.form.clone());
        let table = form
            .source
            .as_ref()
            .filter(|s| s.kind == crate::design::SourceKind::Table)
            .and_then(|s| s.table.clone())
            .filter(|t| !t.is_empty());
        let mut observers = vec![];
        if let Some(runtime) = props.env.runtime.clone() {
            observers.push(cx.observe(&runtime, |_, _, cx| cx.notify()));
        }
        let initial_notice = props.initial_notice.clone();
        let events = props.events.clone().unwrap_or_default();
        let mut view = Self {
            props,
            form: Rc::new(form),
            table,
            schema: None,
            record: RecordValues::new(),
            original: RecordValues::new(),
            identity: None,
            form_state: Map::new(),
            errors: FormErrors::default(),
            status: None,
            notice: initial_notice,
            busy: false,
            running: false,
            loaded: false,
            event_state: events,
            navigated: false,
            ignore_refresh: false,
            reported_dirty: false,
            seq: 0,
            _observers: observers,
        };
        view.load(cx);
        view
    }

    // ---- model --------------------------------------------------------------------

    pub fn record(&self) -> &RecordValues {
        &self.record
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn mode(&self) -> FormMode {
        self.props.mode
    }

    pub fn is_dirty(&self) -> bool {
        self.props.mode == FormMode::Edit
            && self.loaded
            && self.record.iter().any(|(k, v)| !same_value(v, self.original.get(k).unwrap_or(&Value::Null)))
    }

    fn ready(&self) -> bool {
        self.loaded
            && !self.running
            && !self.busy
            && (self.props.mode == FormMode::Create || self.table.is_none() || self.identity.is_some())
    }

    fn read_only_source(&self) -> bool {
        self.table.is_none()
    }

    fn read_only(&self) -> bool {
        self.props.mode == FormMode::Detail || self.read_only_source()
    }

    fn allowed(&self, op: Operation, cx: &App) -> bool {
        if self.read_only_source() {
            return false;
        }
        let modes = &self.form.modes;
        if op == Operation::Create && !modes.contains(&FormMode::Create) {
            return false;
        }
        if op == Operation::Update && !modes.contains(&FormMode::Edit) {
            return false;
        }
        let subject = if is_designed_form(self.props.env.config(cx), &self.form) {
            ("form", self.form.id.clone())
        } else {
            ("table", self.table.clone().unwrap_or_default())
        };
        self.props.env.can(subject.0, &subject.1, op, cx)
    }

    pub fn can_edit(&self, cx: &App) -> bool {
        self.props.mode == FormMode::Detail
            && self.allowed(Operation::Update, cx)
            && self.form.modes.contains(&FormMode::Edit)
            && self.identity.is_some()
    }

    pub fn can_delete(&self, cx: &App) -> bool {
        self.props.mode == FormMode::Detail && self.allowed(Operation::Delete, cx) && self.identity.is_some()
    }

    pub fn can_save(&self) -> bool {
        !self.read_only() && !self.busy && self.loaded
    }

    pub fn title(&self) -> String {
        match self.props.mode {
            FormMode::Create => format!("New {}", widgets::singular(&self.form.name).to_lowercase()),
            FormMode::Edit => format!("Edit {}", widgets::singular(&self.form.name).to_lowercase()),
            _ => self.form.name.clone(),
        }
    }

    /// Save/Create button text. Embedded editors name their record so nested pairs are distinguishable.
    pub fn submit_label(&self) -> String {
        let verb = if self.props.mode == FormMode::Create { "Create" } else { "Save" };
        if self.props.embedded {
            format!("{verb} {}", widgets::singular(&self.form.name).to_lowercase())
        } else {
            verb.to_string()
        }
    }

    fn scope(&self, cx: &App) -> FormScope {
        FormScope::new(self.record.clone(), self.form_state.clone(), self.props.env.app(cx))
    }

    // ---- loading ------------------------------------------------------------------

    fn load(&mut self, cx: &mut Context<Self>) {
        self.seq += 1;
        let seq = self.seq;
        self.status = None;
        self.errors = FormErrors::default();
        let label = self.props.env.label.to_string();
        let table = self.table.clone();
        match (self.props.mode, table) {
            (FormMode::Create, table) => {
                let mut initial = default_record(&self.form, &Map::new(), &self.props.env.app(cx));
                initial.extend(self.props.link.clone());
                self.record = initial.clone();
                self.original = initial;
                self.identity = None;
                self.loaded = true;
                if let Some(table) = table {
                    task::spawn_call(cx, move || table_schema(&label, &table), move |this, schema, cx| {
                        if this.seq == seq {
                            this.schema = schema.ok();
                            cx.notify();
                        }
                    });
                }
                self.after_load(cx);
            }
            (_, None) => {
                // A query-sourced form receives its whole row, never a key.
                let row = match &self.props.record_id {
                    Some(Value::Object(row)) => Some(row.clone()),
                    _ => None,
                };
                if self.props.record_id.is_some() && row.is_none() {
                    self.status = Some(error_notice("This form shows query rows, so it cannot open a record by id."));
                }
                self.record = row.unwrap_or_default();
                self.loaded = true;
                self.after_load(cx);
            }
            (_, Some(table)) => {
                let id = self.props.record_id.clone().unwrap_or(Value::Null);
                task::spawn_call(
                    cx,
                    move || {
                        let schema = table_schema(&label, &table);
                        let loaded = load_record(&label, &table, &id);
                        (schema, loaded)
                    },
                    move |this, (schema, loaded), cx| {
                        if this.seq != seq {
                            return;
                        }
                        this.schema = schema.ok();
                        match loaded {
                            Ok(Some(found)) => {
                                this.record = found.record.clone();
                                this.original = found.record;
                                this.identity = Some(found.identity);
                            }
                            Ok(None) => {
                                this.record = RecordValues::new();
                                this.original = RecordValues::new();
                                this.identity = None;
                                this.status = Some(error_notice("This record no longer exists."));
                            }
                            Err(e) => this.status = Some(error_notice(write_message(&e))),
                        }
                        this.loaded = true;
                        this.after_load(cx);
                        this.report_dirty(cx);
                        cx.notify();
                    },
                );
            }
        }
        cx.notify();
    }

    /// Reads the record again after an action changed it; the shown record stays on screen.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.load(cx);
    }

    fn report_dirty(&mut self, cx: &mut Context<Self>) {
        let dirty = self.is_dirty();
        if dirty != self.reported_dirty {
            self.reported_dirty = dirty;
            if let Some(on_dirty) = self.props.on_dirty.clone() {
                later(cx, move |cx| on_dirty(dirty, cx));
            }
        }
    }

    // ---- editing ------------------------------------------------------------------

    pub fn set_field(&mut self, column: &str, value: Value, cx: &mut Context<Self>) {
        self.record.insert(column.to_string(), value);
        self.report_dirty(cx);
        cx.notify();
    }

    fn blur(&mut self, control_id: &str, cx: &mut Context<Self>) {
        let Some(control) = self.form.controls.iter().find(|c| c.id == control_id).cloned() else { return };
        let Some(column) = control.binding.as_ref().map(|b| b.column.clone()).filter(|c| !c.is_empty()) else {
            return;
        };
        if self.read_only() {
            return;
        }
        let scope = self.scope(cx);
        let value = self.record.get(&column).cloned().unwrap_or(Value::Null);
        let problem = validate_control(&control, &value, &scope);
        self.errors.fields.retain(|(id, _)| id != control_id);
        if let Some(problem) = problem {
            self.errors.fields.push((control_id.to_string(), problem));
        }
        cx.notify();
    }

    // ---- messages -----------------------------------------------------------------

    /// Shows a message once: next to the form while it stays open, else with the related
    /// list an embedded form closes into, else on the page (an action navigated away).
    fn announce(&mut self, text: impl Into<String>, tone: NoticeTone, cx: &mut Context<Self>) {
        let text = text.into();
        if !self.navigated && !self.props.embedded {
            self.notice = Some(Notice { message: text, tone });
            cx.notify();
        } else if let Some(on_notify) = self.props.on_notify.clone() {
            later(cx, move |cx| on_notify(text, tone, cx));
        } else {
            let env = self.props.env.clone();
            later(cx, move |cx| env.notify(text, tone, cx));
        }
    }

    // ---- actions ------------------------------------------------------------------

    fn host(&self, cx: &App) -> UiHost {
        UiHost::new(
            self.props.env.config(cx).clone(),
            self.props.env.role_id(cx),
            self.props.env.attached(),
        )
    }

    fn on_host_event(&mut self, event: HostEvent, cx: &mut Context<Self>) {
        match event {
            HostEvent::Navigate(target) => {
                self.navigated = true;
                let on_navigate = self.props.on_navigate.clone();
                later(cx, move |cx| on_navigate(target, cx));
            }
            HostEvent::SetState(StateScope::Form, key, value) => {
                self.form_state.insert(key, value);
                cx.notify();
            }
            HostEvent::SetState(StateScope::App, key, value) => {
                let env = self.props.env.clone();
                later(cx, move |cx| env.set_app_state(&key, value, cx));
            }
            HostEvent::Notify(text, tone) => self.announce(text, tone, cx),
            HostEvent::Refresh => {
                if self.ignore_refresh {
                    return;
                }
                if self.props.mode == FormMode::Detail {
                    self.reload(cx);
                } else {
                    let (on_mode, mode, id) =
                        (self.props.on_mode.clone(), self.props.mode, self.props.record_id.clone());
                    later(cx, move |cx| on_mode(mode, id, cx));
                }
            }
            _ => {}
        }
    }

    /// The data an action run reads, built on the UI thread.
    fn action_data(&self, values: RecordValues, snapshot: Option<RecordValues>, cx: &App) -> ActionData {
        ActionData {
            config: self.props.env.config(cx).clone(),
            label: self.props.env.label.to_string(),
            record: values,
            form: self.form_state.clone(),
            app: self.props.env.app(cx),
            params: self.props.params.clone(),
            snapshot: snapshot.or_else(|| {
                (self.props.mode != FormMode::Create && self.table.is_some()).then(|| self.original.clone())
            }),
        }
    }

    /// Runs the action bound to a form event; `done` gets its result.
    fn run_event(
        &mut self,
        event: FormEvent,
        values: RecordValues,
        snapshot: Option<RecordValues>,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, ActionResult, &mut Context<Self>) + 'static,
    ) {
        self.navigated = false;
        let data = self.action_data(values, snapshot, cx);
        let form = (*self.form).clone();
        let host = self.host(cx);
        let runtime = self.props.env.runtime.clone();
        run_action_job(
            cx,
            host,
            runtime,
            move |host| {
                let ctx = data.into_context(host);
                run_form_event(&form, event, &ctx).unwrap_or_else(ActionResult::success)
            },
            Self::on_host_event,
            done,
        );
    }

    fn run_button(&mut self, control_id: &str, cx: &mut Context<Self>) {
        let Some(control) = self.form.controls.iter().find(|c| c.id == control_id).cloned() else { return };
        let Some(action_id) = control.action_id.clone().filter(|a| !a.is_empty()) else { return };
        if !self.ready() {
            return;
        }
        self.notice = None;
        self.running = true;
        self.navigated = false;
        let data = self.action_data(self.record.clone(), None, cx);
        let host = self.host(cx);
        let runtime = self.props.env.runtime.clone();
        run_action_job(
            cx,
            host,
            runtime,
            move |host| {
                let ctx = data.into_context(host);
                run_action(&action_id, &ctx)
            },
            Self::on_host_event,
            |this, result, cx| {
                this.running = false;
                if !result.ok {
                    if let Some(error) = result.error.filter(|e| !e.is_empty()) {
                        this.notice = Some(error_notice(error));
                    }
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    /// On load once per opened view, then on current each time it shows another record.
    fn after_load(&mut self, cx: &mut Context<Self>) {
        if !self.loaded || (self.table.is_some() && self.props.mode != FormMode::Create && self.identity.is_none()) {
            return;
        }
        let key = serde_json::json!([
            self.form.id,
            if self.props.mode == FormMode::Create { Value::from("new") } else { self.props.record_id.clone().unwrap_or(Value::Null) }
        ])
        .to_string();
        let events = events_to_raise(&mut self.event_state.borrow_mut(), &key);
        if !events.iter().any(|e| event_action(&self.form, *e).is_some()) {
            return;
        }
        self.running = true;
        self.raise_next(events, cx);
    }

    fn raise_next(&mut self, mut events: Vec<FormEvent>, cx: &mut Context<Self>) {
        while !events.is_empty() && event_action(&self.form, events[0]).is_none() {
            events.remove(0);
        }
        if events.is_empty() {
            self.running = false;
            cx.notify();
            return;
        }
        let event = events.remove(0);
        let record = self.record.clone();
        self.run_event(event, record, None, cx, move |this, result, cx| {
            if !result.ok {
                if let Some(error) = result.error.clone().filter(|e| !e.is_empty()) {
                    this.notice = Some(error_notice(error));
                }
                this.running = false;
                cx.notify();
                return;
            }
            this.raise_next(events, cx);
        });
    }

    // ---- save ---------------------------------------------------------------------

    pub fn save(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.read_only() {
            return;
        }
        let scope = self.scope(cx);
        let result = validate_form(&self.form, &scope);
        let invalid = has_errors(&result);
        self.errors = result;
        if invalid {
            self.status = Some(error_notice("Fix the highlighted problems before saving."));
            cx.notify();
            return;
        }
        if self.table.is_none() {
            return;
        }
        self.notice = None;
        // Busy before anything is awaited, so a second press cannot start a second write.
        self.busy = true;
        let values = values_to_write(&self.form, &scope, &self.original);
        let changes = self.props.mode == FormMode::Create || self.is_dirty();
        if changes && event_action(&self.form, FormEvent::BeforeUpdate).is_some() {
            self.run_event(FormEvent::BeforeUpdate, values.clone(), None, cx, move |this, result, cx| {
                if let Some(veto) = veto_message(Some(&result)) {
                    this.status = Some(error_notice(veto));
                    this.busy = false;
                    cx.notify();
                    return;
                }
                this.write(values, changes, cx);
            });
        } else {
            self.write(values, changes, cx);
        }
        cx.notify();
    }

    fn fail(&mut self, message: String, cx: &mut Context<Self>) {
        self.status = Some(error_notice(message));
        self.busy = false;
        cx.notify();
    }

    fn write(&mut self, values: RecordValues, changes: bool, cx: &mut Context<Self>) {
        let Some(table) = self.table.clone() else { return };
        let creating = self.props.mode == FormMode::Create;
        let op = if creating { Operation::Create } else { Operation::Update };
        if !self.allowed(op, cx) {
            let message = if creating {
                "This role cannot create records here."
            } else {
                "This role cannot change records here."
            };
            return self.fail(message.into(), cx);
        }
        let identity = self.identity.clone();
        if !creating && identity.is_none() {
            return self.fail("This record cannot be identified for saving.".into(), cx);
        }
        let label = self.props.env.label.to_string();
        let original = self.original.clone();
        let known = self.schema.clone();
        let sent = values.clone();
        task::spawn_call(
            cx,
            move || -> Result<(WriteOutcome, Arc<TableSchema>), AppError> {
                let schema = match known {
                    Some(schema) => schema,
                    None => table_schema(&label, &table)?,
                };
                let outcome = if creating {
                    create_row(&label, &table, &schema, &values)?
                } else {
                    update_row(&label, &table, &schema, identity.as_deref().unwrap_or(&[]), &values, &original)?
                };
                Ok((outcome, schema))
            },
            move |this, result, cx| match result {
                Err(e) => this.fail(write_message(&e), cx),
                Ok((outcome, schema)) => this.saved(outcome, schema, sent, changes, cx),
            },
        );
    }

    fn saved(
        &mut self,
        outcome: WriteOutcome,
        schema: Arc<TableSchema>,
        values: RecordValues,
        changes: bool,
        cx: &mut Context<Self>,
    ) {
        let creating = self.props.mode == FormMode::Create;
        self.schema = Some(schema.clone());
        let env = self.props.env.clone();
        env.records_changed(cx);
        let default_text = if creating { "Record created." } else { "Changes saved." };
        let tone = if outcome.problem.is_some() { NoticeTone::Error } else { NoticeTone::Info };
        self.announce(outcome.problem.clone().unwrap_or_else(|| default_text.into()), tone, cx);
        let (saved, snapshot, next_id) = if creating {
            let id = record_id_for(&self.props.env.label, &schema, &outcome.record, &outcome.identity);
            (outcome.record.clone(), None, id)
        } else {
            let mut merged = self.original.clone();
            merged.extend(values.clone());
            (values.clone(), Some(merged), self.props.record_id.clone().unwrap_or(Value::Null))
        };
        let reported = if creating { saved.clone() } else { snapshot.clone().unwrap_or_default() };
        let finish = move |this: &mut Self, navigated: bool, cx: &mut Context<Self>| {
            if let Some(on_saved) = this.props.on_saved.clone() {
                let record = reported.clone();
                later(cx, move |cx| on_saved(record, cx));
            }
            this.busy = false;
            cx.notify();
            if navigated {
                return;
            }
            if this.props.embedded {
                let on_close = this.props.on_close.clone();
                later(cx, move |cx| on_close(cx));
            } else {
                let on_mode = this.props.on_mode.clone();
                let (carry, notice) = (this.props.on_carry.clone(), this.notice.clone());
                later(cx, move |cx| {
                    if let (Some(carry), Some(notice)) = (carry, notice) {
                        carry(notice, cx);
                    }
                    on_mode(FormMode::Detail, Some(next_id), cx);
                });
            }
        };
        if (creating || changes) && event_action(&self.form, FormEvent::AfterUpdate).is_some() {
            self.ignore_refresh = true;
            self.run_event(FormEvent::AfterUpdate, saved, snapshot, cx, move |this, result, cx| {
                this.ignore_refresh = false;
                if let Some(problem) = after_update_problem(Some(&result)) {
                    this.announce(problem, NoticeTone::Error, cx);
                }
                let navigated = this.navigated;
                finish(this, navigated, cx);
            });
        } else {
            finish(self, false, cx);
        }
    }

    // ---- delete -------------------------------------------------------------------

    pub fn remove(&mut self, cx: &mut Context<Self>) {
        let (Some(table), Some(identity)) = (self.table.clone(), self.identity.clone()) else { return };
        let answer = crate::ui::dialogs::confirm(cx, "Delete record?", "This record will be deleted. This cannot be undone.");
        task::after(cx, answer, move |this, ok, cx| {
            if !ok {
                return;
            }
            if !this.allowed(Operation::Delete, cx) {
                return this.fail("This role cannot delete records here.".into(), cx);
            }
            let (label, original, known) = (this.props.env.label.to_string(), this.original.clone(), this.schema.clone());
            task::spawn_call(
                cx,
                move || {
                    let schema = match known {
                        Some(schema) => schema,
                        None => table_schema(&label, &table)?,
                    };
                    delete_row(&label, &table, &schema, &identity, &original)
                },
                |this, result, cx| match result {
                    Err(e) => this.fail(write_message(&e), cx),
                    Ok(()) => {
                        let env = this.props.env.clone();
                        env.records_changed(cx);
                        env.notify("Record deleted.", NoticeTone::Info, cx);
                        let on_close = this.props.on_close.clone();
                        later(cx, move |cx| on_close(cx));
                    }
                },
            );
        });
    }
}

/// What an action run needs from the view, copied so it can move to the action thread.
struct ActionData {
    config: crate::archive::DocumentConfig,
    label: String,
    record: RecordValues,
    form: Map<String, Value>,
    app: Map<String, Value>,
    params: Map<String, Value>,
    snapshot: Option<RecordValues>,
}

impl ActionData {
    fn into_context(self, host: Arc<dyn crate::forms::actions::ActionHost>) -> crate::forms::actions::ActionContext {
        let mut ctx = context(self.config, &self.label, host, Some(self.record), self.form, self.app);
        ctx.params = Some(self.params);
        ctx.snapshot = self.snapshot;
        ctx
    }
}

impl Render for RecordView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.weak_entity();
        let title = self.title();
        let scope = self.scope(cx);
        let form = self.form.clone();
        let visible = Rc::new(visible_controls(&form, &scope));
        let enabled = Rc::new(enabled_controls(&form, &scope));
        let read_only = self.read_only();
        let ready = self.ready();
        let mut locked: HashSet<String> = self.props.link.keys().cloned().collect();
        if self.props.mode == FormMode::Edit {
            if let Some(schema) = &self.schema {
                locked.extend(schema.columns.iter().filter(|c| c.primary_key_position > 0).map(|c| c.name.clone()));
            }
        }
        let config = self.props.env.config(cx);
        let runnable: HashSet<String> = form
            .controls
            .iter()
            .filter(|c| c.kind == ControlKind::Button)
            .filter(|c| {
                c.action_id.as_deref().is_none_or(str::is_empty)
                    || (ready && self.props.env.can("action", c.action_id.as_deref().unwrap_or(""), Operation::Execute, cx))
            })
            .map(|c| c.id.clone())
            .collect();
        let _ = config;
        let errors: HashMap<String, String> = self.errors.fields.iter().cloned().collect();
        let saved = self.identity.is_some();
        let depth = self.props.depth;
        let env = self.props.env.clone();
        let (set_to, blur_to, button_to, enter_to) = (this.clone(), this.clone(), this.clone(), this.clone());
        let ctx = BodyContext {
            env: env.clone(),
            form: form.clone(),
            scope: scope.clone(),
            visible,
            enabled,
            errors: Rc::new(errors),
            read_only,
            locked: Rc::new(locked),
            repeated: false,
            prefix: BodyKey::default(),
            runnable: Rc::new(runnable),
            on_set: Rc::new(move |column, value, cx| {
                let _ = set_to.update(cx, |v, cx| v.set_field(column, value, cx));
            }),
            on_blur: Rc::new(move |id, cx| {
                let _ = blur_to.update(cx, |v, cx| v.blur(id, cx));
            }),
            on_button: Rc::new(move |id, _, cx| {
                let _ = button_to.update(cx, |v, cx| v.run_button(id, cx));
            }),
            on_enter: Rc::new(move |cx| {
                let _ = enter_to.update(cx, |v, cx| v.save(cx));
            }),
            related: Rc::new({
                let scope = scope.clone();
                move |control: &Control, disabled, window, cx| {
                    related_list(
                        RelatedProps {
                            env: env.clone(),
                            control: control.clone(),
                            scope: scope.clone(),
                            saved,
                            disabled,
                            depth,
                        },
                        "",
                        window,
                        cx,
                    )
                }
            }),
        };
        let theme = cx.theme().clone();
        let body = if self.loaded {
            control_grid(&ctx, None, None, window, cx)
        } else {
            widgets::styled_text("record-loading", "Loading record…", {
                let color = theme.muted_foreground;
                move |d| d.ty(Type::Body).text_color(color)
            })
            .into_any_element()
        };
        let line = |kind: &str, notice: &Option<Notice>, cx: &App| {
            notice.as_ref().map(|n| {
                let error = n.tone == NoticeTone::Error;
                let anim = format!("slide:{kind}:{}", n.message);
                widgets::slide_in(anim, super::message(format!("{kind}:{}", if error { "error" } else { "info" }), n.message.clone(), error, cx))
            })
        };
        let mode = self.props.mode;
        let embedded = self.props.embedded;
        let submit_label = self.submit_label();
        let can_edit = self.can_edit(cx);
        let can_delete = self.can_delete(cx);
        let record_id = self.props.record_id.clone();
        let on_mode = self.props.on_mode.clone();
        let on_close = self.props.on_close.clone();
        let close_for_cancel = self.props.on_close.clone();
        let mode_for_cancel = self.props.on_mode.clone();
        let id_for_cancel = self.props.record_id.clone();
        v_flex()
            .id(SharedString::from(format!("form:{}", widgets::slug(&title))))
            .aria_label(title.clone())
            .size_full()
            .gap_2()
            .p_4()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(widgets::styled_text("record-title", title.clone(), move |d| {
                        d.when(embedded, |d| d.ty(Type::Section)).when(!embedded, |d| d.ty(Type::Title))
                    }))
                    .child(
                        h_flex()
                            .gap_2()
                            .when(mode == FormMode::Detail && can_edit, |d| {
                                let (on_mode, id) = (on_mode.clone(), record_id.clone());
                                d.child(Button::new("edit").small().label("Edit").on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.notice = None;
                                        let (on_mode, id) = (on_mode.clone(), id.clone());
                                        later(cx, move |cx| on_mode(FormMode::Edit, id, cx));
                                    },
                                )))
                            })
                            .when(mode == FormMode::Detail && self.allowed(Operation::Delete, cx), |d| {
                                d.child(
                                    Button::new("delete")
                                        .danger()
                                        .outline()
                                        .small()
                                        .label("Delete")
                                        .disabled(!can_delete)
                                        .on_click(cx.listener(|this, _, _, cx| this.remove(cx))),
                                )
                            })
                            .when(mode == FormMode::Detail && embedded, |d| {
                                let on_close = on_close.clone();
                                d.child(Button::new("close").small().label("Close").on_click(move |_, _, cx| on_close(cx)))
                            }),
                    ),
            )
            .children(line("status", &self.status, cx))
            // Messages slide in; no space is reserved for them.
            .children(line("notice", &self.notice, cx))
            .when(!self.errors.form.is_empty(), |d| d.child(super::body::problems_callout("form-errors", &self.errors.form, cx)))
            .child(div().id("record-body").flex_1().min_h_0().overflow_y_scroll().child(body))
            .when(!read_only, |d| {
                d.child(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(Button::new("cancel").label("Cancel").on_click(move |_, _, cx| {
                            if mode == FormMode::Edit {
                                mode_for_cancel(FormMode::Detail, id_for_cancel.clone(), cx);
                            } else {
                                close_for_cancel(cx);
                            }
                        }))
                        .child(
                            Button::new(if mode == FormMode::Create { "create" } else { "save" })
                                .primary()
                                .label(submit_label.clone())
                                .disabled(!self.can_save())
                                .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                        ),
                )
            })
            .test_support()
    }
}
