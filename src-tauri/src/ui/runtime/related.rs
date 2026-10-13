//! Master/detail related-record lists (port of `RelatedList.tsx`), nested up to
//! `MAX_SUBFORM_DEPTH` levels: child rows whose key columns match the current record, kept by
//! the list's `filter` expression (`record` is the child, `parent` the parent). `disabled` (the
//! control's or a container's `enabledWhen`) makes the list read-only.
//!
//! Test ids: the region is `related:<label slug>` (labelled with the list's label), rows
//! `related:<slug>:row:<n>`, cells `related:<slug>:cell:<n>:<column slug>` (text; "Yes"/"No"
//! for booleans), `related:<slug>:add` ("Add x"), `:edit:<n>`, `:delete:<n>`, `:prev`,
//! `:next`, `:pager`, `:empty`, `:hint` (nesting limit / save first), `:error`, `:message`
//! and the embedded child form in `related:<slug>:form`.

use super::{
    RuntimeEnv,
    stack::{FormRenderer, FormRendererProps},
};
use crate::data::{DataValue, Filter, FilterOperator, TableSchema};
use crate::design::{Control, Form, FormMode};
use crate::forms::actions::NoticeTone;
use crate::forms::conditions::{filter_inputs_key, paged_scan, row_filter};
use crate::forms::data::{PageRequest, RecordPage, record_id_for, table_page, table_schema};
use crate::forms::fields::{field_settings_for, field_text};
use crate::forms::generate::humanize;
use crate::forms::lookups::{LookupLabels, Lookups, column_lookups, label_for, lookup_labels};
use crate::forms::nesting::MAX_SUBFORM_DEPTH;
use crate::forms::rbac::Operation;
use crate::forms::registry::{is_designed_form, resolve_form, table_forms};
use crate::forms::row_writes::delete_row;
use crate::forms::schema::related_keys;
use crate::forms::state::FormScope;
use crate::forms::generate::GenerateOptions;
use crate::forms::values::{RecordValues, boolean_value, display_text, is_boolean_column, to_column_value};
use crate::ui::prelude::*;
use serde_json::{Map, Value};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct RelatedProps {
    pub env: RuntimeEnv,
    pub control: Control,
    pub scope: FormScope,
    /// True when the parent record is stored (it has an identity).
    pub saved: bool,
    pub disabled: bool,
    pub depth: usize,
}

struct Rows {
    records: Vec<RecordValues>,
    identities: Vec<Vec<DataValue>>,
    total: u64,
}

struct Loaded {
    schema: Arc<TableSchema>,
    child_form: Option<Form>,
    rows: Rows,
    lookups: Lookups,
    labels: LookupLabels,
}

pub struct RelatedRecords {
    props: Option<RelatedProps>,
    schema: Option<Arc<TableSchema>>,
    child_form: Option<Form>,
    rows: Option<Rows>,
    lookups: Lookups,
    labels: LookupLabels,
    editing: Option<(FormMode, Option<Value>)>,
    editor: Option<Entity<FormRenderer>>,
    error: String,
    message: String,
    offset: u64,
    load_key: String,
    seq: u64,
    _bus: Option<Subscription>,
}

fn id_of(control: &Control) -> String {
    widgets::slug(if control.label.is_empty() { &control.id } else { &control.label })
}

impl RelatedRecords {
    pub fn new() -> Self {
        Self {
            props: None,
            schema: None,
            child_form: None,
            rows: None,
            lookups: vec![],
            labels: LookupLabels::new(),
            editing: None,
            editor: None,
            error: String::new(),
            message: String::new(),
            offset: 0,
            load_key: String::new(),
            seq: 0,
            _bus: None,
        }
    }

    fn props(&self) -> &RelatedProps {
        self.props.as_ref().expect("props are set before rendering")
    }

    /// The child column values the parent record fixes.
    fn link(props: &RelatedProps) -> Map<String, Value> {
        let Some(related) = props.control.related.as_ref() else { return Map::new() };
        related_keys(related)
            .into_iter()
            .map(|key| (key.column, props.scope.record().get(&key.target).cloned().unwrap_or(Value::Null)))
            .collect()
    }

