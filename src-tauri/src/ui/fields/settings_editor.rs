//! Field settings of one table: rich text, attachments, multiple choices and input
//! masks (port of `src/fields/FieldSettingsEditor.tsx` and `MaskField.tsx`). They are
//! definition edits that apply at once and never change the column.
//!
//! Test ids: `<column> format` (select), `<column> input mask` (input) with
//! `<column> mask-hint` (the preview or the problem), `<column> choices` (a text area
//! saved when it loses focus), `remove-unused-files` and `attachment-status`,
//! `no-field-columns`.

use crate::ui::controls::{new_input, input_text, select, text_input};
use crate::forms::{
    field_values::FIELD_FORMATS,
    fields::field_settings_for,
    mask::{mask_issue, mask_template, parse_mask},
};
use crate::data::Column;
use crate::recordstore::{EntitySettings, FieldSettings};
use crate::ui::{
    prelude::*,
    schema::{
        logical::{logical_of, parse_logical},
    },
    shell::Shell,
    store::{ConfigStore, StoreEvent},
};
use gpui_kit::component::input::{InputEvent, InputState, Textarea, TextareaState};
use std::collections::HashMap;

fn blank(field: &FieldSettings) -> bool {
    field.format.as_deref().is_none_or(str::is_empty)
        && field.input_mask.as_deref().is_none_or(str::is_empty)
        && field.options.is_empty()
}

/// Saves a change to one column's field settings as an undoable definition edit. A
/// field with nothing set is removed; the table's entity is created when missing.
pub fn update_field_settings(
    store: &Entity<ConfigStore>,
    table: &str,
    column: &str,
    patch: impl FnOnce(&mut FieldSettings),
    cx: &mut App,
) -> Result<(), crate::manager::AppError> {
    let (table, column) = (table.to_string(), column.to_string());
    let label = format!("Field settings for {table}.{column}");
    store.update(cx, |s, cx| {
        s.update(
            &label,
            move |config| {
                let index = config.entities.iter().position(|e| e.table == table);
                let mut entity = index.map(|i| config.entities[i].clone()).unwrap_or_else(|| EntitySettings {
                    id: uuid::Uuid::new_v4().to_string(),
                    table: table.clone(),
                    concurrency: "optimistic".into(),
                    action_id: None,
                    fields: vec![],
                });
                let mut field = entity
                    .fields
                    .iter()
                    .find(|f| f.column == column)
                    .cloned()
                    .unwrap_or_else(|| FieldSettings {
                        id: uuid::Uuid::new_v4().to_string(),
                        column: column.clone(),
                        ..Default::default()
                    });
                patch(&mut field);
                entity.fields.retain(|f| f.column != column);
                if !blank(&field) {
                    entity.fields.push(field);
                }
                match index {
                    Some(i) => config.entities[i] = entity,
                    None => config.entities.push(entity),
                }
            },
            cx,
        )
        .map(|_| ())
    })
}

pub struct FieldSettingsEditor {
    shell: Entity<Shell>,
    table: String,
    columns: Vec<Column>,
    masks: HashMap<String, Entity<InputState>>,
    choices: HashMap<String, Entity<TextareaState>>,
    notice: String,
    error: String,
    _subscriptions: Vec<Subscription>,
}

fn base_of(column: &Column) -> String {
    parse_logical(Some(&logical_of(column))).base
}

