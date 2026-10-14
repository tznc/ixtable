//! WHERE filters and HAVING conditions over aggregates, as nested all/any groups. Port of
//! src/query/builder/FiltersPanel.tsx.
//!
//! Test ids are the slug of the accessible name, e.g. `filter-1-column`,
//! `filter-1-operator`, `filter-1-compares-with`, `filter-1-parameter`,
//! `filter-1-value`, `filter-match`, `add-filter`, `add-filter-group`,
//! `remove-filter-1`; nested groups read `filter-group-1-match`, `add-filter-to-group-1`.
//! Having uses `having-1-aggregate` and `add-condition`.

use super::{
    QueryMode,
    choice::Opt,
    inputs::{Field, for_each_condition},
    joins::columns_of,
    model::{
        AGGREGATES, Aggregate, BuilderCondition, BuilderGroup, BuilderModel, Combinator,
        FILTER_OPERATORS, FilterOperator, FilterValue, Item, conditions_of, empty_group, new_id,
    },
};
use crate::ui::{prelude::*, widgets};

fn group_at<'a>(model: &'a mut BuilderModel, having: bool, path: &[usize]) -> Option<&'a mut BuilderGroup> {
    let mut group = if having { &mut model.having } else { &mut model.filters };
    for &index in path {
        group = match group.items.get_mut(index)? {
            Item::Group(g) => g,
            Item::Condition(_) => return None,
        };
    }
    Some(group)
}

impl QueryMode {
    fn edit_group(
        &mut self,
        having: bool,
        path: Vec<usize>,
        f: impl FnOnce(&mut BuilderGroup),
        cx: &mut Context<Self>,
    ) {
        self.edit_model(
            |m| {
                if let Some(g) = group_at(m, having, &path) {
                    f(g);
                }
            },
            cx,
        );
    }

    fn patch_condition(&mut self, id: &str, f: impl FnOnce(&mut BuilderCondition), cx: &mut Context<Self>) {
        let id = id.to_string();
        let mut f = Some(f);
        self.edit_model(
            |m| {
                for_each_condition(m, |c| {
                    if c.id == id {
                        if let Some(f) = f.take() {
                            f(c);
                        }
                    }
                })
            },
            cx,
        );
    }

    pub(super) fn render_filters(
        &mut self,
        model: &BuilderModel,
        having: bool,
        cx: &mut Context<Self>,
        window: &mut Window,
    ) -> impl IntoElement {
        let (id, title) = if having {
            ("query-having", "Having")
        } else {
            ("query-filters", "Filters")
        };
        let group = if having { &model.having } else { &model.filters };
        let body = self.render_group(model, having, group, vec![], String::new(), window, cx);
        let count = conditions_of(group).len();
        let (none, noun) = if having {
            ("No conditions on aggregates", "condition")
        } else {
            ("No filters, all rows", "filter")
        };
        let summary = match count {
            0 => none.to_string(),
            1 => format!("1 {noun}"),
            n => format!("{n} {noun}s"),
        };
        crate::ui::section::section(id, title, count > 0, window, cx)
            .count(count)
            .summary(summary)
            .child(body)
            .render(cx)
    }

