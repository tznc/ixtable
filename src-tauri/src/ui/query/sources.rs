//! The builder's Sources panel (source table and joins) and the builder shell. Port of
//! src/query/builder/SourcesPanel.tsx and BuilderEditor.tsx.
//!
//! Test ids: `source-table`, `join-kind:<alias>`, `join-left:<alias>:<n>`,
//! `join-right:<alias>:<n>`, `join-remove:<alias>`, `join-condition-remove:<alias>:<n>`,
//! `join-add-condition:<alias>`, `suggest-join:<label>`, `join-table`, `add-join`.
//! `<n>` counts from 1.

use super::{
    QueryMode,
    choice::Opt,
    joins::{add_join, columns_of, join_suggestions, remove_source},
    model::{BuilderModel, BuilderSource, JOIN_KINDS, JoinCondition, JoinKind, alias_for, empty_model, new_id},
};
use crate::ui::{dialogs, prelude::*, task, widgets};

impl QueryMode {
    /// Loads column metadata for every table and view (for pickers and FK suggestions)
    /// when the set of names changes.
    pub(super) fn ensure_schemas(&mut self, cx: &mut Context<Self>) {
        let names = self.tables_and_views(cx);
        let key = names.join("\n");
        if self.schema_key.as_deref() == Some(key.as_str()) {
            return;
        }
        self.schema_key = Some(key.clone());
        let label = self.label.clone();
        task::spawn_call(
            cx,
            move || {
                names
                    .into_iter()
                    .filter_map(|name| {
                        crate::inspect_table(label.clone(), name.clone(), None)
                            .ok()
                            .map(|schema| (name, schema))
                    })
                    .collect::<super::joins::Schemas>()
            },
            move |this, schemas, cx| {
                // A newer set of names owns the schemas now.
                if this.schema_key.as_deref() == Some(key.as_str()) {
                    this.schemas = schemas;
                    cx.notify();
                }
            },
        );
    }

    /// The visual query builder: sources and joins, fields, filters, having, sort.
    pub(super) fn render_builder(
        &mut self,
        model: &BuilderModel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.ensure_schemas(cx);
        let sources = self.render_sources(model, cx);
        let rest = (!model.sources.is_empty()).then(|| {
            v_flex()
                .gap_3()
                .child(self.render_fields(model, window, cx))
                .child(self.render_filters(model, false, cx, window))
                .child(self.render_group_by(model, cx))
                .child(self.render_filters(model, true, cx, window))
                .child(self.render_sort(model, window, cx))
        });
        v_flex().gap_3().child(sources).children(rest)
    }

