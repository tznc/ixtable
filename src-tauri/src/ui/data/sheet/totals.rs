//! The aggregates a datasheet totals row offers (port of `src/data/sheet/totals.ts`).

use crate::data::{totals::TotalFunction, Column};
use crate::ui::schema::logical::{logical_of, parse_logical};

/// The totals a column offers, matching the checks in `data/totals.rs`.
pub fn totals_for(column: &Column) -> Vec<TotalFunction> {
    use TotalFunction::*;
    let base = parse_logical(Some(&logical_of(column))).base;
    if ["integer", "real", "decimal"].contains(&base.as_str()) {
        vec![Sum, Avg, Count, Min, Max, Stdev, Var]
    } else if ["blob", "json", "boolean"].contains(&base.as_str()) {
        vec![Count]
    } else {
        vec![Count, Min, Max]
    }
}

/// The key the totals setting stores and the picker shows.
pub fn total_key(function: TotalFunction) -> &'static str {
    match function {
        TotalFunction::Sum => "sum",
        TotalFunction::Avg => "avg",
        TotalFunction::Count => "count",
        TotalFunction::Min => "min",
        TotalFunction::Max => "max",
        TotalFunction::Stdev => "stdev",
        TotalFunction::Var => "var",
    }
}

pub fn total_from_key(key: &str) -> Option<TotalFunction> {
    [
        TotalFunction::Sum,
        TotalFunction::Avg,
        TotalFunction::Count,
        TotalFunction::Min,
        TotalFunction::Max,
        TotalFunction::Stdev,
        TotalFunction::Var,
    ]
    .into_iter()
    .find(|f| total_key(*f) == key)
}
