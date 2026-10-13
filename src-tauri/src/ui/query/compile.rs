//! Compiles a builder model to DuckDB SQL. Port of src/query/builder/compile.ts.
//! Identifiers are double-quoted, literal values single-quoted, and parameters
//! emitted as `$name` placeholders that `queries` binds as typed values.

use super::model::{
    Aggregate, BuilderCondition, BuilderField, BuilderGroup, BuilderModel, FilterOperator,
    FilterValue, Item, Literal,
};
use std::collections::HashSet;

/// An inconsistent model; the message is shown under the editor.
#[derive(Debug, Clone, PartialEq)]
pub struct BuilderError(pub String);

impl std::fmt::Display for BuilderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

type Built<T> = Result<T, BuilderError>;

fn err<T>(message: impl Into<String>) -> Built<T> {
    Err(BuilderError(message.into()))
}

fn valid_param_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Double-quotes an identifier, doubling embedded quotes.
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// SQL literal for a filter value. Strings are single-quoted with quotes doubled.
pub fn quote_literal(value: &Literal) -> Built<String> {
    Ok(match value {
        Literal::Null => "NULL".into(),
        Literal::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Literal::Number(n) => {
            if !n.is_finite() {
                return err("Numbers in filters must be finite");
            }
            n.to_string()
        }
        Literal::Text(t) => format!("'{}'", t.replace('\'', "''")),
    })
}

fn column(source: &str, name: &str, aliases: &HashSet<&str>) -> Built<String> {
    if !aliases.contains(source) {
        return err(format!("Unknown source {source}"));
    }
    if name == "*" {
        return Ok("*".into());
    }
    Ok(format!("{}.{}", quote_ident(source), quote_ident(name)))
}

fn aggregate(kind: Option<Aggregate>, expr: &str) -> Built<String> {
    let Some(kind) = kind else {
        if expr == "*" {
            return err("* is only allowed with Count");
        }
        return Ok(expr.to_string());
    };
    if expr == "*" && kind != Aggregate::Count {
        return err("* is only allowed with Count");
    }
    Ok(match kind {
        Aggregate::CountDistinct => format!("count(DISTINCT {expr})"),
        other => format!("{}({expr})", other.id()),
    })
}

/// Output column name for a field: its alias, or e.g. `sum_amount` for aggregates.
pub fn field_label(field: &BuilderField) -> String {
    if let Some(alias) = field.alias.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
        return alias.to_string();
    }
    let Some(kind) = field.aggregate else {
        return field.column.clone();
    };
    let suffix = if field.column == "*" {
        String::new()
    } else {
        format!("_{}", field.column)
    };
    let name = if kind == Aggregate::CountDistinct {
        "count_distinct"
    } else {
        kind.id()
    };
    format!("{name}{suffix}")
}

fn operand(value: Option<&FilterValue>) -> Built<String> {
    match value {
        None => err("Filter needs a value"),
        Some(FilterValue::Param { name }) => {
            if !valid_param_name(name) {
                return err(format!("Invalid parameter name {name:?}"));
            }
            Ok(format!("${name}"))
        }
        Some(FilterValue::Value { value }) => quote_literal(value),
    }
}

fn condition(c: &BuilderCondition, aliases: &HashSet<&str>, allow_aggregate: bool) -> Built<String> {
    if c.aggregate.is_some() && !allow_aggregate {
        return err(format!(
            "Filter on {} uses an aggregate; move it to Having",
            c.column
        ));
    }
    let expr = aggregate(c.aggregate, &column(&c.source, &c.column, aliases)?)?;
    if c.operator.unary() {
        let not = if c.operator == FilterOperator::IsNull { "" } else { "NOT " };
        return Ok(format!("{expr} IS {not}NULL"));
    }
    let rhs = operand(c.value.as_ref())?;
    Ok(match c.operator {
        FilterOperator::Contains => format!("contains(lower(CAST({expr} AS VARCHAR)), lower({rhs}))"),
        FilterOperator::StartsWith => {
            format!("starts_with(lower(CAST({expr} AS VARCHAR)), lower({rhs}))")
        }
        op => format!("{expr} {} {rhs}", op.id()),
    })
}

fn group(g: &BuilderGroup, aliases: &HashSet<&str>, allow_aggregate: bool) -> Built<String> {
    let mut parts = vec![];
    for item in &g.items {
        let part = match item {
            Item::Group(inner) => group(inner, aliases, allow_aggregate)?,
            Item::Condition(c) => condition(c, aliases, allow_aggregate)?,
        };
        if !part.is_empty() {
            parts.push(part);
        }
    }
    if parts.len() <= 1 {
        return Ok(parts.pop().unwrap_or_default());
    }
    let joiner = if g.combinator == super::model::Combinator::Or {
        " OR "
    } else {
        " AND "
    };
    Ok(parts
        .iter()
        .map(|p| format!("({p})"))
        .collect::<Vec<_>>()
        .join(joiner))
}

