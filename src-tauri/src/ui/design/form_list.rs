//! The form list (port of `FormList.tsx`): select, create, rename, duplicate and delete
//! forms, "Generate form from table" and "Generate app from tables".
//!
//! Test ids: the list is `forms` (labelled "Forms"); each form is `form:<id>` (its name),
//! with `rename:<id>`, `duplicate:<id>` and `delete:<id>`; the rename input is
//! `rename-input` (labelled "Rename <name>", applied on blur or Enter, Escape cancels);
//! `new-form`, `generate-table` (the table choice), `generate-form`, `generate-app`, the
//! notice `generate-notice` and the problem `generate-error`.

use super::{generate::generate_app, studio::DesignStudio};
use crate::data::TableSchema;
use crate::forms::{
    generate::{GenerateOptions, GeneratedCrud, generate_crud_forms},
    operations::{delete_form, duplicate_form},
    schema::new_form,
};
use crate::manager::AppError;
use crate::recordstore::EntitySettings;
use crate::ui::{controls::input, prelude::*};
use gpui_kit::component::input::{InputEvent, InputState};

/// A rename in progress.
struct Rename {
    form_id: String,
    state: Entity<InputState>,
    _subscription: Subscription,
}

#[derive(Default)]
pub struct ListState {
    rename: Option<Rename>,
    pub table: String,
    pub notice: String,
    pub error: String,
}

/// Builds list and detail forms for a table, with lookups and related lists from the live
/// schema of the other tables.
fn generate_for(label: &str, table: &str, tables: &[String], entities: &[EntitySettings]) -> Result<GeneratedCrud, AppError> {
    let schema = crate::inspect_table(label.to_string(), table.to_string(), None)?;
    let others: Vec<TableSchema> = tables
        .iter()
        .filter(|name| name.as_str() != table)
        .filter_map(|name| crate::inspect_table(label.to_string(), name.clone(), None).ok())
        .collect();
    let targets = others.iter().map(|t| (t.name.clone(), t)).collect();
    let children = others.iter().filter(|t| t.foreign_keys.iter().any(|fk| fk.target_table == table)).collect();
    let options = GenerateOptions { targets, children, child_forms: Default::default(), entities };
    Ok(generate_crud_forms(&schema, &options))
}

impl DesignStudio {
    fn create_form(&mut self, cx: &mut Context<Self>) {
        let count = self.design(cx).forms.len();
        let form = new_form(&format!("Form {}", count + 1), None);
        self.select_form(&form.id, cx);
        self.edit_design("New form", move |d| d.forms.push(form), cx);
    }

    fn copy_form(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(source) = self.design(cx).forms.iter().find(|f| f.id == id).cloned() else {
            return;
        };
        let copy = duplicate_form(&source, None);
        self.select_form(&copy.id, cx);
        self.edit_design("Duplicate form", move |d| d.forms.push(copy), cx);
    }

    fn remove_form(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(next) = self.design(cx).forms.iter().find(|f| f.id != id) {
            let next = next.id.clone();
            self.select_form(&next, cx);
        }
        let id = id.to_string();
        self.edit_design("Delete form", move |d| *d = delete_form(d, &id), cx);
    }

    fn generate_form(&mut self, cx: &mut Context<Self>) {
        let tables = self.tables(cx);
        let Some(table) = Some(self.list.table.clone()).filter(|t| !t.is_empty()).or_else(|| tables.first().cloned()) else {
            return;
        };
        let (label, entities) = {
            let shell = self.shell.read(cx);
            (shell.label().to_string(), shell.config(cx).entities.clone())
        };
        task::spawn_call(
            cx,
            move || generate_for(&label, &table, &tables, &entities),
            |this, result, cx| match result {
                Ok(GeneratedCrud { list, detail, navigation }) => {
                    this.list.error.clear();
                    this.select_form(&detail.id.clone(), cx);
                    this.edit_design(
                        "Generate form",
                        move |d| {
                            d.forms.push(list);
                            d.forms.push(detail);
                            if d.start_page.is_none() {
                                d.start_page = Some(navigation.id.clone());
                            }
                            d.navigation.push(navigation);
                        },
                        cx,
                    );
                }
                Err(e) => {
                    this.list.error = e.message;
                    cx.notify();
                }
            },
        );
    }

    fn generate_all(&mut self, cx: &mut Context<Self>) {
        let (store, label) = {
            let shell = self.shell.read(cx);
            (shell.store.clone(), shell.label().to_string())
        };
        generate_app(store, label, None, cx, |this, result, cx| {
            match result {
                Ok(added) => {
                    this.list.error.clear();
                    this.list.notice = if added > 0 {
                        format!("Added {added} forms and pages.")
                    } else {
                        "Every table already has forms.".to_string()
                    };
                }
                Err(e) => this.list.error = e.message,
            }
            cx.notify();
        });
    }

