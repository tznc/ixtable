//! Join suggestions from foreign keys, and source edits. Port of
//! src/query/builder/joins.ts.

use super::model::{
    BuilderGroup, BuilderJoin, BuilderModel, BuilderSource, Item, JoinCondition, JoinKind,
    alias_for, new_id,
};
use crate::data::TableSchema;
use std::collections::{BTreeMap, HashSet};

/// Column metadata for every table and view, keyed by name.
pub type Schemas = BTreeMap<String, TableSchema>;

pub fn columns_of(schemas: &Schemas, table: &str) -> Vec<String> {
    schemas
        .get(table)
        .map(|s| s.columns.iter().map(|c| c.name.clone()).collect())
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq)]
pub struct JoinSuggestion {
    pub table: String,
    pub left_source: String,
    /// (left column, right column)
    pub conditions: Vec<(String, String)>,
    pub label: String,
}

/// Joins implied by foreign keys between the model's sources and tables not yet in it.
pub fn join_suggestions(model: &BuilderModel, schemas: &Schemas) -> Vec<JoinSuggestion> {
    let used: HashSet<&str> = model.sources.iter().map(|s| s.table.as_str()).collect();
    let mut out: Vec<JoinSuggestion> = vec![];
    let mut push = |table: &str, left_source: &str, left: &[String], right: &[String]| {
        let conditions: Vec<(String, String)> = left
            .iter()
            .enumerate()
            .map(|(i, l)| (l.clone(), right.get(i).cloned().unwrap_or_default()))
            .collect();
        let label = format!(
            "Join {table} on {}",
            conditions
                .iter()
                .map(|(l, r)| format!("{left_source}.{l} = {table}.{r}"))
                .collect::<Vec<_>>()
                .join(" and ")
        );
        if !out.iter().any(|s| s.label == label) {
            out.push(JoinSuggestion {
                table: table.into(),
                left_source: left_source.into(),
                conditions,
                label,
            });
        }
    };
    for source in &model.sources {
        if let Some(schema) = schemas.get(&source.table) {
            for fk in &schema.foreign_keys {
                if !used.contains(fk.target_table.as_str()) {
                    push(
                        &fk.target_table,
                        &source.alias,
                        &fk.from_columns,
                        &fk.target_columns,
                    );
                }
            }
        }
        for other in schemas.values() {
            if used.contains(other.name.as_str()) {
                continue;
            }
            for fk in &other.foreign_keys {
                if fk.target_table == source.table {
                    push(&other.name, &source.alias, &fk.target_columns, &fk.from_columns);
                }
            }
        }
    }
    out
}

/// Pairs `(later, earlier)` of 1-based condition numbers that repeat an earlier condition.
pub fn duplicate_conditions(conditions: &[JoinCondition]) -> Vec<(usize, usize)> {
    let mut out = vec![];
    for (i, c) in conditions.iter().enumerate() {
        if c.left_column.is_empty() && c.right_column.is_empty() {
            continue;
        }
        if let Some(first) = conditions[..i].iter().position(|e| e == c) {
            out.push((i + 1, first + 1));
        }
    }
    out
}

/// Adds `table` as a joined source with the given conditions.
pub fn add_join(
    model: &BuilderModel,
    table: &str,
    left_source: &str,
    conditions: &[(String, String)],
    kind: JoinKind,
) -> BuilderModel {
    let used: Vec<String> = model.sources.iter().map(|s| s.alias.clone()).collect();
    let alias = alias_for(table, &used);
    let mut next = model.clone();
    next.sources.push(BuilderSource {
        id: new_id(),
        table: table.into(),
        alias: alias.clone(),
    });
    next.joins.push(BuilderJoin {
        id: new_id(),
        kind,
        source: alias,
        conditions: conditions
            .iter()
            .map(|(l, r)| JoinCondition {
                left_source: left_source.into(),
                left_column: l.clone(),
                right_column: r.clone(),
            })
            .collect(),
    });
    next
}

fn prune(group: &BuilderGroup, alias: &str) -> BuilderGroup {
    BuilderGroup {
        id: group.id.clone(),
        combinator: group.combinator,
        items: group
            .items
            .iter()
            .filter(|i| match i {
                Item::Group(_) => true,
                Item::Condition(c) => c.source != alias,
            })
            .map(|i| match i {
                Item::Group(g) => Item::Group(prune(g, alias)),
                other => other.clone(),
            })
            .collect(),
    }
}

/// Removes a joined source and everything that refers to it.
pub fn remove_source(model: &BuilderModel, alias: &str) -> BuilderModel {
    let fields: Vec<_> = model
        .fields
        .iter()
        .filter(|f| f.source != alias)
        .cloned()
        .collect();
    BuilderModel {
        version: model.version,
        sources: model
            .sources
            .iter()
            .filter(|s| s.alias != alias)
            .cloned()
            .collect(),
        joins: model
            .joins
            .iter()
            .filter(|j| j.source != alias)
            .map(|j| BuilderJoin {
                conditions: j
                    .conditions
                    .iter()
                    .filter(|c| c.left_source != alias)
                    .cloned()
                    .collect(),
                ..j.clone()
            })
            .collect(),
        group_by: model
            .group_by
            .iter()
            .filter(|g| g.source != alias)
            .cloned()
            .collect(),
        filters: prune(&model.filters, alias),
        having: prune(&model.having, alias),
        order_by: model
            .order_by
            .iter()
            .filter(|o| fields.iter().any(|f| f.id == o.field_id))
            .cloned()
            .collect(),
        fields,
        limit: model.limit,
    }
}
