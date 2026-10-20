//! One bound input control (port of `Field` in `src/runtime/controls.tsx`): a label, the
//! input for the control's kind and its error. A `FieldView` is an entity the body keeps per
//! control; the body pushes new `FieldProps` into it on every render.
//!
//! Test ids (`<prefix>` is empty except in repeated rows): `<prefix>field:<label slug>` is the
//! input (text inputs report their text as the snapshot value; a checkbox reports `checked`;
//! selects and relationships are buttons labelled with the shown choice). Choices are
//! `<id>:option:<slug of the choice label>` (`none` for the empty choice), a relationship's
//! search box is `<id>:search`, and the error text is `<id>:error`. Rich-text and attachment
//! ids are listed in `field_formats.rs`.
//!
//! Deviations from the web UI: dates and times are typed as ISO text (no picker); choices
//! open in an inline list; rich text is edited as light markup with a toolbar and a preview.

use super::BodyKey;
use crate::design::{Control, ControlKind, Relationship, Tone};
use crate::forms::conditions::filter_inputs_key;
use crate::forms::data::{
    Choice, cached_relationship_label, query_choices, relationship_choices, relationship_label,
};
use crate::forms::fields::{parse_choices, choices_value, value_text};
use crate::forms::richtext_markup::{markup_rich_text, rich_text_markup};
use crate::forms::mask::{apply_mask, mask_template, parse_mask};
use crate::forms::values::{RecordValues, number_value, same_value};
use crate::ui::prelude::*;
use gpui_kit::component::{
    checkbox::Checkbox,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    switch::Switch,
};
use serde_json::{Map, Value};
use std::rc::Rc;
use std::time::Duration;

pub type ChangeHandler = Rc<dyn Fn(Value, &mut App)>;
pub type KeysHandler = Rc<dyn Fn(Vec<(String, Value)>, &mut App)>;
pub type BlurHandler = Rc<dyn Fn(&mut App)>;
pub type EnterHandler = Rc<dyn Fn(&mut App)>;

/// Everything a field shows; the owner builds it on each render.
#[derive(Clone)]
pub struct FieldProps {
    pub label: SharedString,
    /// Repeated rows show their labels once in a header; the input keeps its accessibility label.
    pub hide_label: bool,
    pub control: Control,
    pub value: Value,
    /// Read-only display (detail mode, disabled by `enabledWhen`, or no permission).
    pub read_only: bool,
    pub error: Option<String>,
    pub tone: Option<Tone>,
    /// Scope for a relationship `filter`: the edited record as `parent`, plus `form`, `app`.
    pub filter_scope: Option<Map<String, Value>>,
    /// Multi-column relationship: current value of every key column (by form column).
    pub key_values: RecordValues,
    /// The form's table, where an attachment control stores its files.
    pub table: Option<String>,
    pub prefix: BodyKey,
    pub window_label: SharedString,
    pub on_change: ChangeHandler,
    pub on_keys: KeysHandler,
    pub on_blur: BlurHandler,
    pub on_enter: EnterHandler,
}

pub struct FieldView {
    props: Option<FieldProps>,
    text: Option<Entity<InputState>>,
    area: Option<Entity<TextareaState>>,
    search: Option<Entity<InputState>>,
    subscriptions: Vec<Subscription>,
    open: bool,
    choices: Vec<Choice>,
    choice_error: Option<String>,
    current_label: String,
    scope_key: String,
    seq: u64,
    pending: Option<Task<()>>,
    // The disabled state last given to the inputs (setting it again would notify and re-render).
    disabled: Option<bool>,
    // An attachment upload is running.
    uploading: bool,
    files_error: Option<String>,
}

fn date_input(value: &Value, kind: ControlKind) -> String {
    if value.is_null() || value.as_str() == Some("") {
        return String::new();
    }
    let text = value_text(value);
    match kind {
        ControlKind::Date => text.chars().take(10).collect(),
        ControlKind::Time => {
            if text.chars().count() > 8 {
                text.chars().skip(11).take(8).collect()
            } else {
                text
            }
        }
        _ => text.replace(' ', "T").chars().take(19).collect(),
    }
}