impl FieldSettingsEditor {
    pub fn new(
        shell: Entity<Shell>,
        table: &str,
        columns: Vec<Column>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = shell.read(cx).store.clone();
        let mut subscriptions = vec![cx.subscribe(&store, |_, _, e: &StoreEvent, cx| {
            if matches!(e, StoreEvent::Changed) {
                cx.notify();
            }
        })];
        let mut masks = HashMap::new();
        let mut choices = HashMap::new();
        for column in columns.iter().filter(|c| editable(c)) {
            let field = field_settings_for(&shell.read(cx).config(cx).entities, Some(table), Some(&column.name)).cloned();
            let mask = new_input(window, cx, "e.g. (000) 000-0000", field.as_ref().and_then(|f| f.input_mask.as_deref()).unwrap_or(""));
            let name = column.name.clone();
            subscriptions.push(cx.subscribe(&mask, move |this, state, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    let text = input_text(&state, cx);
                    this.save(&name, move |f| f.input_mask = (!text.is_empty()).then_some(text), cx);
                }
            }));
            masks.insert(column.name.clone(), mask);
            let area = cx.new(|cx| TextareaState::new(window, cx).rows(3));
            let text = field.map(|f| f.options.join("\n")).unwrap_or_default();
            area.update(cx, |s, cx| s.set_value(text, window, cx));
            let name = column.name.clone();
            subscriptions.push(cx.subscribe(&area, move |this, state, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Blur) {
                    let text = state.read(cx).value().to_string();
                    this.save_choices(&name, &text, cx);
                }
            }));
            choices.insert(column.name.clone(), area);
        }
        Self {
            shell,
            table: table.to_string(),
            columns,
            masks,
            choices,
            notice: String::new(),
            error: String::new(),
            _subscriptions: subscriptions,
        }
    }

    fn field(&self, column: &str, cx: &App) -> Option<FieldSettings> {
        field_settings_for(&self.shell.read(cx).config(cx).entities, Some(&self.table), Some(column)).cloned()
    }

    fn save(&mut self, column: &str, patch: impl FnOnce(&mut FieldSettings), cx: &mut Context<Self>) {
        let store = self.shell.read(cx).store.clone();
        let result = update_field_settings(&store, &self.table, column, patch, cx);
        self.error = result.err().map(|e| e.message).unwrap_or_default();
        cx.notify();
    }

    /// Types `text` into the choices box of `column` and saves it, as leaving the box does.
    pub fn set_choices(&mut self, column: &str, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(area) = self.choices.get(column) {
            let text = text.to_string();
            area.update(cx, |s, cx| s.set_value(text, window, cx));
        }
        self.save_choices(column, text, cx);
    }

    /// Choices typed one per line: trimmed, without blanks or repeats.
    fn save_choices(&mut self, column: &str, text: &str, cx: &mut Context<Self>) {
        let mut next: Vec<String> = Vec::new();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            if !next.iter().any(|x| x == line) {
                next.push(line.to_string());
            }
        }
        let current = self.field(column, cx).map(|f| f.options).unwrap_or_default();
        if next != current {
            self.save(column, move |f| f.options = next, cx);
        }
    }

    fn change_format(&mut self, column: &str, format: &str, window: &mut Window, cx: &mut Context<Self>) {
        let format = format.to_string();
        if !format.is_empty() {
            if let Some(mask) = self.masks.get(column) {
                mask.update(cx, |s, cx| s.set_value(String::new(), window, cx));
            }
        }
        self.save(
            column,
            move |f| {
                f.format = (!format.is_empty()).then_some(format.clone());
                if !format.is_empty() {
                    f.input_mask = None;
                }
            },
            cx,
        );
    }

    fn cleanup(&mut self, cx: &mut Context<Self>) {
        let label = self.shell.read(cx).label().to_string();
        task::spawn_call(
            cx,
            move || crate::record_attachments::remove_unused_record_attachments(label),
            |this, result, cx| {
                this.notice = match result {
                    Ok(1) => "Removed 1 unused file.".into(),
                    Ok(n) => format!("Removed {n} unused files."),
                    Err(e) => e.message,
                };
                cx.notify();
            },
        );
    }
}

fn editable(column: &Column) -> bool {
    !column.generated && column.primary_key_position == 0 && matches!(base_of(column).as_str(), "text" | "json")
}

