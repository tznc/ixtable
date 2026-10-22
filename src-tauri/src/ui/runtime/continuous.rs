//! Continuous mode (port of `ContinuousView.tsx` and `ContinuousRow.tsx`): one page of
//! records, each drawn with the form's own grid and controls and edited in place, plus a
//! new-record row when the role may create. Reads go through the same DuckDB page reader as
//! list mode.
//!
//! Test ids: the region is `continuous:<form name slug>`; rows are `crow:<n>` ("Record N"; the
//! new-record row is `crow:new`, "New record") and their fields carry the prefix `record-N:`
//! (`record-1:field:name`); a row's buttons are `crow:<n>:save` ("Save" / "Add"),
//! `crow:<n>:undo` and `crow:<n>:delete`; `crow:<n>:problem` shows its error. The view has
//! `continuous-notice` (errors, or any message without a `Runtime`; success is a toast), `continuous-error`, `continuous-empty`, `pager-status`, `prev-page`
//! and `next-page`. (The web UI also saved a changed row when focus left it; here Save and
//! Enter do.)

use super::{
    BodyKey, RuntimeEnv,
    body::{BodyContext, control_grid, header_grid},
    host::{HostEvent, UiHost, context, run_action_job},
};
use crate::data::{DataValue, TableSchema};
use crate::design::{ControlKind, Form, FormMode};
use crate::forms::actions::{NavigationTarget, NoticeTone, StateScope, run_action};
use crate::forms::conditions::{TRUNCATED_NOTICE, page_label};
use crate::forms::data::{PageRequest, RecordPage, load_page, source_params, table_schema};
use crate::forms::navigation::{PageKind, RuntimePage};
use crate::forms::rbac::Operation;
use crate::forms::registry::is_designed_form;
use crate::forms::row_writes::{create_row, delete_row, update_row, values_to_write};
use crate::forms::state::{
    FormErrors, FormScope, default_record, enabled_controls, has_errors, validate_control, validate_form,
    visible_controls,
};
use crate::forms::values::{RecordValues, same_value};
use crate::ui::prelude::*;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

type Changed = Rc<dyn Fn(String, NoticeTone, &mut App)>;

fn text_of(error: &crate::manager::AppError) -> String {
    if error.code == "CONFLICT" {
        return "Someone else changed this record. The list was reloaded with the latest values.".into();
    }
    error.message.clone()
}

// ---- one row ------------------------------------------------------------------------

pub struct ContinuousRow {
    env: RuntimeEnv,
    form: Rc<Form>,
    table: Option<String>,
    label: String,
    slot: String,
    stored: Option<RecordValues>,
    identity: Option<Vec<DataValue>>,
    original: RecordValues,
    record: RecordValues,
    form_state: Map<String, Value>,
    errors: FormErrors,
    problem: String,
    busy: bool,
    can_update: bool,
    can_delete: bool,
    on_changed: Changed,
}

impl ContinuousRow {
    #[allow(clippy::too_many_arguments)]
    fn new(
        env: RuntimeEnv,
        form: Rc<Form>,
        table: Option<String>,
        slot: String,
        label: String,
        stored: Option<RecordValues>,
        identity: Option<Vec<DataValue>>,
        on_changed: Changed,
        cx: &mut Context<Self>,
    ) -> Self {
        let original = stored
            .clone()
            .unwrap_or_else(|| default_record(&form, &Map::new(), &env.app(cx)));
        if let Some(runtime) = env.runtime.clone() {
            cx.observe(&runtime, |_, _, cx| cx.notify()).detach();
        }
        Self {
            env,
            form,
            table,
            label,
            slot,
            stored,
            identity,
            record: original.clone(),
            original,
            form_state: Map::new(),
            errors: FormErrors::default(),
            problem: String::new(),
            busy: false,
            can_update: false,
            can_delete: false,
            on_changed,
        }
    }