/// The text a value shows in a text-like input.
pub fn input_text(value: &Value, kind: ControlKind) -> String {
    match kind {
        ControlKind::Date | ControlKind::Time | ControlKind::Datetime => date_input(value, kind),
        ControlKind::RichText if value.is_null() => String::new(),
        ControlKind::RichText => rich_text_markup(&value_text(value)),
        _ => {
            if value.is_null() {
                String::new()
            } else {
                value_text(value)
            }
        }
    }
}

/// The value typed text stands for; None when it is not (yet) a valid value of the kind.
pub fn parse_input(kind: ControlKind, text: &str) -> Option<Value> {
    match kind {
        ControlKind::Number | ControlKind::Decimal => {
            if text.trim().is_empty() {
                return Some(Value::Null);
            }
            text.trim().replace(',', "").parse::<f64>().ok().map(number_value)
        }
        ControlKind::Date | ControlKind::Time | ControlKind::Datetime => {
            Some(if text.is_empty() { Value::Null } else { Value::String(text.to_string()) })
        }
        _ => Some(Value::String(text.to_string())),
    }
}

fn rich_html(markup: &str) -> Value {
    markup_rich_text(markup).map_or(Value::Null, Value::String)
}

/// Element id of a field's input.
pub fn field_id(prefix: &str, label: &str, control_id: &str) -> SharedString {
    let slug = widgets::slug(label);
    format!("{prefix}field:{}", if slug.is_empty() { control_id } else { slug.as_str() }).into()
}

impl FieldView {
    pub fn new() -> Self {
        Self {
            props: None,
            text: None,
            area: None,
            search: None,
            subscriptions: vec![],
            open: false,
            choices: vec![],
            choice_error: None,
            current_label: String::new(),
            scope_key: String::new(),
            seq: 0,
            pending: None,
            disabled: None,
            uploading: false,
            files_error: None,
        }
    }

    fn props(&self) -> &FieldProps {
        self.props.as_ref().expect("field props are set before rendering")
    }

    /// Takes the latest props: creates the inputs on first use and brings their text in line
    /// with the value when the value did not come from typing.
    pub fn set_props(&mut self, props: FieldProps, window: &mut Window, cx: &mut Context<Self>) {
        let kind = props.control.kind;
        let first = self.props.is_none();
        if first {
            self.create_inputs(kind, &props, window, cx);
        }
        let value = props.value.clone();
        let read_only = props.read_only;
        let key_before = self.props.as_ref().map(|p| key_signature(p));
        // Typing changes the input before its change reaches the form: only a new value from
        // outside replaces the text.
        let value_changed = first || self.props.as_ref().is_none_or(|p| !same_value(&p.value, &value));
        self.props = Some(props);
        if let Some(input) = self.text.clone() {
            let typed = input.read(cx).value().to_string();
            if value_changed && !parse_input(kind, &typed).is_some_and(|v| same_value(&v, &value)) {
                let shown = self.masked_text(&input_text(&value, kind));
                input.update(cx, |s, cx| s.set_value(shown, window, cx));
            }
            if self.disabled != Some(read_only) {
                input.update(cx, |s, cx| s.set_disabled(read_only, cx));
            }
        }
        if let Some(area) = self.area.clone() {
            let typed = area.read(cx).value().to_string();
            let shown = input_text(&value, kind);
            let matches = if kind == ControlKind::RichText {
                same_value(&rich_html(&typed), &value) || (typed.is_empty() && value.is_null())
            } else {
                typed == shown
            };
            if value_changed && !matches {
                area.update(cx, |s, cx| s.set_value(shown, window, cx));
            }
            if self.disabled != Some(read_only) {
                area.update(cx, |s, cx| s.set_disabled(read_only, cx));
            }
        }
        self.disabled = Some(read_only);
        if matches!(kind, ControlKind::Relationship | ControlKind::Select) {
            let key_now = key_signature(self.props());
            if first || key_before.as_deref() != Some(key_now.as_str()) {
                self.refresh_label(cx);
            }
            if first && kind == ControlKind::Select {
                self.load_select_choices(cx);
            }
            let scope = self.filter_key();
            if scope != self.scope_key {
                self.scope_key = scope;
                if self.open {
                    self.load_choices(cx);
                }
            }
        }
    }

    fn masked_text(&self, text: &str) -> String {
        match self.props().control.input_mask.as_deref().filter(|m| !m.is_empty()) {
            Some(mask) => apply_mask(&parse_mask(mask), text).display,
            None => text.to_string(),
        }
    }

