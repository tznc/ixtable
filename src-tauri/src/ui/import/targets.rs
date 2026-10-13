//! Where imported columns go (port of `src/import/ImportTargetEditor.tsx`): the
//! columns of a new table, or the mapping onto an existing table's fields.
//!
//! Test ids: `Import <file column>` (checkbox), `Field name for <file column>` (input),
//! `Type for <file column>` (select), `Import primary key` (select); and for an existing
//! table `Field for <file column>` (select) and `mapping-type:<file column>`.

use super::mapping::{ColumnDraft, FieldDrafts};
use crate::data::Column;
use crate::ui::{
    prelude::*,
    schema::{
        controls::{check, input_text, new_input, select, text_input},
        logical::LOGICAL_TYPES,
    },
};
use gpui_kit::component::input::{InputEvent, InputState};

fn type_label(logical: &str) -> String {
    let base = logical.split('(').next().unwrap_or(logical);
    LOGICAL_TYPES
        .iter()
        .find(|(value, _)| *value == base)
        .map_or_else(|| logical.to_string(), |(_, label)| label.to_string())
}

/// New-table columns: include, name, type and the primary key.
pub struct NewTableColumns {
    drafts: Vec<ColumnDraft>,
    names: Vec<Entity<InputState>>,
    primary_key: String,
    _subscriptions: Vec<Subscription>,
}

impl NewTableColumns {
    pub fn new() -> Self {
        Self {
            drafts: vec![],
            names: vec![],
            primary_key: String::new(),
            _subscriptions: vec![],
        }
    }

    pub fn set(&mut self, drafts: Vec<ColumnDraft>, primary_key: String, window: &mut Window, cx: &mut Context<Self>) {
        self._subscriptions.clear();
        self.names = drafts
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let input = new_input(window, cx, "", &d.name);
                self._subscriptions.push(cx.subscribe(&input, move |this, state, e: &InputEvent, cx| {
                    if matches!(e, InputEvent::Change) {
                        let text = input_text(&state, cx);
                        if let Some(draft) = this.drafts.get_mut(i) {
                            draft.name = text;
                        }
                        cx.notify();
                    }
                }));
                input
            })
            .collect();
        self.drafts = drafts;
        self.primary_key = primary_key;
        cx.notify();
    }

    pub fn drafts(&self) -> &[ColumnDraft] {
        &self.drafts
    }

    pub fn primary_key(&self) -> &str {
        &self.primary_key
    }

    pub fn set_primary_key(&mut self, name: &str, cx: &mut Context<Self>) {
        self.primary_key = name.to_string();
        cx.notify();
    }

    fn change(&mut self, i: usize, patch: impl FnOnce(&mut ColumnDraft), cx: &mut Context<Self>) {
        if let Some(draft) = self.drafts.get_mut(i) {
            patch(draft);
            cx.notify();
        }
    }
}