    fn creating(&self) -> bool {
        self.stored.is_none()
    }

    fn editable(&self) -> bool {
        if self.creating() { self.table.is_some() } else { self.can_update }
    }

    pub fn is_dirty(&self) -> bool {
        self.record.iter().any(|(k, v)| !same_value(v, self.original.get(k).unwrap_or(&Value::Null)))
    }

    pub fn record(&self) -> &RecordValues {
        &self.record
    }

    /// A re-read page brings new stored values; a row with unsaved edits keeps them (and its
    /// original values, so the optimistic check still sees a concurrent change).
    fn sync(&mut self, stored: Option<RecordValues>, identity: Option<Vec<DataValue>>, update: bool, delete: bool) {
        self.can_update = update;
        self.can_delete = delete;
        self.identity = identity;
        if let Some(stored) = stored {
            if self.stored.as_ref() != Some(&stored) {
                if !self.is_dirty() {
                    self.original = stored.clone();
                    self.record = stored.clone();
                }
                self.stored = Some(stored);
            }
        }
    }

    fn scope(&self, cx: &App) -> FormScope {
        FormScope::new(self.record.clone(), self.form_state.clone(), self.env.app(cx))
    }

    fn set_field(&mut self, column: &str, value: Value, cx: &mut Context<Self>) {
        self.record.insert(column.to_string(), value);
        cx.notify();
    }

    fn blur(&mut self, control_id: &str, cx: &mut Context<Self>) {
        let Some(control) = self.form.controls.iter().find(|c| c.id == control_id).cloned() else { return };
        let Some(column) = control.binding.as_ref().map(|b| b.column.clone()).filter(|c| !c.is_empty()) else {
            return;
        };
        if !self.editable() {
            return;
        }
        let scope = self.scope(cx);
        let value = self.record.get(&column).cloned().unwrap_or(Value::Null);
        let message = validate_control(&control, &value, &scope);
        self.errors.fields.retain(|(id, _)| id != control_id);
        if let Some(message) = message {
            self.errors.fields.push((control_id.to_string(), message));
        }
        cx.notify();
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        self.record = self.original.clone();
        cx.notify();
    }