    fn create_inputs(&mut self, kind: ControlKind, props: &FieldProps, window: &mut Window, cx: &mut Context<Self>) {
        match kind {
            ControlKind::Multiline | ControlKind::RichText => {
                let area = cx.new(|cx| TextareaState::new(window, cx));
                self.subscriptions.push(cx.subscribe(&area, |this, area, event: &InputEvent, cx| match event {
                    InputEvent::Change => {
                        let text = area.read(cx).value().to_string();
                        let kind = this.props().control.kind;
                        let value = if kind == ControlKind::RichText {
                            rich_html(&text)
                        } else {
                            Value::String(text)
                        };
                        this.emit_change(value, cx);
                    }
                    InputEvent::Blur => {
                        let on_blur = this.props().on_blur.clone();
                        on_blur(cx);
                    }
                    _ => {}
                }));
                self.area = Some(area);
            }
            ControlKind::Text
            | ControlKind::Number
            | ControlKind::Decimal
            | ControlKind::Date
            | ControlKind::Time
            | ControlKind::Datetime => {
                let placeholder = match kind {
                    ControlKind::Date => "YYYY-MM-DD",
                    ControlKind::Time => "HH:MM:SS",
                    ControlKind::Datetime => "YYYY-MM-DDTHH:MM:SS",
                    _ => "",
                };
                let mask = props.control.input_mask.clone().filter(|m| !m.is_empty());
                let template = mask.as_deref().map(|m| mask_template(&parse_mask(m)));
                let input = cx.new(|cx| {
                    InputState::new(window, cx).placeholder(template.unwrap_or_else(|| placeholder.to_string()))
                });
                self.subscriptions.push(cx.subscribe_in(&input, window, |this, input, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        let kind = this.props().control.kind;
                        let mut text = input.read(cx).value().to_string();
                        if let Some(mask) = this.props().control.input_mask.clone().filter(|m| !m.is_empty()) {
                            let result = apply_mask(&parse_mask(&mask), &text);
                            if result.display != text {
                                text = result.display.clone();
                                let shown = text.clone();
                                input.update(cx, |s, cx| s.set_value(shown, window, cx));
                            }
                            let stored = if result.stored.is_empty() { Value::Null } else { Value::String(result.stored) };
                            this.emit_change(stored, cx);
                            return;
                        }
                        if let Some(value) = parse_input(kind, &text) {
                            this.emit_change(value, cx);
                        }
                    }
                    InputEvent::PressEnter { .. } => {
                        let on_enter = this.props().on_enter.clone();
                        on_enter(cx);
                    }
                    InputEvent::Blur => {
                        let on_blur = this.props().on_blur.clone();
                        on_blur(cx);
                    }
                    _ => {}
                }));
                self.text = Some(input);
            }
            ControlKind::Relationship => {
                let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search…"));
                self.subscriptions.push(cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.search_changed(cx);
                    }
                }));
                self.search = Some(search);
            }
            _ => {}
        }
    }

    fn emit_change(&mut self, value: Value, cx: &mut Context<Self>) {
        let props = self.props();
        if same_value(&value, &props.value) {
            return;
        }
        let on_change = props.on_change.clone();
        on_change(value, cx);
    }

    // ---- choices ------------------------------------------------------------

    fn filter_key(&self) -> String {
        let props = self.props();
        let Some(rel) = props.control.relationship.as_ref() else {
            return String::new();
        };
        props
            .filter_scope
            .as_ref()
            .map(|scope| filter_inputs_key(rel.filter.as_deref(), scope))
            .unwrap_or_default()
    }

    fn relationship(&self) -> Option<Relationship> {
        self.props().control.relationship.clone()
    }

    fn key_pairs(&self) -> Option<Vec<crate::design::KeyPair>> {
        self.relationship().map(|r| r.keys).filter(|keys| keys.len() > 1)
    }

    /// The stored key: the bound value, or target column to value for a multi-column key.
    fn stored_key(&self) -> Value {
        let props = self.props();
        match self.key_pairs() {
            Some(pairs) => Value::Object(
                pairs
                    .iter()
                    .map(|pair| {
                        (pair.target.clone(), props.key_values.get(&pair.column).cloned().unwrap_or(Value::Null))
                    })
                    .collect(),
            ),
            None => props.value.clone(),
        }
    }

    fn refresh_label(&mut self, cx: &mut Context<Self>) {
        match self.props().control.kind {
            ControlKind::Relationship => {
                let Some(rel) = self.relationship() else { return };
                let key = self.stored_key();
                let label = self.props().window_label.to_string();
                if let Some(cached) = cached_relationship_label(&label, &rel, &key) {
                    self.current_label = cached;
                }
                self.seq += 1;
                let seq = self.seq;
                let composite = self.key_pairs().is_some();
                let fallback = if composite { String::new() } else { value_text(&key) };
                task::spawn_call(
                    cx,
                    move || relationship_label(&label, &rel, &key),
                    move |this, text, cx| {
                        if this.seq == seq || this.open {
                            this.current_label = if text.is_empty() { fallback } else { text };
                            cx.notify();
                        }
                    },
                );
            }
            _ => {}
        }
    }

    fn load_select_choices(&mut self, cx: &mut Context<Self>) {
        let control = self.props().control.clone();
        if let Some(query_id) = control.options_query_id.clone().filter(|q| !q.is_empty()) {
            let label = self.props().window_label.to_string();
            task::spawn_call(
                cx,
                move || query_choices(&label, &query_id),
                |this, result, cx| {
                    match result {
                        Ok(choices) => this.choices = choices,
                        Err(_) => this.choices = vec![],
                    }
                    cx.notify();
                },
            );
        } else {
            self.choices = control
                .options
                .iter()
                .map(|o| Choice {
                    value: Value::String(o.value.clone()),
                    label: if o.label.is_empty() { o.value.clone() } else { o.label.clone() },
                    record: None,
                })
                .collect();
        }
    }

    fn search_changed(&mut self, cx: &mut Context<Self>) {
        // The list re-reads once typing settles.
        self.seq += 1;
        let seq = self.seq;
        self.pending = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(150)).await;
            let _ = this.update(cx, |this, cx| {
                if this.seq == seq {
                    this.load_choices(cx);
                }
            });
        }));
    }

    fn load_choices(&mut self, cx: &mut Context<Self>) {
        if self.props().control.kind != ControlKind::Relationship {
            return;
        }
        let Some(rel) = self.relationship() else { return };
        let search = self.search.as_ref().map(|s| s.read(cx).value().to_string()).unwrap_or_default();
        let label = self.props().window_label.to_string();
        let scope = self.props().filter_scope.clone().unwrap_or_default();
        self.seq += 1;
        let seq = self.seq;
        task::spawn_call(
            cx,
            move || relationship_choices(&label, &rel, &search, 50, &scope),
            move |this, result, cx| {
                if this.seq != seq {
                    return;
                }
                match result {
                    Ok(choices) => {
                        this.choices = choices;
                        this.choice_error = None;
                    }
                    Err(e) => {
                        this.choices = vec![];
                        this.choice_error = Some(e.message);
                    }
                }
                cx.notify();
            },
        );
    }

    fn toggle_open(&mut self, cx: &mut Context<Self>) {
        self.open = !self.open;
        if self.open && self.props().control.kind == ControlKind::Relationship {
            self.load_choices(cx);
        }
        cx.notify();
    }

    fn choose(&mut self, choice: Option<Choice>, cx: &mut Context<Self>) {
        self.open = false;
        let kind = self.props().control.kind;
        if kind == ControlKind::Relationship {
            if let Some(pairs) = self.key_pairs() {
                let values = pairs
                    .iter()
                    .map(|pair| {
                        let value = choice
                            .as_ref()
                            .and_then(|c| c.record.as_ref())
                            .and_then(|r| r.get(&pair.target))
                            .cloned()
                            .unwrap_or(Value::Null);
                        (pair.column.clone(), value)
                    })
                    .collect();
                let on_keys = self.props().on_keys.clone();
                on_keys(values, cx);
                self.current_label = choice.map(|c| c.label).unwrap_or_default();
                cx.notify();
                return;
            }
            self.current_label = choice.as_ref().map(|c| c.label.clone()).unwrap_or_default();
        }
        let value = choice.map_or(Value::Null, |c| c.value);
        let on_change = self.props().on_change.clone();
        on_change(value, cx);
        // Leaving the field validates it.
        let on_blur = self.props().on_blur.clone();
        on_blur(cx);
        cx.notify();
    }

    // ---- rendering ------------------------------------------------------------

    fn choice_list(&self, id: &SharedString, cx: &mut Context<Self>) -> Div {
        let props = self.props();
        let kind = props.control.kind;
        let selected_index = match (kind, self.key_pairs()) {
            (ControlKind::Relationship, Some(pairs)) => self.choices.iter().position(|choice| {
                pairs.iter().all(|pair| {
                    same_value(
                        choice.record.as_ref().and_then(|r| r.get(&pair.target)).unwrap_or(&Value::Null),
                        props.key_values.get(&pair.column).unwrap_or(&Value::Null),
                    )
                })
            }),
            _ => self.choices.iter().position(|choice| same_value(&choice.value, &props.value)),
        };
        let mut seen: Vec<String> = vec![];
        let mut list = v_flex()
            .gap_0p5()
            .p_1()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .child(
                Button::new(SharedString::from(format!("{id}:option:none")))
                    .ghost()
                    .small()
                    .w_full()
                    .start_label(None, "—")
                    .on_click(cx.listener(|this, _, _, cx| this.choose(None, cx))),
            );
        for (index, choice) in self.choices.iter().enumerate() {
            let mut slug = widgets::slug(&choice.label);
            if slug.is_empty() || seen.contains(&slug) {
                slug = format!("{slug}{index}");
            }
            seen.push(slug.clone());
            let picked = choice.clone();
            list = list.child(
                Button::new(SharedString::from(format!("{id}:option:{slug}")))
                    .ghost()
                    .small()
                    .w_full()
                    .start_label(None, choice.label.clone())
                    .selected(selected_index == Some(index))
                    .on_click(cx.listener(move |this, _, _, cx| this.choose(Some(picked.clone()), cx))),
            );
        }
        if let Some(error) = &self.choice_error {
            list = list.child(widgets::styled_text(
                SharedString::from(format!("{id}:choice-error")),
                format!("Choice filter: {error}"),
                |d| d.ty(Type::Body).text_color(gpui_kit::red()),
            ));
        }
        list
    }

    fn render_dropdown(&mut self, id: &SharedString, cx: &mut Context<Self>) -> impl IntoElement {
        let props = self.props().clone();
        let kind = props.control.kind;
        let shown = match kind {
            ControlKind::Relationship => {
                if self.current_label.is_empty() { "—".to_string() } else { self.current_label.clone() }
            }
            _ => match self.choices.iter().find(|c| same_value(&c.value, &props.value)) {
                Some(choice) => choice.label.clone(),
                None if props.value.is_null() || props.value.as_str() == Some("") => "—".to_string(),
                None => value_text(&props.value),
            },
        };
        let read_only = props.read_only;
        if read_only {
            return v_flex()
                .child(
                    widgets::styled_text(id.clone(), if shown == "—" { String::new() } else { shown }, |d| {
                        d.px_3().py_1().rounded_md().border_1().ty(Type::Body)
                    }),
                )
                .into_any_element();
        }
        let toggle = Button::new(id.clone())
            .outline()
            .small()
            .w_full()
            .end_icon_label(IconName::ChevronDown, shown)
            .on_click(cx.listener(|this, _, _, cx| this.toggle_open(cx)));
        let dropdown = self.open.then(|| {
            let search = self.search.clone();
            v_flex()
                .gap_1()
                .when_some(search, |el, search| {
                    el.child(
                        Input::new(&search)
                            .id(SharedString::from(format!("{id}:search")))
                            .aria_label(format!("Search {}", props.label)),
                    )
                })
                .child(self.choice_list(id, cx))
        });
        v_flex().gap_1().child(toggle).children(dropdown).into_any_element()
    }

    fn render_multi_select(&mut self, id: &SharedString, _cx: &mut Context<Self>) -> impl IntoElement {
        let props = self.props().clone();
        let selected = parse_choices(&props.value);
        // The listed options, then stored choices the list no longer has (so they stay visible and removable).
        let mut all: Vec<(String, String)> = props
            .control
            .options
            .iter()
            .map(|o| (o.value.clone(), if o.label.is_empty() { o.value.clone() } else { o.label.clone() }))
            .collect();
        for choice in selected.iter().filter(|c| !props.control.options.iter().any(|o| &o.value == *c)) {
            all.push((choice.clone(), choice.clone()));
        }
        let order: Vec<String> = all.iter().map(|(value, _)| value.clone()).collect();
        h_flex().id(id.clone()).flex_wrap().gap_1().children(all.into_iter().map(|(value, label)| {
            let on = selected.contains(&value);
            let (current, order) = (selected.clone(), order.clone());
            let on_change = props.on_change.clone();
            Button::new(SharedString::from(format!("{id}:option:{}", widgets::slug(&value))))
                .small()
                .outline()
                .label(label)
                .selected(on)
                .disabled(props.read_only)
                .on_click(move |_, _, cx| {
                    let next: Vec<String> = order
                        .iter()
                        .filter(|o| if **o == value { !on } else { current.contains(*o) })
                        .cloned()
                        .collect();
                    on_change(choices_value(&next), cx);
                })
        }))
        .test_support()
        .into_any_element()
    }
}