impl Render for FieldSettingsEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = window;
        let editor = cx.entity();
        let theme = cx.theme();
        let (muted, border, danger) = (theme.muted_foreground, theme.border, theme.danger);
        let columns: Vec<Column> = self.columns.iter().filter(|c| editable(c)).cloned().collect();
        let has_attachments = self
            .shell
            .read(cx)
            .config(cx)
            .entities
            .iter()
            .find(|e| e.table == self.table)
            .is_some_and(|e| e.fields.iter().any(|f| f.format.as_deref() == Some("attachment")));
        let rows: Vec<AnyElement> = columns
            .iter()
            .map(|column| {
                let name = column.name.clone();
                let base = base_of(column);
                let field = self.field(&name, cx);
                let format = field.as_ref().and_then(|f| f.format.clone()).unwrap_or_default();
                let mut options = vec![(String::new(), "Plain".to_string())];
                options.extend(
                    FIELD_FORMATS
                        .iter()
                        .filter(|(_, _, types)| types.contains(&base.as_str()))
                        .map(|(value, label, _)| (value.to_string(), label.to_string())),
                );
                let pick_editor = editor.clone();
                let pick_name = name.clone();
                let mask_text = field.as_ref().and_then(|f| f.input_mask.clone()).unwrap_or_default();
                let issue = (!mask_text.is_empty()).then(|| mask_issue(&mask_text)).flatten();
                h_flex()
                    .flex_wrap()
                    .items_start()
                    .gap_3()
                    .py_2()
                    .border_b_1()
                    .border_color(border)
                    .child(div().min_w(px(112.)).font_weight(FontWeight::SEMIBOLD).child(name.clone()))
                    .child(select(
                        SharedString::from(format!("{name} format")),
                        &format,
                        options,
                        "Plain",
                        move |value, window, cx| {
                            let (value, name) = (value.to_string(), pick_name.clone());
                            pick_editor.update(cx, |this, cx| this.change_format(&name, &value, window, cx));
                        },
                    ))
                    .when(format.is_empty() && base == "text", |el| {
                        let mask = self.masks[&name].clone();
                        el.child(
                            v_flex()
                                .gap_1()
                                .child(div().w(px(220.)).child(text_input(
                                    &mask,
                                    SharedString::from(format!("{name} input mask")),
                                    &format!("{name} input mask"),
                                )))
                                .when(!mask_text.is_empty(), |el| {
                                    let (text, color) = match issue {
                                        Some(problem) => (problem.to_string(), danger),
                                        None => (
                                            format!("Entry looks like {}", mask_template(&parse_mask(&mask_text))),
                                            muted,
                                        ),
                                    };
                                    el.child(div().text_xs().text_color(color).child(widgets::text(
                                        SharedString::from(format!("{name} mask-hint")),
                                        text,
                                    )))
                                }),
                        )
                    })
                    .when(format == "multiSelect", |el| {
                        let area = self.choices[&name].clone();
                        el.child(
                            v_flex()
                                .gap_1()
                                .child(div().text_sm().text_color(muted).child("Choices (one per line)"))
                                .child(
                                    div()
                                        .id(SharedString::from(format!("{name} choices")))
                                        .w(px(240.))
                                        .child(Textarea::new(&area).aria_label(format!("{name} choices"))),
                                ),
                        )
                    })
                    .into_any_element()
            })
            .collect();
        v_flex()
            .id("Field settings")
            .gap_2()
            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child("Field settings"))
            .child(div().text_sm().text_color(muted).child(
                "How text and JSON columns are entered and shown in forms, the datasheet, and reports. These apply at once and do not change the column.",
            ))
            .when(columns.is_empty(), |el| {
                el.child(div().child(widgets::text("no-field-columns", "This table has no text or JSON columns.")))
            })
            .children(rows)
            .when(!self.error.is_empty(), |el| {
                el.child(div().text_sm().text_color(danger).child(widgets::text("field-settings-error", self.error.clone())))
            })
            .when(has_attachments, |el| {
                el.child(
                    h_flex()
                        .gap_3()
                        .items_center()
                        .py_2()
                        .child(
                            Button::new("remove-unused-files")
                                .danger()
                                .outline()
                                .small()
                                .label("Remove unused files")
                                .on_click(cx.listener(|this, _, _, cx| this.cleanup(cx))),
                        )
                        .child(div().text_sm().text_color(muted).child(widgets::text(
                            "attachment-status",
                            if self.notice.is_empty() {
                                "Deletes stored files that no record refers to and that are over an hour old."
                                    .to_string()
                            } else {
                                self.notice.clone()
                            },
                        ))),
                )
            })
    }
}