    #[allow(clippy::too_many_arguments)]
    fn render_group(
        &mut self,
        model: &BuilderModel,
        having: bool,
        group: &BuilderGroup,
        path: Vec<usize>,
        path_label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let scope = if having { "Having" } else { "Filter" };
        let noun = if having { "condition" } else { "filter" };
        let where_ = if path_label.is_empty() {
            scope.to_string()
        } else {
            format!("{scope} {path_label}")
        };
        let columns: Vec<String> = model
            .sources
            .iter()
            .flat_map(|s| {
                columns_of(&self.schemas, &s.table)
                    .into_iter()
                    .map(|c| format!("{}.{c}", s.alias))
            })
            .collect();
        let parameters: Vec<String> = self
            .draft
            .as_ref()
            .map(|d| d.query.parameters.iter().map(|p| p.name.clone()).collect())
            .unwrap_or_default();
        let base = model.sources.first().map(|s| s.alias.clone());

        let combinator = self
            .choice(
                widgets::slug(&format!("{where_} match")),
                if group.combinator == Combinator::Or { "or" } else { "and" },
                vec![Opt::new("and", "all"), Opt::new("or", "any")],
                {
                    let path = path.clone();
                    move |this, value, _, cx| {
                        let combinator = if value == "or" { Combinator::Or } else { Combinator::And };
                        this.edit_group(having, path.clone(), |g| g.combinator = combinator, cx)
                    }
                },
                cx,
            )
            .render(cx);
        let remove_group = (!path.is_empty()).then(|| {
            let (parent, index) = (path[..path.len() - 1].to_vec(), path[path.len() - 1]);
            Button::new(SharedString::from(widgets::slug(&format!("Remove {where_}"))))
                .ghost()
                .xsmall()
                .icon(IconName::Trash)
                .tooltip(format!("Remove {where_}"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.edit_group(having, parent.clone(), |g| {
                        if index < g.items.len() {
                            g.items.remove(index);
                        }
                    }, cx)
                }))
        });

        let mut rows: Vec<AnyElement> = vec![];
        let mut condition_n = 0;
        for (index, item) in group.items.iter().enumerate() {
            match item {
                Item::Group(inner) => {
                    let mut inner_path = path.clone();
                    inner_path.push(index);
                    let label = if path_label.is_empty() {
                        format!("group {}", index + 1)
                    } else {
                        format!("{path_label} group {}", index + 1)
                    };
                    rows.push(self.render_group(model, having, inner, inner_path, label, window, cx));
                }
                Item::Condition(c) => {
                    condition_n += 1;
                    let label = if path_label.is_empty() {
                        format!("{scope} {condition_n}")
                    } else {
                        format!("{where_} {noun} {condition_n}")
                    };
                    let row = self.render_condition(
                        c, &label, &columns, &parameters, having, &path, index, window, cx,
                    );
                    rows.push(row);
                }
            }
        }