fn key_signature(props: &FieldProps) -> String {
    let mut key = format!("{}", props.value);
    if let Some(rel) = props.control.relationship.as_ref() {
        for pair in rel.keys.iter().filter(|_| rel.keys.len() > 1) {
            key.push_str(&format!("|{}={}", pair.column, props.key_values.get(&pair.column).unwrap_or(&Value::Null)));
        }
    }
    key
}

impl Render for FieldView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let props = self.props().clone();
        let id = field_id(&props.prefix, &props.label, &props.control.id);
        let kind = props.control.kind;
        let required = props.control.validation.required;
        let theme = cx.theme().clone();
        let tone = props.tone.as_ref().and_then(|t| widgets::tone_color(t, cx));
        let input: AnyElement = match kind {
            ControlKind::Boolean => {
                let on = matches!(
                    &props.value,
                    Value::Bool(true) | Value::String(_) | Value::Number(_)
                ) && crate::forms::values::boolean_value(&props.value).unwrap_or(false);
                let on_change = props.on_change.clone();
                if props.control.variant.as_deref() == Some("toggle") {
                    Switch::new(id.clone())
                        .checked(on)
                        .disabled(props.read_only)
                        .accessibility_label(props.label.clone())
                        .on_click(move |checked, _, cx| on_change(Value::Bool(*checked), cx))
                        .into_any_element()
                } else {
                    Checkbox::new(id.clone())
                        .checked(on)
                        .disabled(props.read_only)
                        .accessibility_label(props.label.clone())
                        .on_click(move |checked, _, cx| on_change(Value::Bool(*checked), cx))
                        .into_any_element()
                }
            }
            ControlKind::Select | ControlKind::Relationship => self.render_dropdown(&id, cx).into_any_element(),
            ControlKind::MultiSelect => self.render_multi_select(&id, cx).into_any_element(),
            ControlKind::Attachment => self.render_attachments(&id, cx).into_any_element(),
            ControlKind::RichText => self.render_rich_text(&id, cx).into_any_element(),
            ControlKind::Multiline => match self.area.as_ref() {
                Some(area) => div()
                    .id(id.clone())
                    .aria_label(props.label.clone())
                    .h(px(84.))
                    .w_full()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .child(Textarea::new(area))
                    .test_support()
                    .into_any_element(),
                None => div().into_any_element(),
            },
            _ => match self.text.as_ref() {
                Some(input) => Input::new(input)
                    .id(id.clone())
                    .aria_label(props.label.clone())
                    .into_any_element(),
                None => div().into_any_element(),
            },
        };
        let inline = kind == ControlKind::Boolean;
        let label = if required && !props.read_only {
            format!("{} *", props.label)
        } else {
            props.label.to_string()
        };
        v_flex()
            .gap_1()
            .w_full()
            .when_some(tone, |el, color| el.border_l_2().border_color(color).pl_2())
            .when(!props.hide_label, |el| {
                el.child(
                    div()
                        .id(SharedString::from(format!("{id}:label")))
                        .aria_label(label.clone())
                        .ty(Type::Body)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(tone.unwrap_or(theme.muted_foreground))
                        .child(label.clone())
                        .test_support(),
                )
            })
            .child(if inline { h_flex().child(input).into_any_element() } else { input })
            .when_some(props.error.clone(), |el, error| {
                el.child(widgets::styled_text(
                    SharedString::from(format!("{id}:error")),
                    error,
                    |d| d.ty(Type::Body).text_color(theme.danger),
                ))
            })
    }
}

#[path = "field_formats.rs"]
mod formats;

#[cfg(test)]
#[path = "field_tests.rs"]
mod tests;