    pub fn save(&mut self, cx: &mut Context<Self>) {
        let Some(table) = self.table.clone() else { return };
        if self.busy || !self.is_dirty() {
            return;
        }
        let scope = self.scope(cx);
        let result = validate_form(&self.form, &scope);
        let invalid = has_errors(&result);
        self.errors = result;
        if invalid {
            self.problem = "Fix the highlighted problems before saving.".into();
            cx.notify();
            return;
        }
        self.busy = true;
        self.problem.clear();
        let values = values_to_write(&self.form, &scope, &self.original);
        let (label, original, identity, creating) =
            (self.env.label.to_string(), self.original.clone(), self.identity.clone(), self.creating());
        if creating && !self.editable() || !creating && !self.can_update {
            self.busy = false;
            self.problem = if creating {
                "This role cannot create records here.".into()
            } else {
                "This role cannot change records here.".into()
            };
            cx.notify();
            return;
        }
        if !creating && identity.is_none() {
            self.busy = false;
            self.problem = "This record cannot be identified for saving.".into();
            cx.notify();
            return;
        }
        task::spawn_call(
            cx,
            move || {
                let schema: Arc<TableSchema> = table_schema(&label, &table)?;
                if creating {
                    create_row(&label, &table, &schema, &values)
                } else {
                    update_row(&label, &table, &schema, identity.as_deref().unwrap_or(&[]), &values, &original)
                }
            },
            move |this, result, cx| {
                this.busy = false;
                match result {
                    Err(e) => this.problem = text_of(&e),
                    Ok(outcome) => {
                        let default_text = if creating { "Record created." } else { "Changes saved." };
                        let tone = if outcome.problem.is_some() { NoticeTone::Error } else { NoticeTone::Info };
                        if !creating {
                            this.original = outcome.record.clone();
                            this.record = outcome.record;
                        } else {
                            this.record = this.original.clone();
                        }
                        let (env, changed) = (this.env.clone(), this.on_changed.clone());
                        let text = outcome.problem.unwrap_or_else(|| default_text.into());
                        env.records_changed(cx);
                        changed(text, tone, cx);
                    }
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    fn remove(&mut self, cx: &mut Context<Self>) {
        let (Some(table), Some(identity)) = (self.table.clone(), self.identity.clone()) else { return };
        if !self.can_delete {
            return;
        }
        let answer = crate::ui::dialogs::confirm(cx, "Delete record?", "This record will be deleted. This cannot be undone.");
        task::after(cx, answer, move |this, ok, cx| {
            if !ok {
                return;
            }
            let (label, original) = (this.env.label.to_string(), this.original.clone());
            task::spawn_call(
                cx,
                move || {
                    let schema = table_schema(&label, &table)?;
                    delete_row(&label, &table, &schema, &identity, &original)
                },
                |this, result, cx| {
                    match result {
                        Err(e) => this.problem = text_of(&e),
                        Ok(()) => {
                            let (env, changed) = (this.env.clone(), this.on_changed.clone());
                            env.records_changed(cx);
                            changed("Record deleted.".into(), NoticeTone::Info, cx);
                        }
                    }
                    cx.notify();
                },
            );
        });
    }

    fn on_host_event(&mut self, event: HostEvent, cx: &mut Context<Self>) {
        match event {
            HostEvent::Navigate(target) => navigate(&self.env, target, cx),
            HostEvent::SetState(StateScope::Form, key, value) => {
                self.form_state.insert(key, value);
                cx.notify();
            }
            HostEvent::SetState(StateScope::App, key, value) => self.env.set_app_state(&key, value, cx),
            HostEvent::Notify(text, tone) => {
                let changed = self.on_changed.clone();
                changed(text, tone, cx);
            }
            HostEvent::Refresh => {
                let changed = self.on_changed.clone();
                changed(String::new(), NoticeTone::Info, cx);
            }
            _ => {}
        }
    }

    fn run_button(&mut self, control_id: &str, cx: &mut Context<Self>) {
        let Some(action_id) = self
            .form
            .controls
            .iter()
            .find(|c| c.id == control_id)
            .and_then(|c| c.action_id.clone())
            .filter(|a| !a.is_empty())
        else {
            return;
        };
        if self.busy {
            return;
        }
        let config = self.env.config(cx).clone();
        let mut ctx_record = self.record.clone();
        let _ = &mut ctx_record;
        let (label, record, form_state, app) = (
            self.env.label.to_string(),
            self.record.clone(),
            self.form_state.clone(),
            self.env.app(cx),
        );
        let snapshot = (!self.creating() && self.table.is_some()).then(|| self.original.clone());
        let host = UiHost::new(config.clone(), self.env.role_id(cx), self.env.attached());
        let runtime = self.env.runtime.clone();
        run_action_job(
            cx,
            host,
            runtime,
            move |host| {
                let mut ctx = context(config, &label, host, Some(record), form_state, app);
                ctx.snapshot = snapshot;
                run_action(&action_id, &ctx)
            },
            Self::on_host_event,
            |this, result, cx| {
                if !result.ok {
                    if let Some(error) = result.error.filter(|e| !e.is_empty()) {
                        this.problem = error;
                    }
                }
                cx.notify();
            },
        );
    }
}

/// Opens a navigation target as a Runtime page (rows outside Run mode have nowhere to go).
fn navigate(env: &RuntimeEnv, target: NavigationTarget, cx: &mut App) {
    let (Some(runtime), Some(kind)) = (env.runtime.clone(), PageKind::parse(&target.kind)) else { return };
    let page = RuntimePage {
        kind,
        id: target.id,
        mode: target.mode,
        record_id: target.record_id,
        params: target.params,
        nav_id: None,
    };
    runtime.update(cx, |r, cx| r.navigate(page, cx));
}

impl Render for ContinuousRow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.weak_entity();
        let scope = self.scope(cx);
        let form = self.form.clone();
        let editable = self.editable();
        let (creating, dirty, busy) = (self.creating(), self.is_dirty(), self.busy);
        let runnable: HashSet<String> = form
            .controls
            .iter()
            .filter(|c| c.kind == ControlKind::Button)
            .filter(|c| {
                c.action_id.as_deref().is_none_or(str::is_empty)
                    || (!busy && self.env.can("action", c.action_id.as_deref().unwrap_or(""), Operation::Execute, cx))
            })
            .map(|c| c.id.clone())
            .collect();
        let errors: HashMap<String, String> = self.errors.fields.iter().cloned().collect();
        let (set_to, blur_to, button_to, enter_to) = (this.clone(), this.clone(), this.clone(), this.clone());
        let ctx = BodyContext {
            env: self.env.clone(),
            form: form.clone(),
            visible: Rc::new(visible_controls(&form, &scope)),
            enabled: Rc::new(enabled_controls(&form, &scope)),
            scope,
            errors: Rc::new(errors),
            read_only: !editable,
            locked: Rc::default(),
            // A repeated row holds no related lists; open the record for its details.
            repeated: true,
            prefix: BodyKey::from(format!("{}:", self.slot)),
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
                if editable {
                    let _ = enter_to.update(cx, |v, cx| v.save(cx));
                }
            }),
            related: Rc::new(|_, _, _, _| div().into_any_element()),
        };
        let theme = cx.theme().clone();
        let grid = control_grid(&ctx, None, None, window, cx);
        let id = format!("crow:{}", self.slot.trim_start_matches("record-").to_string());
        let id = if creating { "crow:new".to_string() } else { id };
        let mut problems: Vec<String> = vec![];
        if !self.problem.is_empty() {
            problems.push(self.problem.clone());
        }
        problems.extend(self.errors.form.clone());
        v_flex()
            .id(SharedString::from(id.clone()))
            .aria_label(self.label.clone())
            .gap_1()
            .p_2()
            .border_1()
            .border_color(if dirty { theme.primary } else { theme.border })
            .rounded_md()
            .child(grid)
            .child(
                h_flex()
                    .gap_2()
                    .when(editable && (creating || dirty), |d| {
                        d.child(
                            Button::new(SharedString::from(format!("{id}:save")))
                                .small()
                                .when(creating, |b| b.primary())
                                .label(if creating { "Add" } else { "Save" })
                                .disabled(busy || !dirty)
                                .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                        )
                    })
                    .when(!creating && dirty, |d| {
                        d.child(
                            Button::new(SharedString::from(format!("{id}:undo")))
                                .small()
                                .label("Undo")
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, _, cx| this.undo(cx))),
                        )
                    })
                    .when(!creating && self.can_delete, |d| {
                        d.child(
                            Button::new(SharedString::from(format!("{id}:delete")))
                                .danger()
                                .outline()
                                .small()
                                .label("Delete")
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, _, cx| this.remove(cx))),
                        )
                    }),
            )
            .when(!problems.is_empty(), |d| {
                d.child(super::body::problems_callout(&format!("{id}:problem"), &problems, cx))
            })
            .test_support()
    }
}