        let add_text = if path_label.is_empty() {
            format!("Add {noun}")
        } else {
            format!("Add {noun} to {path_label}")
        };
        let group_text = if path_label.is_empty() {
            format!("Add {noun} group")
        } else {
            format!("Add group to {path_label}")
        };
        let first_column = columns.first().cloned().or_else(|| base.clone().map(|b| format!("{b}.")));
        v_flex()
            .id(SharedString::from(format!("group-{}", widgets::slug(&where_))))
            .gap_2()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().text_sm().child("Match"))
                    .child(combinator)
                    .children(remove_group),
            )
            .children(rows)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new(SharedString::from(widgets::slug(&add_text)))
                            .outline()
                            .small()
                            .icon(IconName::Plus)
                            .label(add_text)
                            .disabled(base.is_none())
                            .on_click(cx.listener({
                                let path = path.clone();
                                move |this, _, _, cx| {
                                    let (source, column) = first_column
                                        .as_deref()
                                        .and_then(|c| c.split_once('.'))
                                        .map_or((String::new(), String::new()), |(s, c)| (s.into(), c.into()));
                                    let condition = BuilderCondition {
                                        id: new_id(),
                                        source,
                                        column,
                                        aggregate: having.then_some(Aggregate::Sum),
                                        operator: FilterOperator::Eq,
                                        value: Some(FilterValue::blank()),
                                    };
                                    this.edit_group(having, path.clone(), |g| g.items.push(Item::Condition(condition)), cx)
                                }
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(widgets::slug(&group_text)))
                            .outline()
                            .small()
                            .icon(IconName::FolderPlus)
                            .label(group_text)
                            .disabled(base.is_none())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.edit_group(having, path.clone(), |g| g.items.push(Item::Group(empty_group())), cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_condition(
        &mut self,
        c: &BuilderCondition,
        label: &str,
        columns: &[String],
        parameters: &[String],
        having: bool,
        path: &[usize],
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let slug = widgets::slug;
        let cid = c.id.clone();
        let value = c.value.clone().unwrap_or_else(FilterValue::blank);
        let aggregate = having.then(|| {
            let mut options = vec![Opt::new("", "Value")];
            options.extend(AGGREGATES.iter().map(|(a, l)| Opt::new(a.id(), *l)));
            self.choice(
                slug(&format!("{label} aggregate")),
                c.aggregate.map_or("", |a| a.id()),
                options,
                {
                    let cid = cid.clone();
                    move |this, value, _, cx| {
                        this.patch_condition(&cid, |c| c.aggregate = Aggregate::from_id(&value), cx)
                    }
                },
                cx,
            )
            .render(cx)
        });
        let column = self
            .choice(
                slug(&format!("{label} column")),
                format!("{}.{}", c.source, c.column),
                columns.iter().map(|c| Opt::same(c.clone())).collect(),
                {
                    let cid = cid.clone();
                    move |this, value, _, cx| {
                        let (source, column) = value.split_once('.').unwrap_or((&value, ""));
                        let (source, column) = (source.to_string(), column.to_string());
                        this.patch_condition(&cid, |c| {
                            c.source = source;
                            c.column = column;
                        }, cx)
                    }
                },
                cx,
            )
            .render(cx);
        let operator = self
            .choice(
                slug(&format!("{label} operator")),
                c.operator.id(),
                FILTER_OPERATORS.iter().map(|o| Opt::new(o.1, o.2)).collect(),
                {
                    let cid = cid.clone();
                    move |this, value, _, cx| {
                        if let Some(op) = FilterOperator::from_id(&value) {
                            this.patch_condition(&cid, |c| c.operator = op, cx)
                        }
                    }
                },
                cx,
            )
            .render(cx);
        let compare = (!c.operator.unary()).then(|| {
            let kind = if matches!(value, FilterValue::Param { .. }) { "param" } else { "value" };
            let first = parameters.first().cloned().unwrap_or_default();
            let compares = self
                .choice(
                    slug(&format!("{label} compares with")),
                    kind,
                    vec![Opt::new("value", "Value"), Opt::new("param", "Parameter")],
                    {
                        let cid = cid.clone();
                        move |this, value, _, cx| {
                            let next = if value == "param" {
                                FilterValue::Param { name: first.clone() }
                            } else {
                                FilterValue::blank()
                            };
                            this.patch_condition(&cid, |c| c.value = Some(next), cx)
                        }
                    },
                    cx,
                )
                .render(cx);
            let operand = match &value {
                FilterValue::Param { name } => {
                    let mut options = vec![Opt::new("", "Choose a parameter…")];
                    options.extend(parameters.iter().map(|p| Opt::new(p.clone(), format!("${p}"))));
                    self.choice(
                        slug(&format!("{label} parameter")),
                        name.clone(),
                        options,
                        {
                            let cid = cid.clone();
                            move |this, value, _, cx| {
                                this.patch_condition(&cid, |c| c.value = Some(FilterValue::Param { name: value }), cx)
                            }
                        },
                        cx,
                    )
                    .render(cx)
                }
                FilterValue::Value { value } => div()
                    .w(px(160.))
                    .child(self.input_el(
                        Field::FilterValue(cid.clone()),
                        slug(&format!("{label} value")),
                        format!("{label} value"),
                        &value.display(),
                        "",
                        window,
                        cx,
                    ))
                    .into_any_element(),
            };
            (compares, operand)
        });
        let (parent, _) = (path.to_vec(), ());
        h_flex()
            .id(SharedString::from(slug(label)))
            .gap_2()
            .items_center()
            .flex_wrap()
            .children(aggregate)
            .child(column)
            .child(operator)
            .children(compare.map(|(a, b)| h_flex().gap_2().child(a).child(b)))
            .child(
                Button::new(SharedString::from(slug(&format!("Remove {label}"))))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Trash)
                    .tooltip(format!("Remove {label}"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let having_ = having;
                        this.edit_group(having_, parent.clone(), |g| {
                            if index < g.items.len() {
                                g.items.remove(index);
                            }
                        }, cx)
                    })),
            )
            .into_any_element()
    }
}