    fn filter_scope(props: &RelatedProps) -> Map<String, Value> {
        let scope = props.scope.scope();
        let mut out = Map::new();
        out.insert("parent".into(), Value::Object(props.scope.record().clone()));
        out.insert("form".into(), scope.get("form").cloned().unwrap_or(Value::Null));
        out.insert("app".into(), scope.get("app").cloned().unwrap_or(Value::Null));
        out.insert("params".into(), Value::Object(Map::new()));
        out
    }

    pub fn set_props(&mut self, props: RelatedProps, cx: &mut Context<Self>) {
        let first = self.props.is_none();
        let Some(related) = props.control.related.clone() else {
            self.props = Some(props);
            return;
        };
        let key = format!(
            "{}|{}|{}|{}",
            serde_json::to_string(&Self::link(&props)).unwrap_or_default(),
            filter_inputs_key(related.filter.as_deref(), &Self::filter_scope(&props)),
            props.saved,
            props.depth
        );
        if first {
            let bus = props.env.bus.clone();
            self._bus = Some(cx.subscribe(&bus, |this, _, _: &crate::ui::events::DocEvent, cx| this.load(0, cx)));
        }
        self.props = Some(props);
        if key != self.load_key {
            // A filter that settles on a new value reloads after a short pause.
            let delay = if first { 0 } else { 250 };
            self.load_key = key;
            self.load(delay, cx);
        }
    }