// ---- the page of rows -------------------------------------------------------------------

pub struct ContinuousView {
    env: RuntimeEnv,
    form: Rc<Form>,
    params: Map<String, Value>,
    offset: u64,
    page: Option<RecordPage>,
    rows: Vec<(String, Entity<ContinuousRow>)>,
    new_row: Option<Entity<ContinuousRow>>,
    error: String,
    notice: Option<(String, NoticeTone)>,
    seq: u64,
    _bus: Subscription,
}

impl ContinuousView {
    pub fn new(
        env: RuntimeEnv,
        form: Form,
        params: Map<String, Value>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let bus = env.bus.clone();
        let subscription = cx.subscribe(&bus, |this, _, event: &crate::ui::events::DocEvent, cx| {
            if matches!(event, crate::ui::events::DocEvent::DatabaseChanged) {
                crate::forms::data::invalidate(&this.env.label, true);
            }
            this.reload(cx);
        });
        let mut view = Self {
            env,
            form: Rc::new(form),
            params,
            offset: 0,
            page: None,
            rows: vec![],
            new_row: None,
            error: String::new(),
            notice: None,
            seq: 0,
            _bus: subscription,
        };
        view.reload(cx);
        view
    }

    fn table(&self) -> Option<String> {
        self.form
            .source
            .as_ref()
            .filter(|s| s.kind == crate::design::SourceKind::Table)
            .and_then(|s| s.table.clone())
            .filter(|t| !t.is_empty())
    }

