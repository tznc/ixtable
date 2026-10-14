//! The builder's Fields panel: column checkboxes per source, then alias / aggregate /
//! order for the chosen fields. Port of src/query/builder/FieldsPanel.tsx.
//!
//! Test ids: `col:<alias>.<column>` (checkbox), `add-row-count`, and per chosen field
//! (named `alias.column` or `Row count`): `output:<name>`, `aggregate:<name>`,
//! `alias:<name>`, `move-up:<name>`, `move-down:<name>`, `remove-field:<name>`.

use super::{
    QueryMode,
    choice::Opt,
    compile::field_label,
    inputs::Field,
    joins::columns_of,
    model::{AGGREGATES, Aggregate, BuilderField, BuilderModel, field_name},
};
use crate::ui::{prelude::*, widgets};
use gpui_kit::component::checkbox::Checkbox;

impl QueryMode {
    fn patch_field(&mut self, id: &str, f: impl FnOnce(&mut BuilderField), cx: &mut Context<Self>) {
        let id = id.to_string();
        self.edit_model(
            |m| {
                if let Some(field) = m.fields.iter_mut().find(|x| x.id == id) {
                    f(field);
                }
            },
            cx,
        );
    }

    fn remove_field(&mut self, id: &str, cx: &mut Context<Self>) {
        let id = id.to_string();
        self.edit_model(
            |m| {
                m.fields.retain(|f| f.id != id);
                m.order_by.retain(|o| o.field_id != id);
            },
            cx,
        );
    }

    fn move_field(&mut self, index: usize, step: isize, cx: &mut Context<Self>) {
        self.edit_model(
            |m| {
                let to = index as isize + step;
                if index < m.fields.len() && to >= 0 && (to as usize) < m.fields.len() {
                    let field = m.fields.remove(index);
                    m.fields.insert(to as usize, field);
                }
            },
            cx,
        );
    }

    fn toggle_column(&mut self, source: &str, column: &str, checked: bool, cx: &mut Context<Self>) {
        let (source, column) = (source.to_string(), column.to_string());
        self.edit_model(
            |m| {
                let existing = m
                    .fields
                    .iter()
                    .find(|f| f.source == source && f.column == column)
                    .map(|f| f.id.clone());
                match (checked, existing) {
                    (true, None) => m.fields.push(BuilderField::new(&source, &column)),
                    (false, Some(id)) => {
                        m.fields.retain(|f| f.id != id);
                        m.order_by.retain(|o| o.field_id != id);
                    }
                    _ => {}
                }
            },
            cx,
        );
    }

    pub(super) fn render_fields(
        &mut self,
        model: &BuilderModel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut groups = vec![];
        for s in &model.sources {
            let boxes = columns_of(&self.schemas, &s.table).into_iter().map(|c| {
                let checked = model.fields.iter().any(|f| f.source == s.alias && f.column == c);
                let (alias, column) = (s.alias.clone(), c.clone());
                Checkbox::new(SharedString::from(format!("col:{alias}.{c}")))
                    .label(format!("{alias}.{c}"))
                    .checked(checked)
                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        this.toggle_column(&alias, &column, *checked, cx)
                    }))
            });
            groups.push(
                v_flex()
                    .gap_1()
                    .child(widgets::styled_text(
                        SharedString::from(format!("columns-of:{}", s.alias)),
                        format!("{} ({})", s.table, s.alias),
                        |d| d.text_xs().font_weight(FontWeight::SEMIBOLD),
                    ))
                    .children(boxes),
            );
        }
        let base = model.sources.first().map(|s| s.alias.clone());
        // "Group by" is only true while the query groups (see the Group by section).
        let value_label = if super::sortgroup::grouping(model).is_empty() { "Value" } else { "Value (group by)" };
        let mut rows = vec![];
        for (i, f) in model.fields.iter().enumerate() {
            let name = field_name(f);
            let alias_input = self.input_el(
                Field::Alias(f.id.clone()),
                format!("alias:{name}"),
                format!("Alias for {name}"),
                f.alias.as_deref().unwrap_or(""),
                &field_label(&BuilderField {
                    alias: None,
                    ..f.clone()
                }),
                window,
                cx,
            );
            let mut options = vec![];
            if f.column != "*" {
                options.push(Opt::new("", value_label));
            }
            options.extend(
                AGGREGATES
                    .iter()
                    .filter(|(a, _)| f.column != "*" || *a == Aggregate::Count)
                    .map(|(a, l)| Opt::new(a.id(), *l)),
            );
            let aggregate = self
                .choice(
                    format!("aggregate:{name}"),
                    f.aggregate.map_or("", |a| a.id()),
                    options,
                    {
                        let id = f.id.clone();
                        move |this, value, _, cx| {
                            this.patch_field(&id, |f| f.aggregate = Aggregate::from_id(&value), cx)
                        }
                    },
                    cx,
                )
                .render(cx);
            let (out_id, up_id, down_id, rm_id) = (f.id.clone(), f.id.clone(), f.id.clone(), f.id.clone());
            let _ = (&up_id, &down_id);
            rows.push(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Checkbox::new(SharedString::from(format!("output:{name}")))
                            .label(name.clone())
                            .checked(f.selected)
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                let checked = *checked;
                                this.patch_field(&out_id, |f| f.selected = checked, cx)
                            })),
                    )
                    .child(aggregate)
                    .child(div().w(px(160.)).child(alias_input))
                    .child(
                        Button::new(SharedString::from(format!("move-up:{name}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::ArrowUp)
                            .tooltip(format!("Move {name} up"))
                            .disabled(i == 0)
                            .on_click(cx.listener(move |this, _, _, cx| this.move_field(i, -1, cx))),
                    )
                    .child(
                        Button::new(SharedString::from(format!("move-down:{name}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::ArrowDown)
                            .tooltip(format!("Move {name} down"))
                            .disabled(i + 1 == model.fields.len())
                            .on_click(cx.listener(move |this, _, _, cx| this.move_field(i, 1, cx))),
                    )
                    .child(
                        Button::new(SharedString::from(format!("remove-field:{name}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Trash)
                            .tooltip(format!("Remove field {name}"))
                            .on_click(cx.listener(move |this, _, _, cx| this.remove_field(&rm_id, cx))),
                    ),
            );
        }
        super::panel("query-fields", "Fields", cx)
            .child(h_flex().gap_6().flex_wrap().items_start().children(groups))
            .children(base.map(|base| {
                Button::new("add-row-count")
                    .outline()
                    .small()
                    .icon(IconName::Hash)
                    .label("Add row count")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let base = base.clone();
                        this.edit_model(
                            |m| {
                                m.fields.push(BuilderField {
                                    aggregate: Some(Aggregate::Count),
                                    ..BuilderField::new(&base, "*")
                                })
                            },
                            cx,
                        )
                    }))
            }))
            .children(rows)
            .when(model.fields.is_empty() && !model.sources.is_empty(), |el| {
                el.child(widgets::text(
                    "no-fields",
                    "No fields chosen: every column is returned.",
                ))
            })
    }
}