/// Compiles a builder model to DuckDB SQL. Errors for an inconsistent model.
pub fn compile_builder(model: &BuilderModel) -> Built<String> {
    let Some(base) = model.sources.first() else {
        return err("Choose a source table");
    };
    let mut aliases: HashSet<&str> = HashSet::new();
    for s in &model.sources {
        if s.table.is_empty() || s.alias.is_empty() {
            return err("Every source needs a table and an alias");
        }
        if !aliases.insert(s.alias.as_str()) {
            return err(format!("Alias {} is used twice", s.alias));
        }
    }
    let mut lines: Vec<String> = vec![];
    let selected: Vec<&BuilderField> = model.fields.iter().filter(|f| f.selected).collect();
    let expr = |f: &BuilderField| aggregate(f.aggregate, &column(&f.source, &f.column, &aliases)?);
    let projection = if !selected.is_empty() {
        let mut parts = vec![];
        for f in &selected {
            parts.push(format!("{} AS {}", expr(f)?, quote_ident(&field_label(f))));
        }
        parts.join(", ")
    } else if !model.group_by.is_empty() {
        let mut parts = vec![];
        for g in &model.group_by {
            parts.push(column(&g.source, &g.column, &aliases)?);
        }
        parts.join(", ")
    } else if !model.fields.is_empty() {
        return err("Select at least one output field");
    } else {
        "*".into()
    };
    lines.push(format!("SELECT {projection}"));
    lines.push(format!(
        "FROM {} AS {}",
        quote_ident(&base.table),
        quote_ident(&base.alias)
    ));
    let mut available: HashSet<&str> = HashSet::from([base.alias.as_str()]);
    for source in &model.sources[1..] {
        let Some(join) = model.joins.iter().find(|j| j.source == source.alias) else {
            return err(format!("Choose how to join {}", source.table));
        };
        if join.conditions.is_empty() {
            return err(format!("Join to {} needs a condition", source.table));
        }
        let mut on = vec![];
        for c in &join.conditions {
            if !available.contains(c.left_source.as_str()) {
                return err(format!(
                    "Join to {} refers to {}",
                    source.table, c.left_source
                ));
            }
            if c.left_column.is_empty() || c.right_column.is_empty() {
                return err(format!("Join to {} needs both columns", source.table));
            }
            on.push(format!(
                "{} = {}",
                column(&c.left_source, &c.left_column, &aliases)?,
                column(&source.alias, &c.right_column, &aliases)?
            ));
        }
        lines.push(format!(
            "{} {} AS {} ON {}",
            join.kind.sql(),
            quote_ident(&source.table),
            quote_ident(&source.alias),
            on.join(" AND ")
        ));
        available.insert(source.alias.as_str());
    }
    let wh = group(&model.filters, &aliases, false)?;
    if !wh.is_empty() {
        lines.push(format!("WHERE {wh}"));
    }
    let sort_fields: Vec<Option<&BuilderField>> = model
        .order_by
        .iter()
        .map(|o| model.fields.iter().find(|f| f.id == o.field_id))
        .collect();
    let aggregated = selected.iter().any(|f| f.aggregate.is_some())
        || sort_fields.iter().any(|f| f.is_some_and(|f| f.aggregate.is_some()));
    let mut grouping: Vec<String> = vec![];
    for g in &model.group_by {
        grouping.push(column(&g.source, &g.column, &aliases)?);
    }
    let groups = aggregated || !grouping.is_empty();
    if groups {
        for f in selected.iter().filter(|f| f.aggregate.is_none()) {
            let c = column(&f.source, &f.column, &aliases)?;
            if !grouping.contains(&c) {
                grouping.push(c);
            }
        }
    }
    if !grouping.is_empty() {
        lines.push(format!("GROUP BY {}", grouping.join(", ")));
    }
    if groups {
        for o in &model.order_by {
            if let Some(field) = model.fields.iter().find(|f| f.id == o.field_id) {
                if field.aggregate.is_none() && !grouping.contains(&expr(field)?) {
                    return err(format!("Sorting by {} needs it in Group by", field.column));
                }
            }
        }
    }
    let having = group(&model.having, &aliases, true)?;
    if !having.is_empty() {
        lines.push(format!("HAVING {having}"));
    }
    if !model.order_by.is_empty() {
        let mut order = vec![];
        for o in &model.order_by {
            let Some(field) = model.fields.iter().find(|f| f.id == o.field_id) else {
                return err("Sort refers to a removed field");
            };
            let dir = if o.direction == super::model::Direction::Desc {
                "DESC"
            } else {
                "ASC"
            };
            order.push(format!("{} {dir}", expr(field)?));
        }
        lines.push(format!("ORDER BY {}", order.join(", ")));
    }
    if let Some(limit) = model.limit {
        if limit.fract() != 0.0 || limit < 0.0 || !limit.is_finite() {
            return err("Limit must be a whole number");
        }
        lines.push(format!("LIMIT {}", limit as i64));
    }
    Ok(lines.join("\n"))
}

/// `compile_builder` that returns the error message instead of an error:
/// `(sql, error)`, one of them empty.
pub fn try_compile(model: &BuilderModel) -> (String, String) {
    match compile_builder(model) {
        Ok(sql) => (sql, String::new()),
        Err(e) => (String::new(), e.0),
    }
}