    fn limit(&self) -> u64 {
        u64::from(self.form.page_size).max(1)
    }

    pub fn can_prev(&self) -> bool {
        self.offset > 0
    }

    pub fn can_next(&self) -> bool {
        self.page.as_ref().is_some_and(|p| self.offset + self.limit() < p.total)
    }

    pub fn page(&self) -> Option<&RecordPage> {
        self.page.as_ref()
    }

    fn allowed(&self, op: Operation, mode: Option<FormMode>, cx: &App) -> bool {
        let Some(table) = self.table() else { return false };
        if mode.is_some_and(|m| !self.form.modes.contains(&m)) {
            return false;
        }
        let subject = if is_designed_form(self.env.config(cx), &self.form) {
            ("form", self.form.id.clone())
        } else {
            ("table", table)
        };
        self.env.can(subject.0, &subject.1, op, cx)
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.seq += 1;
        let seq = self.seq;
        let mut scope = Map::new();
        scope.insert("app".into(), Value::Object(self.env.app(cx)));
        scope.insert("params".into(), Value::Object(self.params.clone()));
        let (label, form, offset, limit) =
            (self.env.label.to_string(), (*self.form).clone(), self.offset, self.limit());
        task::spawn_call(
            cx,
            move || {
                let bound = source_params(&form, &scope).map_err(|m| m)?;
                load_page(
                    &label,
                    &form,
                    &PageRequest { offset, limit, sorts: vec![], filters: vec![] },
                    &scope,
                    &bound,
                )
                .map_err(|e| e.message)
            },
            move |this, result, cx| {
                if this.seq != seq {
                    return;
                }
                match result {
                    Ok(page) => {
                        this.error.clear();
                        this.apply(page, cx);
                    }
                    Err(message) => this.error = message,
                }
                cx.notify();
            },
        );
    }

