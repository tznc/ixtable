//! The builder's Group by and Sort-and-limit panels. Port of
//! src/query/builder/GroupByPanel.tsx and SortPanel.tsx.
//!
//! Grouping is decided by the Fields rows (a query with an aggregate groups by every
//! output field without one), so the Group by section only summarizes it. Columns added
//! here are extras that group without being output.
//!
//! Test ids: `group-by:<alias.column>` (one per grouping column), `group-by-source:<alias.column>`
//! ("From Fields" or "Added here"), `group-by-column` (drop-down), `add-group-by`,
//! `remove-group-by:<alias.column>` (extras only); `sort-<n>-field`, `sort-<n>-direction`,
//! `add-sort`, `remove-sort-<n>`, `row-limit` (input).

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

/// The columns the compiled query groups by, in GROUP BY order, and whether each is an
/// extra added in the Group by section (true) or follows from the Fields rows (false).
/// Mirrors `compile::compile_builder`.
pub(super) fn grouping(model: &BuilderModel) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = model.group_by.iter().map(|g| (key(g), true)).collect();
    let sorted_aggregate = model
        .order_by
        .iter()
        .filter_map(|o| model.fields.iter().find(|f| f.id == o.field_id))
        .any(|f| f.aggregate.is_some());
    let aggregated = sorted_aggregate || model.fields.iter().any(|f| f.selected && f.aggregate.is_some());
    if aggregated || !out.is_empty() {
        for f in model.fields.iter().filter(|f| f.selected && f.aggregate.is_none()) {
            let k = format!("{}.{}", f.source, f.column);
            if !out.iter().any(|(existing, _)| *existing == k) {
                out.push((k, false));
            }
        }
    }
    out
}

impl QueryMode {
    /// Explicit GROUP BY columns. Output fields without an aggregate are grouped
    /// automatically, so this lists only the extra columns, which need not be output.
    pub(super) fn render_group_by(
        &mut self,
        model: &BuilderModel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
        let grouped = grouping(model);
        let rows = grouped.iter().map(|(k, extra)| {
            let k = k.clone();
            h_flex()
                .gap_2()
                .items_center()
                .child(widgets::text(format!("group-by:{k}"), k.clone()))
                .child(widgets::styled_text(
                    SharedString::from(format!("group-by-source:{k}")),
                    if *extra { "Added here" } else { "From Fields" },
                    |d| d.text_xs().opacity(0.7),
                ))
                .when(*extra, |row| {
                    let k = k.clone();
                    row.child(
                        Button::new(SharedString::from(format!("remove-group-by:{k}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Trash)
                            .tooltip(format!("Remove group by {k}"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let k = k.clone();
                                this.edit_model(|m| m.group_by.retain(|g| key(g) != k), cx)
                            })),
                    )
                })
        });
        let chosen = self.group_choice.clone();
        let names: Vec<&str> = grouped.iter().map(|(k, _)| k.as_str()).collect();
        let summary = if names.is_empty() {
            "No grouping".to_string()
        } else {
            format!("Grouped by {}", names.join(", "))
        };
        let body = v_flex()
            .gap_2()
            .when(grouped.is_empty(), |el| {
                el.child(widgets::text(
                    "group-by-none",
                    "No grouping. Choose an aggregate on a field to group the others.",
                ))
            })
            .children(rows)
            .child(
                h_flex().gap_2().items_center().child(choose).child(
                    Button::new("add-group-by")
                        .outline()
                        .small()
                        .icon(IconName::Plus)
                        .label("Also group by")
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
            );
        crate::ui::section::section("query-group", "Group by", !grouped.is_empty(), window, cx)
            .count(grouped.len())
            .summary(summary)
            .child(body)
            .render(cx)
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
                            .icon(IconName::Trash)
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
        let sort_names: Vec<String> = model
            .order_by
            .iter()
            .filter_map(|o| {
                let f = model.fields.iter().find(|f| f.id == o.field_id)?;
                let arrow = if o.direction == Direction::Desc { "↓" } else { "↑" };
                Some(format!("{} {arrow}", field_name(f)))
            })
            .collect();
        let mut parts = vec![];
        if !sort_names.is_empty() {
            parts.push(format!("Sorted by {}", sort_names.join(", ")));
        }
        if let Some(l) = model.limit {
            parts.push(format!("limit {l}"));
        }
        let summary = if parts.is_empty() { "Unsorted, all rows".to_string() } else { parts.join(", ") };
        let open = !parts.is_empty();
        let count = model.order_by.len();
        let body = v_flex()
            .gap_2()
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
            );
        crate::ui::section::section("query-sort", "Sort and limit", open, window, cx)
            .count(count)
            .summary(summary)
            .child(body)
            .render(cx)
    }
}