impl Default for NewTableColumns {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for NewTableColumns {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        let mut rows = Vec::new();
        for (i, draft) in self.drafts.iter().enumerate() {
            let source = draft.source.clone();
            let mut options: Vec<(String, String)> = vec![];
            if !LOGICAL_TYPES.iter().any(|(value, _)| *value == draft.logical_type) {
                options.push((draft.logical_type.clone(), draft.logical_type.clone()));
            }
            options.extend(
                LOGICAL_TYPES
                    .iter()
                    .filter(|(value, _)| *value != "decimal")
                    .map(|(value, label)| (value.to_string(), label.to_string())),
            );
            let (toggle, pick) = (entity.clone(), entity.clone());
            rows.push(
                h_flex()
                    .gap_3()
                    .items_center()
                    .py_1()
                    .border_b_1()
                    .border_color(border)
                    .child(check(
                        SharedString::from(format!("Import {source}")),
                        "",
                        draft.include,
                        move |value, _, cx| toggle.update(cx, |this, cx| this.change(i, |d| d.include = value, cx)),
                    ))
                    .child(div().w(px(160.)).truncate().child(source.clone()))
                    .child(div().w(px(180.)).child(text_input(
                        &self.names[i],
                        SharedString::from(format!("Field name for {source}")),
                        &format!("Field name for {source}"),
                    )))
                    .child(select(
                        SharedString::from(format!("Type for {source}")),
                        &draft.logical_type,
                        options,
                        "Choose a type",
                        move |value, _, cx| {
                            let value = value.to_string();
                            pick.update(cx, |this, cx| this.change(i, |d| d.logical_type = value, cx))
                        },
                    ))
                    .into_any_element(),
            );
        }
        let mut keys = vec![(String::new(), "New id column (numbered automatically)".to_string())];
        keys.extend(
            self.drafts
                .iter()
                .filter(|d| d.include)
                .map(|d| (d.name.clone(), d.name.clone())),
        );
        let key_entity = entity.clone();
        v_flex()
            .gap_2()
            .child(div().text_sm().text_color(muted).child("Import \u{b7} File column \u{b7} Field name \u{b7} Type"))
            .children(rows)
            .child(div().text_sm().text_color(muted).child("Primary key"))
            .child(select("Import primary key", &self.primary_key, keys, "New id column (numbered automatically)", move |value, _, cx| {
                let value = value.to_string();
                key_entity.update(cx, |this, cx| this.set_primary_key(&value, cx));
            }))
    }
}

/// Existing-table mapping: each file column goes to a field or is skipped.
pub struct FieldMappingEditor {
    columns: Vec<(String, String)>,
    fields: Vec<Column>,
    mapping: FieldDrafts,
}

impl FieldMappingEditor {
    pub fn new() -> Self {
        Self {
            columns: vec![],
            fields: vec![],
            mapping: vec![],
        }
    }

    /// `columns`: the file's (name, detected logical type) pairs.
    pub fn set(&mut self, columns: Vec<(String, String)>, table: &[Column], mapping: FieldDrafts, cx: &mut Context<Self>) {
        self.columns = columns;
        self.fields = table.iter().filter(|c| !c.generated).cloned().collect();
        self.mapping = mapping;
        cx.notify();
    }

    pub fn mapping(&self) -> &FieldDrafts {
        &self.mapping
    }

    fn assign(&mut self, source: &str, field: &str, cx: &mut Context<Self>) {
        match self.mapping.iter_mut().find(|(s, _)| s == source) {
            Some(entry) => entry.1 = field.to_string(),
            None => self.mapping.push((source.to_string(), field.to_string())),
        }
        cx.notify();
    }
}

impl Default for FieldMappingEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for FieldMappingEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        let mut options = vec![(String::new(), "Skip this column".to_string())];
        options.extend(self.fields.iter().map(|f| {
            let required = !f.nullable && f.default_value.is_none() && !f.auto_increment;
            (
                f.name.clone(),
                format!("{}{}", f.name, if required { " (required)" } else { "" }),
            )
        }));
        v_flex()
            .gap_1()
            .child(div().text_sm().text_color(muted).child("File column \u{b7} Detected type \u{b7} Field"))
            .children(self.columns.iter().map(|(name, logical)| {
                let current = self
                    .mapping
                    .iter()
                    .find(|(s, _)| s == name)
                    .map(|(_, f)| f.clone())
                    .unwrap_or_default();
                let entity = entity.clone();
                let source = name.clone();
                h_flex()
                    .gap_3()
                    .items_center()
                    .py_1()
                    .border_b_1()
                    .border_color(border)
                    .child(div().w(px(160.)).truncate().child(name.clone()))
                    .child(div().w(px(160.)).child(widgets::text(
                        SharedString::from(format!("mapping-type:{name}")),
                        type_label(logical),
                    )))
                    .child(select(
                        SharedString::from(format!("Field for {name}")),
                        &current,
                        options.clone(),
                        "Skip this column",
                        move |value, _, cx| {
                            let (source, value) = (source.clone(), value.to_string());
                            entity.update(cx, |this, cx| this.assign(&source, &value, cx));
                        },
                    ))
            }))
    }
}