    fn render_sources(&mut self, model: &BuilderModel, cx: &mut Context<Self>) -> impl IntoElement {
        let tables = self.tables_and_views(cx);
        let base = model.sources.first().cloned();
        let mut table_options = vec![Opt::new("", "Choose a table…")];
        table_options.extend(tables.iter().cloned().map(Opt::same));
        let source_choice = self
            .choice(
                "source-table",
                base.as_ref().map_or(String::new(), |b| b.table.clone()),
                table_options.clone(),
                |this, table, window, cx| this.choose_source(table, window, cx),
                cx,
            )
            .render(cx);
        let mut joins = vec![];
        for join in &model.joins {
            let Some(source) = model.sources.iter().find(|s| s.alias == join.source) else {
                continue;
            };
            let index = model.sources.iter().position(|s| s.alias == source.alias).unwrap_or(0);
            let earlier = &model.sources[..index];
            let blank = JoinCondition {
                left_source: base.as_ref().map_or(String::new(), |b| b.alias.clone()),
                ..Default::default()
            };
            let conditions = if join.conditions.is_empty() {
                vec![blank.clone()]
            } else {
                join.conditions.clone()
            };
            let alias = source.alias.clone();
            let kind = self
                .choice(
                    format!("join-kind:{alias}"),
                    join.kind.id(),
                    JOIN_KINDS.iter().map(|k| Opt::new(k.1, k.2)).collect(),
                    {
                        let (join_id, alias) = (join.id.clone(), alias.clone());
                        move |this, value, _, cx| {
                            let kind = JoinKind::from_id(&value).unwrap_or(JoinKind::Inner);
                            let _ = &alias;
                            this.patch_join(&join_id, |j| j.kind = kind, cx)
                        }
                    },
                    cx,
                )
                .render(cx);
            let mut rows = vec![h_flex()
                .gap_2()
                .items_center()
                .child(kind)
                .child(div().text_sm().child(format!("{} AS {}", source.table, source.alias)))
                .child(
                    Button::new(SharedString::from(format!("join-remove:{alias}")))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Delete)
                        .tooltip(format!("Remove join {alias}"))
                        .on_click(cx.listener({
                            let alias = alias.clone();
                            move |this, _, _, cx| this.edit_model(|m| *m = remove_source(m, &alias), cx)
                        })),
                )
                .into_any_element()];
            for (i, condition) in conditions.iter().enumerate() {
                let n = i + 1;
                let mut left_options = vec![Opt::new(
                    format!("{}.", condition.left_source),
                    "Choose…",
                )];
                for s in earlier {
                    for c in columns_of(&self.schemas, &s.table) {
                        left_options.push(Opt::new(format!("{}.{c}", s.alias), format!("{}.{c}", s.alias)));
                    }
                }
                let mut right_options = vec![Opt::new("", "Choose…")];
                for c in columns_of(&self.schemas, &source.table) {
                    right_options.push(Opt::new(c.clone(), format!("{}.{c}", source.alias)));
                }
                let left = self
                    .choice(
                        format!("join-left:{alias}:{n}"),
                        format!("{}.{}", condition.left_source, condition.left_column),
                        left_options,
                        {
                            let (join_id, conditions) = (join.id.clone(), conditions.clone());
                            move |this, value, _, cx| {
                                let (src, col) = value.split_once('.').unwrap_or((&value, ""));
                                let mut next = conditions.clone();
                                next[i].left_source = src.to_string();
                                next[i].left_column = col.to_string();
                                this.patch_join(&join_id, |j| j.conditions = next, cx)
                            }
                        },
                        cx,
                    )
                    .render(cx);
                let right = self
                    .choice(
                        format!("join-right:{alias}:{n}"),
                        condition.right_column.clone(),
                        right_options,
                        {
                            let (join_id, conditions) = (join.id.clone(), conditions.clone());
                            move |this, value, _, cx| {
                                let mut next = conditions.clone();
                                next[i].right_column = value;
                                this.patch_join(&join_id, |j| j.conditions = next, cx)
                            }
                        },
                        cx,
                    )
                    .render(cx);
                rows.push(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().w(px(24.)).text_sm().child(if i == 0 { "on" } else { "and" }))
                        .child(left)
                        .child("=")
                        .child(right)
                        .when(conditions.len() > 1, |el| {
                            el.child(
                                Button::new(SharedString::from(format!("join-condition-remove:{alias}:{n}")))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Delete)
                                    .tooltip(format!("Remove join {alias} condition {n}"))
                                    .on_click(cx.listener({
                                        let (join_id, conditions) = (join.id.clone(), conditions.clone());
                                        move |this, _, _, cx| {
                                            let mut next = conditions.clone();
                                            next.remove(i);
                                            this.patch_join(&join_id, |j| j.conditions = next, cx)
                                        }
                                    })),
                            )
                        })
                        .into_any_element(),
                );
            }
            rows.push(
                Button::new(SharedString::from(format!("join-add-condition:{alias}")))
                    .outline()
                    .small()
                    .icon(IconName::Plus)
                    .label(format!("Add condition to {alias}"))
                    .on_click(cx.listener({
                        let (join_id, conditions, blank) = (join.id.clone(), conditions.clone(), blank.clone());
                        move |this, _, _, cx| {
                            let mut next = conditions.clone();
                            next.push(blank.clone());
                            this.patch_join(&join_id, |j| j.conditions = next, cx)
                        }
                    }))
                    .into_any_element(),
            );
            joins.push(v_flex().gap_1().pl_3().children(rows));
        }
        let suggestions: Vec<_> = if base.is_some() {
            join_suggestions(model, &self.schemas)
        } else {
            vec![]
        };
        let suggest: Vec<_> = suggestions.into_iter().map(|s| {
            Button::new(SharedString::from(format!("suggest-join:{}", s.label)))
                .outline()
                .small()
                .icon(IconName::Link)
                .label(s.label.clone())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.edit_model(
                        |m| *m = add_join(m, &s.table, &s.left_source, &s.conditions, JoinKind::Inner),
                        cx,
                    )
                }))
        }).collect();
        let join_row = base.as_ref().map(|base| {
            let alias = base.alias.clone();
            let mut options = vec![Opt::new("", "Join another table…")];
            options.extend(tables.iter().cloned().map(Opt::same));
            let pick = self
                .choice(
                    "join-table",
                    self.join_table.clone(),
                    options,
                    |this, value, _, cx| {
                        this.join_table = value;
                        cx.notify();
                    },
                    cx,
                )
                .render(cx);
            h_flex().gap_2().items_center().child(pick).child(
                Button::new("add-join")
                    .outline()
                    .small()
                    .icon(IconName::Plus)
                    .label("Add join")
                    .disabled(self.join_table.is_empty())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let table = std::mem::take(&mut this.join_table);
                        let alias = alias.clone();
                        this.edit_model(
                            |m| {
                                *m = add_join(
                                    m,
                                    &table,
                                    &alias,
                                    &[(String::new(), String::new())],
                                    JoinKind::Inner,
                                )
                            },
                            cx,
                        )
                    })),
            )
        });
        super::panel("query-sources", "Sources", cx)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().w(px(40.)).text_sm().child("From"))
                    .child(source_choice)
                    .children(base.as_ref().map(|b| div().text_sm().child(format!("AS {}", b.alias)))),
            )
            .children(joins)
            .when(!suggestions_empty(&self.schemas, model), |el| {
                el.child(widgets::styled_text("suggested-joins", "Suggested joins", |d| {
                    d.text_xs()
                }))
            })
            .child(h_flex().gap_2().flex_wrap().children(suggest))
            .children(join_row)
    }

    fn patch_join(
        &mut self,
        id: &str,
        f: impl FnOnce(&mut super::model::BuilderJoin),
        cx: &mut Context<Self>,
    ) {
        let id = id.to_string();
        self.edit_model(
            |m| {
                if let Some(j) = m.joins.iter_mut().find(|j| j.id == id) {
                    f(j);
                }
            },
            cx,
        );
    }

    /// Chooses the base table; a model with fields or joins is cleared after a confirmation.
    fn choose_source(&mut self, table: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(model) = self.model() else {
            return;
        };
        if model.sources.first().map(|s| s.table.as_str()) == Some(table.as_str()) {
            return;
        }
        let apply = move |this: &mut Self, cx: &mut Context<Self>| {
            let next = if table.is_empty() {
                empty_model()
            } else {
                BuilderModel {
                    sources: vec![BuilderSource {
                        id: new_id(),
                        alias: alias_for(&table, &[]),
                        table,
                    }],
                    ..empty_model()
                }
            };
            this.edit_model(|m| *m = next, cx);
        };
        if model.is_empty() || (model.fields.is_empty() && model.joins.is_empty()) {
            return apply(self, cx);
        }
        let answer = dialogs::confirm(
            cx,
            "Change source table",
            "Changing the source table clears the fields, joins, and filters. Continue?",
        );
        task::after_in(window, cx, answer, move |this, ok, _, cx| {
            if ok {
                apply(this, cx);
            }
        });
    }
}

fn suggestions_empty(schemas: &super::joins::Schemas, model: &BuilderModel) -> bool {
    model.sources.is_empty() || join_suggestions(model, schemas).is_empty()
}