    /// Keeps a row's entity (and its unsaved edits) across page reads.
    fn apply(&mut self, page: RecordPage, cx: &mut Context<Self>) {
        let (can_update, can_delete) =
            (self.allowed(Operation::Update, Some(FormMode::Edit), cx), self.allowed(Operation::Delete, None, cx));
        let table = self.table();
        let this = cx.weak_entity();
        let changed: Changed = Rc::new(move |text, tone, cx| {
            let _ = this.update(cx, |v, cx| v.changed(text, tone, cx));
        });
        let mut old: HashMap<String, Entity<ContinuousRow>> = self.rows.drain(..).collect();
        for (index, record) in page.rows.iter().enumerate() {
            let identity = page.identities.as_ref().and_then(|ids| ids.get(index)).cloned();
            let key = serde_json::to_string(&identity.as_ref().map_or_else(
                || vec![serde_json::json!(self.offset as usize + index)],
                |id| id.iter().map(|v| serde_json::to_value(v).unwrap_or_default()).collect(),
            ))
            .unwrap_or_default();
            let slot = format!("record-{}", self.offset as usize + index + 1);
            let entity = match old.remove(&key) {
                Some(existing) => {
                    existing.update(cx, |row, _| {
                        row.slot = slot;
                        row.label = format!("Record {}", self.offset as usize + index + 1);
                        row.sync(Some(record.clone()), identity, can_update, can_delete);
                    });
                    existing
                }
                None => {
                    let (env, form, table, changed) = (self.env.clone(), self.form.clone(), table.clone(), changed.clone());
                    let stored = Some(record.clone());
                    let label = format!("Record {}", self.offset as usize + index + 1);
                    let entity = cx.new(|cx| {
                        ContinuousRow::new(env, form, table, slot, label, stored, identity.clone(), changed, cx)
                    });
                    entity.update(cx, |row, _| row.sync(None, identity, can_update, can_delete));
                    entity
                }
            };
            self.rows.push((key, entity));
        }
        self.new_row = (self.allowed(Operation::Create, Some(FormMode::Create), cx)).then(|| {
            self.new_row.clone().unwrap_or_else(|| {
                let (env, form, table) = (self.env.clone(), self.form.clone(), table.clone());
                cx.new(|cx| {
                    ContinuousRow::new(env, form, table, "record-new".into(), "New record".into(), None, None, changed, cx)
                })
            })
        });
        self.page = Some(page);
    }

    fn changed(&mut self, text: String, tone: NoticeTone, cx: &mut Context<Self>) {
        if tone == NoticeTone::Info && self.env.runtime.is_some() {
            // Success is a toast, never a line under the title.
            let env = self.env.clone();
            cx.defer(move |cx| env.succeed(text, cx));
        } else if !text.is_empty() {
            self.notice = Some((text, tone));
        }
        self.reload(cx);
        cx.notify();
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
        self.reload(cx);
    }
}

impl Render for ContinuousView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let total = self.page.as_ref().map_or(0, |p| p.total);
        let shown = self.page.as_ref().map_or(0, |p| p.rows.len() as u64);
        let status = match &self.page {
            Some(_) => page_label(self.offset, shown, total),
            None => "Loading…".to_string(),
        };
        v_flex()
            .id(SharedString::from(format!("continuous:{}", widgets::slug(&self.form.name))))
            .aria_label(self.form.name.clone())
            .size_full()
            .gap_2()
            .p_4()
            .child(widgets::styled_text("list-title", self.form.name.clone(), |d| {
                d.ty(Type::Title)
            }))
            .when(!self.error.is_empty(), |d| d.child(super::message("continuous-error", self.error.clone(), true, cx)))
            // The status line slides in; no space is reserved for it.
            .children(self.notice.clone().map(|(text, tone)| {
                let anim = format!("slide:continuous:{text}");
                widgets::slide_in(anim, super::message("continuous-notice", text, tone == NoticeTone::Error, cx))
            }))
            .when(self.page.as_ref().is_some_and(|p| p.truncated), |d| {
                d.child(widgets::callout("list-notice", widgets::Severity::Warning, TRUNCATED_NOTICE, None, cx))
            })
            .child(
                v_flex()
                    .id("continuous-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap_2()
                    // Labels once as column headers; rows carry none (inset to match a row's padding and border).
                    .when(!self.rows.is_empty() || self.new_row.is_some(), |d| {
                        d.child(div().id("continuous-header").px(px(9.)).child(header_grid(&self.form, cx)).test_support())
                    })
                    .children(self.rows.iter().map(|(_, row)| row.clone()))
                    .when(self.page.as_ref().is_some_and(|p| p.rows.is_empty()), |d| {
                        d.child(widgets::empty_state("continuous-empty", "No records yet", "Records you add will show here.", cx))
                    })
                    .children(self.new_row.clone()),
            )
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
                    ),
            )
            .test_support()
    }
}
