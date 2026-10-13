//! The builder's Group by and Sort-and-limit panels. Port of
//! src/query/builder/GroupByPanel.tsx and SortPanel.tsx.
//!
//! Test ids: `group-by:<alias.column>` (one per chosen column), `group-by-column` (drop-down),
//! `add-group-by`, `remove-group-by:<alias.column>`; `sort-<n>-field`,
//! `sort-<n>-direction`, `add-sort`, `remove-sort-<n>`, `row-limit` (input).

use super::{
    QueryMode,
    choice::Opt,
    inputs::Field,
    joins::columns_of,
    model::{BuilderModel, BuilderSort, Direction, GroupColumn, field_name, new_id},
};
use crate::ui::{prelude::*, widgets};

fn key(g: &GroupColumn) -> String {
    format!("{}.{}", g.source, g.column)
}

impl QueryMode {
    /// Explicit GROUP BY columns. Output fields without an aggregate are grouped
    /// automatically, so this lists only the extra columns, which need not be output.
    pub(super) fn render_group_by(&mut self, model: &BuilderModel, cx: &mut Context<Self>) -> impl IntoElement {
        let used: Vec<String> = model.group_by.iter().map(key).collect();
        let mut options = vec![Opt::new("", "Choose a column…")];
        for s in &model.sources {
            for column in columns_of(&self.schemas, &s.table) {
                let k = format!("{}.{column}", s.alias);
                if !used.contains(&k) {
                    options.push(Opt::same(k));
                }
            }
        }
        let choose = self
            .choice(
                "group-by-column",
                self.group_choice.clone(),
                options,
                |this, value, _, cx| {
                    this.group_choice = value;
                    cx.notify();
                },
                cx,
            )
            .render(cx);
        let rows = model.group_by.iter().map(|g| {
            let k = key(g);
            h_flex()
                .gap_2()
                .items_center()
                .child(widgets::text(format!("group-by:{k}"), k.clone()))
                .child(
                    Button::new(SharedString::from(format!("remove-group-by:{k}")))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Delete)
                        .tooltip(format!("Remove group by {k}"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let k = k.clone();
                            this.edit_model(|m| m.group_by.retain(|g| key(g) != k), cx)
                        })),
                )
        });
        let chosen = self.group_choice.clone();
        super::panel("query-group", "Group by", cx)
            
            .children(rows)
            .child(
                h_flex().gap_2().items_center().child(choose).child(
                    Button::new("add-group-by")
                        .outline()
                        .small()
                        .icon(IconName::Plus)
                        .label("Add group by")
                        .disabled(chosen.is_empty())
                        .on_click(cx.listener(|this, _, _, cx| {
                            let choice = std::mem::take(&mut this.group_choice);
                            if let Some((source, column)) = choice.split_once('.') {
                                let g = GroupColumn {
                                    source: source.into(),
                                    column: column.into(),
                                };
                                this.edit_model(|m| m.group_by.push(g), cx);
                            }
                        })),
                ),
            )
    }

    fn patch_sort(&mut self, id: &str, f: impl FnOnce(&mut BuilderSort), cx: &mut Context<Self>) {
        let id = id.to_string();
        self.edit_model(
            |m| {
                if let Some(o) = m.order_by.iter_mut().find(|o| o.id == id) {
                    f(o);
                }
            },
            cx,
        );
    }

    /// ORDER BY over chosen fields, plus the optional row limit.
    pub(super) fn render_sort(
        &mut self,
        model: &BuilderModel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let field_options: Vec<Opt> = model
            .fields
            .iter()
            .map(|f| {
                let name = field_name(f);
                Opt::new(
                    f.id.clone(),
                    match f.aggregate {
                        Some(a) => format!("{}({name})", a.id()),
                        None => name,
                    },
                )
            })
            .collect();
        let mut rows = vec![];
        for (i, o) in model.order_by.iter().enumerate() {
            let n = i + 1;
            let field = self
                .choice(
                    format!("sort-{n}-field"),
                    o.field_id.clone(),
                    field_options.clone(),
                    {
                        let id = o.id.clone();
                        move |this, value, _, cx| this.patch_sort(&id, |o| o.field_id = value, cx)
                    },
                    cx,
                )
                .render(cx);
            let direction = self
                .choice(
                    format!("sort-{n}-direction"),
                    if o.direction == Direction::Desc { "desc" } else { "asc" },
                    vec![Opt::new("asc", "Ascending"), Opt::new("desc", "Descending")],
                    {
                        let id = o.id.clone();
                        move |this, value, _, cx| {
                            let d = if value == "desc" { Direction::Desc } else { Direction::Asc };
                            this.patch_sort(&id, |o| o.direction = d, cx)
                        }
                    },
                    cx,
                )
                .render(cx);
            let id = o.id.clone();
            rows.push(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(field)
                    .child(direction)
                    .child(
                        Button::new(SharedString::from(format!("remove-sort-{n}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Delete)
                            .tooltip(format!("Remove sort {n}"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let id = id.clone();
                                this.edit_model(|m| m.order_by.retain(|o| o.id != id), cx)
                            })),
                    ),
            );
        }
        let limit = model.limit.map(|l| l.to_string()).unwrap_or_default();
        let limit_input = self.input_el(Field::RowLimit, "row-limit", "Row limit", &limit, "All rows", window, cx);
        let first = model.fields.first().map(|f| f.id.clone());
        super::panel("query-sort", "Sort and limit", cx)
            .children(rows)
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        Button::new("add-sort")
                            .outline()
                            .small()
                            .icon(IconName::ArrowDownUp)
                            .label("Add sort")
                            .disabled(first.is_none())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(field_id) = first.clone() {
                                    this.edit_model(
                                        |m| {
                                            m.order_by.push(BuilderSort {
                                                id: new_id(),
                                                field_id,
                                                direction: Direction::Asc,
                                            })
                                        },
                                        cx,
                                    )
                                }
                            })),
                    )
                    .child(div().text_sm().child("Limit"))
                    .child(div().w(px(120.)).child(limit_input)),
            )
    }
}