    fn load(&mut self, delay_ms: u64, cx: &mut Context<Self>) {
        let Some(props) = self.props.clone() else { return };
        let Some(related) = props.control.related.clone() else { return };
        if !props.saved || props.depth >= MAX_SUBFORM_DEPTH {
            return;
        }
        self.seq += 1;
        let seq = self.seq;
        let label = props.env.label.to_string();
        let config = props.env.config(cx).clone();
        let link = Self::link(&props);
        let filter_scope = Self::filter_scope(&props);
        let offset = self.offset;
        let work = move || -> Result<Loaded, String> {
            let schema = table_schema(&label, &related.table).map_err(|e| e.message)?;
            let child_form = match related.form_id.as_deref().filter(|f| !f.is_empty()) {
                Some(id) => resolve_form(&config, id),
                None => Some(table_forms(&schema, &GenerateOptions::default()).detail),
            };
            let limit = u64::from(child_form.as_ref().map_or(25, |f| f.page_size)).max(1);
            let filters: Vec<Filter> = link
                .iter()
                .map(|(column, value)| Filter {
                    column: column.clone(),
                    operator: FilterOperator::Eq,
                    value: Some(to_column_value(
                        value,
                        schema.columns.iter().find(|c| &c.name == column).map_or("", |c| c.declared_type.as_str()),
                    )),
                    values: None,
                })
                .collect();
            let read = |from: u64, size: u64| -> Result<RecordPage, String> {
                table_page(
                    &label,
                    &related.table,
                    &PageRequest { offset: from, limit: size, sorts: vec![], filters: filters.clone() },
                )
                .map_err(|e| e.message)
            };
            let keep = row_filter(related.filter.as_deref(), &filter_scope).map_err(|e| e.message)?;
            let rows = match keep {
                Some(keep) => {
                    let scan = paged_scan(
                        |from, size| {
                            let page = read(from as u64, size as u64)?;
                            let identities = page.identities.clone().unwrap_or_default();
                            Ok::<_, String>(
                                page.rows.into_iter().zip(identities).collect::<Vec<(RecordValues, Vec<DataValue>)>>(),
                            )
                        },
                        |row: &(RecordValues, Vec<DataValue>)| keep(&row.0),
                    )?;
                    let total = scan.matches.len() as u64;
                    let found: Vec<_> = scan.matches.into_iter().skip(offset as usize).take(limit as usize).collect();
                    Rows {
                        records: found.iter().map(|r| r.0.clone()).collect(),
                        identities: found.into_iter().map(|r| r.1).collect(),
                        total,
                    }
                }
                None => {
                    let page = read(offset, limit)?;
                    Rows {
                        total: page.total,
                        identities: page.identities.unwrap_or_default(),
                        records: page.rows,
                    }
                }
            };
            let columns: Vec<String> = if related.columns.is_empty() {
                schema.columns.iter().map(|c| c.name.clone()).filter(|c| !link.contains_key(c)).collect()
            } else {
                related.columns.clone()
            };
            let lookups = column_lookups(&label, Some(&related.table), &columns, |column| {
                child_form
                    .as_ref()
                    .and_then(|f| f.controls.iter().find(|c| c.binding.as_ref().is_some_and(|b| b.column == column)))
                    .and_then(|c| c.relationship.clone())
            });
            let labels = lookup_labels(&label, &lookups, &rows.records);
            Ok(Loaded { schema, child_form, rows, lookups, labels })
        };
        let apply = move |this: &mut Self, result: Result<Loaded, String>, cx: &mut Context<Self>| {
            if this.seq != seq {
                return;
            }
            match result {
                Ok(loaded) => {
                    let limit = u64::from(loaded.child_form.as_ref().map_or(25, |f| f.page_size)).max(1);
                    let total = loaded.rows.total;
                    this.schema = Some(loaded.schema);
                    this.child_form = loaded.child_form;
                    this.rows = Some(loaded.rows);
                    this.lookups = loaded.lookups;
                    this.labels = loaded.labels;
                    this.error.clear();
                    // A delete can empty the last page: step back to the new last page.
                    if this.offset > 0 && this.offset >= total {
                        this.offset = total.saturating_sub(1) / limit * limit;
                        this.load(0, cx);
                    }
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
    }

    fn limit(&self) -> u64 {
        u64::from(self.child_form.as_ref().map_or(25, |f| f.page_size)).max(1)
    }

    pub fn can_prev(&self) -> bool {
        self.offset > 0
    }

    pub fn can_next(&self) -> bool {
        self.rows.as_ref().is_some_and(|r| self.offset + self.limit() < r.total)
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
        self.load(0, cx);
    }

    fn allowed(&self, op: Operation, cx: &App) -> bool {
        let props = self.props();
        let Some(related) = props.control.related.as_ref() else { return false };
        let config = props.env.config(cx);
        let subject = match &self.child_form {
            Some(form) if is_designed_form(config, form) => ("form", form.id.clone()),
            _ => ("table", related.table.clone()),
        };
        !props.disabled && props.env.can(subject.0, &subject.1, op, cx)
    }

    fn edit(&mut self, mode: FormMode, record_id: Option<Value>, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(form), props) = (self.child_form.clone(), self.props().clone()) else { return };
        self.message.clear();
        let link = Self::link(&props);
        let this = cx.entity();
        let (notify_to, done_to) = (this.clone(), this.clone());
        let renderer = cx.new(|cx| FormRenderer::new(
            FormRendererProps {
                env: RuntimeEnv { runtime: props.env.runtime.clone(), ..props.env.clone() },
                form_id: form.id.clone(),
                mode: Some(mode),
                record_id: record_id.clone(),
                link,
                embedded: true,
                depth: props.depth + 1,
                on_done: Some(Rc::new(move |cx| {
                    done_to.update(cx, |this, cx| {
                        this.editing = None;
                        this.editor = None;
                        this.load(0, cx);
                        cx.notify();
                    })
                })),
                back: None,
                on_notify: Some(Rc::new(move |text, tone, cx| {
                    notify_to.update(cx, |this, cx| {
                        if tone == NoticeTone::Error {
                            this.error = text;
                        } else {
                            let label = this.props().control.label.clone();
                            this.message = format!("{label}: {text}");
                        }
                        cx.notify();
                    })
                })),
                params: Map::new(),
                on_dirty: None,
                on_saved: None,
            },
            window,
            cx,
        ));
        self.editing = Some((mode, record_id));
        self.editor = Some(renderer);
        cx.notify();
    }

    fn remove(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(rows) = &self.rows else { return };
        let (Some(record), Some(identity)) = (rows.records.get(index).cloned(), rows.identities.get(index).cloned())
        else {
            return;
        };
        let props = self.props().clone();
        let Some(related) = props.control.related.clone() else { return };
        let label = props.control.label.to_lowercase();
        let answer = crate::ui::dialogs::confirm(cx, "Confirm", &format!("Delete this {label} row?"));
        task::after(cx, answer, move |this, ok, cx| {
            if !ok {
                return;
            }
            if !this.allowed(Operation::Delete, cx) {
                this.error = "This role cannot delete these records.".into();
                cx.notify();
                return;
            }
            let (name, env_label) = (related.table.clone(), props.env.label.to_string());
            let schema = this.schema.clone();
            task::spawn_call(
                cx,
                move || {
                    let schema = match schema {
                        Some(s) => s,
                        None => table_schema(&env_label, &name)?,
                    };
                    delete_row(&env_label, &name, &schema, &identity, &record)
                },
                move |this, result, cx| {
                    match result {
                        Ok(()) => {
                            this.error.clear();
                            let env = this.props().env.clone();
                            env.records_changed(cx);
                        }
                        Err(e) => this.error = e.message,
                    }
                    this.load(0, cx);
                    cx.notify();
                },
            );
        });
    }
}

use std::rc::Rc;

impl Render for RelatedRecords {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let props = self.props().clone();
        let theme = cx.theme().clone();
        let Some(related) = props.control.related.clone() else { return div().into_any_element() };
        let name = if props.control.label.is_empty() { humanize(&related.table) } else { props.control.label.clone() };
        let slug = id_of(&props.control);
        let region = |content: Vec<AnyElement>| {
            v_flex()
                .id(SharedString::from(format!("related:{slug}")))
                .aria_label(name.clone())
                .gap_2()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .child(div().font_weight(FontWeight::SEMIBOLD).child(name.clone()))
                .children(content)
                .test_support()
                .into_any_element()
        };
        let hint = |text: String| {
            vec![widgets::styled_text(SharedString::from(format!("related:{slug}:hint")), text, {
                let color = theme.muted_foreground;
                move |d| d.text_sm().text_color(color)
            })
            .into_any_element()]
        };
        if props.depth >= MAX_SUBFORM_DEPTH {
            return region(hint(format!(
                "Related records nest at most {MAX_SUBFORM_DEPTH} levels deep, so {} are not shown here.",
                name.to_lowercase()
            )));
        }
        if !props.saved {
            return region(hint(format!("Save this record to add {}.", name.to_lowercase())));
        }
        let link = Self::link(&props);
        let columns: Vec<String> = if related.columns.is_empty() {
            self.schema
                .as_ref()
                .map(|s| s.columns.iter().map(|c| c.name.clone()).filter(|c| !link.contains_key(c)).collect())
                .unwrap_or_default()
        } else {
            related.columns.clone()
        };
        let control_of = |column: &str| {
            self.child_form
                .as_ref()
                .and_then(|f| f.controls.iter().find(|c| c.binding.as_ref().is_some_and(|b| b.column == column)))
                .cloned()
        };
        let lower = name.to_lowercase();
        let entities = props.env.config(cx).entities.clone();
        let mut body: Vec<AnyElement> = vec![];
        if !self.error.is_empty() {
            body.push(super::message(format!("related:{slug}:error"), self.error.clone(), true, cx).into_any_element());
        }
        if !self.message.is_empty() {
            body.push(super::message(format!("related:{slug}:message"), self.message.clone(), false, cx).into_any_element());
        }
        if self.allowed(Operation::Create, cx) && self.child_form.is_some() && self.editing.is_none() {
            body.push(
                Button::new(SharedString::from(format!("related:{slug}:add")))
                    .small()
                    .label(format!("Add {lower}"))
                    .on_click(cx.listener(|this, _, window, cx| this.edit(FormMode::Create, None, window, cx)))
                    .into_any_element(),
            );
        }
        body.push(
            h_flex()
                .gap_2()
                .border_b_1()
                .border_color(theme.border)
                .children(columns.iter().map(|column| {
                    let label = control_of(column).map_or_else(|| humanize(column), |c| c.label);
                    div().flex_1().text_sm().font_weight(FontWeight::MEDIUM).child(label)
                }))
                .into_any_element(),
        );
        if let Some(rows) = &self.rows {
            for (index, record) in rows.records.iter().enumerate() {
                let cells = columns.iter().map(|column| {
                    let id = SharedString::from(format!("related:{slug}:cell:{index}:{}", widgets::slug(column)));
                    let boolean = is_boolean_column(
                        control_of(column).is_some_and(|c| c.kind == crate::design::ControlKind::Boolean),
                        self.schema.as_ref().and_then(|s| s.columns.iter().find(|c| &c.name == column)),
                    );
                    let text = if boolean {
                        match boolean_value(record.get(column).unwrap_or(&Value::Null)) {
                            Some(true) => "Yes".to_string(),
                            Some(false) => "No".to_string(),
                            None => String::new(),
                        }
                    } else if let Some(text) = label_for(&self.lookups, &self.labels, column, record) {
                        text
                    } else {
                        let format = field_settings_for(&entities, Some(&related.table), Some(column))
                            .and_then(|f| f.format.clone());
                        let value = record.get(column).unwrap_or(&Value::Null);
                        match format {
                            Some(format) => field_text(value, Some(&format)),
                            None => display_text(value),
                        }
                    };
                    div().flex_1().text_sm().child(widgets::text(id, text))
                });
                let mut actions = h_flex().gap_1();
                if self.allowed(Operation::Update, cx) && self.child_form.is_some() && self.schema.is_some() {
                    let record = record.clone();
                    let identity = rows.identities.get(index).cloned().unwrap_or_default();
                    let label = props.env.label.to_string();
                    let schema = self.schema.clone();
                    actions = actions.child(
                        Button::new(SharedString::from(format!("related:{slug}:edit:{index}")))
                            .small()
                            .ghost()
                            .label("Edit")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(schema) = &schema {
                                    let id = record_id_for(&label, schema, &record, &identity);
                                    this.edit(FormMode::Edit, Some(id), window, cx);
                                }
                            })),
                    );
                }
                if self.allowed(Operation::Delete, cx) {
                    actions = actions.child(
                        Button::new(SharedString::from(format!("related:{slug}:delete:{index}")))
                            .small()
                            .ghost()
                            .label("Delete")
                            .on_click(cx.listener(move |this, _, _, cx| this.remove(index, cx))),
                    );
                }
                body.push(
                    h_flex()
                        .id(SharedString::from(format!("related:{slug}:row:{index}")))
                        .gap_2()
                        .items_center()
                        .children(cells)
                        .child(actions)
                        .test_support()
                        .into_any_element(),
                );
            }
            if rows.records.is_empty() {
                body.push(
                    widgets::styled_text(
                        SharedString::from(format!("related:{slug}:empty")),
                        format!("No {lower} yet."),
                        {
                            let color = theme.muted_foreground;
                            move |d| d.text_sm().text_color(color)
                        },
                    )
                    .into_any_element(),
                );
            }
            let limit = self.limit();
            if rows.total > limit {
                body.push(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(widgets::text(
                            SharedString::from(format!("related:{slug}:pager")),
                            format!(
                                "{}\u{2013}{} of {}",
                                self.offset + 1,
                                (self.offset + limit).min(rows.total),
                                rows.total
                            ),
                        ))
                        .child(
                            Button::new(SharedString::from(format!("related:{slug}:prev")))
                                .small()
                                .label("Previous page")
                                .disabled(!self.can_prev())
                                .on_click(cx.listener(|this, _, _, cx| this.page_by(false, cx))),
                        )
                        .child(
                            Button::new(SharedString::from(format!("related:{slug}:next")))
                                .small()
                                .label("Next page")
                                .disabled(!self.can_next())
                                .on_click(cx.listener(|this, _, _, cx| this.page_by(true, cx))),
                        )
                        .into_any_element(),
                );
            }
        }
        if let (Some(editor), false) = (self.editor.clone(), props.disabled) {
            let _ = window;
            body.push(
                div()
                    .id(SharedString::from(format!("related:{slug}:form")))
                    .aria_label(format!("{name} record"))
                    .p_2()
                    .border_1()
                    .border_color(theme.border)
                    .child(editor)
                    .test_support()
                    .into_any_element(),
            );
        }
        region(body)
    }
}

/// The related list of a control: a keyed entity that takes the latest props each render.
pub fn related_list(props: RelatedProps, prefix: &str, window: &mut Window, cx: &mut App) -> AnyElement {
    let key = SharedString::from(format!("{prefix}related:{}", props.control.id));
    let view = window.use_keyed_state((ElementId::from(key), "related"), cx, |_, _| RelatedRecords::new());
    view.update(cx, |v, cx| v.set_props(props, cx));
    view.into_any_element()
}