    fn start_rename(&mut self, form_id: &str, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let initial = name.to_string();
        let state = cx.new(|cx| InputState::new(window, cx).default_value(initial));
        let id = form_id.to_string();
        let subscription = cx.subscribe_in(&state, window, move |this, state, e: &InputEvent, _, cx| {
            if matches!(e, InputEvent::Blur | InputEvent::PressEnter { .. }) && this.list.rename.is_some() {
                let name = state.read(cx).value().trim().to_string();
                this.list.rename = None;
                let current = this.design(cx).forms.iter().find(|f| f.id == id).map(|f| f.name.clone());
                if !name.is_empty() && current.as_deref().is_some_and(|c| c != name) {
                    let id = id.clone();
                    this.edit_design(
                        "Rename form",
                        move |d| {
                            if let Some(f) = d.forms.iter_mut().find(|f| f.id == id) {
                                f.name = name;
                            }
                        },
                        cx,
                    );
                }
                cx.notify();
            }
        });
        state.update(cx, |s, cx| s.focus(window, cx));
        self.list.rename = Some(Rename { form_id: form_id.to_string(), state, _subscription: subscription });
        cx.notify();
    }

    pub(super) fn render_form_list(&mut self, selected: Option<&str>, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, danger, active) = (theme.muted_foreground, theme.danger, theme.accent);
        let forms = self.design(cx).forms.clone();
        let tables = self.tables(cx);
        let no_tables = tables.is_empty();
        let table = Some(self.list.table.clone())
            .filter(|t| tables.contains(t))
            .or_else(|| tables.first().cloned())
            .unwrap_or_default();
        let rows: Vec<_> = forms.iter().map(|form| {
            let id = form.id.clone();
            let is_active = selected == Some(form.id.as_str());
            let renaming = self.list.rename.as_ref().filter(|r| r.form_id == form.id).map(|r| r.state.clone());
            let name = if form.name.is_empty() { "Untitled form".to_string() } else { form.name.clone() };
            let (select_id, rename_id, copy_id, delete_id, rename_name) =
                (id.clone(), id.clone(), id.clone(), id.clone(), form.name.clone());
            let label = match renaming {
                Some(state) => div()
                    .flex_1()
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if event.keystroke.key == "escape" {
                            this.list.rename = None;
                            cx.notify();
                        }
                    }))
                    .child(input(&state, "rename-input", &format!("Rename {}", form.name), false))
                    .into_any_element(),
                None => Button::new(SharedString::from(format!("form:{id}")))
                    .ghost()
                    .small()
                    .flex_1()
                    .start_label(None, name)
                    .selected(is_active)
                    .on_click(cx.listener(move |this, _, _, cx| this.select_form(&select_id, cx)))
                    .into_any_element(),
            };
            h_flex()
                .gap_0p5()
                .rounded_md()
                .when(is_active, |el| el.bg(active))
                .child(label)
                .child(
                    super::ctl::icon_button(SharedString::from(format!("rename:{id}")), IconName::Pencil, "Rename")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.start_rename(&rename_id, &rename_name, window, cx)
                        })),
                )
                .child(
                    super::ctl::icon_button(SharedString::from(format!("duplicate:{id}")), IconName::Copy, "Duplicate")
                        .on_click(cx.listener(move |this, _, _, cx| this.copy_form(&copy_id, cx))),
                )
                .child(
                    super::ctl::icon_button(SharedString::from(format!("delete:{id}")), IconName::Trash, "Delete")
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_form(&delete_id, cx))),
                )
        }).collect();
        let picker = {
            let options = super::studio::same(&tables);
            self.pick(
                "generate-table",
                "Table to generate from",
                &table,
                options,
                "",
                |this, value, cx| {
                    this.list.table = value.to_string();
                    cx.notify();
                },
                cx,
            )
        };
        v_flex()
            .id("forms")
            .aria_label("Forms")
            .gap_1()
            .child(div().text_xs().text_color(muted).child("FORMS"))
            .children(rows)
            .child(
                Button::new("new-form")
                    .outline()
                    .small()
                    .icon(IconName::Plus)
                    .label("New form")
                    .on_click(cx.listener(|this, _, _, cx| this.create_form(cx))),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(picker)
                    .child(
                        Button::new("generate-form")
                            .outline()
                            .small()
                            .icon(IconName::WandSparkles)
                            .label("Generate form from table")
                            .disabled(no_tables)
                            .on_click(cx.listener(|this, _, _, cx| this.generate_form(cx))),
                    )
                    .child(
                        Button::new("generate-app")
                            .outline()
                            .small()
                            .icon(IconName::WandSparkles)
                            .label("Generate app from tables")
                            .disabled(no_tables)
                            .on_click(cx.listener(|this, _, _, cx| this.generate_all(cx))),
                    )
                    .when(!self.list.notice.is_empty(), |el| {
                        el.child(widgets::styled_text("generate-notice", self.list.notice.clone(), |d| d.text_xs()))
                    })
                    .when(!self.list.error.is_empty(), |el| {
                        el.child(widgets::styled_text("generate-error", self.list.error.clone(), move |d| {
                            d.text_xs().text_color(danger)
                        }))
                    }),
            )
            .test_support()
            .into_any_element()
    }
}
