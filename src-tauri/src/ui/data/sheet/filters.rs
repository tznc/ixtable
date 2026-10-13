//! Filter by selection and the filter chips (port of `src/data/sheet/filters.ts`).

use crate::data::{DataValue, Filter, FilterOperator};
use crate::ui::schema::logical::show_value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    Contains,
    StartsWith,
    IsNull,
    IsNotNull,
}

impl Op {
    /// The operator's wire name, shown for operators without a symbol.
    pub fn key(self) -> &'static str {
        match self {
            Op::Eq => "eq",
            Op::Ne => "ne",
            Op::Lt => "lt",
            Op::Lte => "lte",
            Op::Gt => "gt",
            Op::Gte => "gte",
            Op::Contains => "contains",
            Op::StartsWith => "starts_with",
            Op::IsNull => "is_null",
            Op::IsNotNull => "is_not_null",
        }
    }
}

/// A filter the datasheet applies on top of the search box.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetFilter {
    pub column: String,
    pub operator: Op,
    pub value: Option<DataValue>,
}

impl SheetFilter {
    pub fn to_backend(&self) -> Filter {
        Filter {
            column: self.column.clone(),
            operator: match self.operator {
                Op::Eq => FilterOperator::Eq,
                Op::Ne => FilterOperator::Ne,
                Op::Lt => FilterOperator::Lt,
                Op::Lte => FilterOperator::Lte,
                Op::Gt => FilterOperator::Gt,
                Op::Gte => FilterOperator::Gte,
                Op::Contains => FilterOperator::Contains,
                Op::StartsWith => FilterOperator::StartsWith,
                Op::IsNull => FilterOperator::IsNull,
                Op::IsNotNull => FilterOperator::IsNotNull,
            },
            value: self.value.clone(),
            values: None,
        }
    }
}

/// Access's "Filter by selection" for one cell: rows whose `column` equals the value
/// (or, with `exclude`, differs from it). A null cell filters on nullness. Blobs
/// cannot be compared and give no filter.
pub fn selection_filter(column: &str, value: &DataValue, exclude: bool) -> Option<SheetFilter> {
    match value {
        DataValue::Blob(_) => None,
        DataValue::Null => Some(SheetFilter {
            column: column.into(),
            operator: if exclude { Op::IsNotNull } else { Op::IsNull },
            value: None,
        }),
        value => Some(SheetFilter {
            column: column.into(),
            operator: if exclude { Op::Ne } else { Op::Eq },
            value: Some(value.clone()),
        }),
    }
}

/// Adds `filter`, replacing an existing filter on the same column and operator.
pub fn add_filter(filters: &[SheetFilter], filter: SheetFilter) -> Vec<SheetFilter> {
    let mut out: Vec<SheetFilter> = filters
        .iter()
        .filter(|f| !(f.column == filter.column && f.operator == filter.operator))
        .cloned()
        .collect();
    out.push(filter);
    out
}

/// One-line description of a filter for its chip, e.g. `Region = North`.
pub fn describe_filter(filter: &SheetFilter) -> String {
    match filter.operator {
        Op::IsNull => format!("{} is empty", filter.column),
        Op::IsNotNull => format!("{} is not empty", filter.column),
        op => {
            let value = filter.value.as_ref().map(show_value).unwrap_or_default();
            let label = match op {
                Op::Eq => "=",
                Op::Ne => "≠",
                other => other.key(),
            };
            format!("{} {label} {value}", filter.column)
        }
    }
}
