//! Datasheet totals row: one aggregate per column over every filtered row.
use super::{duck_value, q, read::ReadRuntime, DataValue, Filter, LogicalType};
use serde::{Deserialize, Serialize};

/// The aggregates a datasheet totals row offers (Access's Total row).
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TotalFunction {
    Sum,
    Avg,
    Count,
    Min,
    Max,
    Stdev,
    Var,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TotalSpec {
    pub column: String,
    pub function: TotalFunction,
}

fn numeric(t: &LogicalType) -> bool {
    matches!(
        t,
        LogicalType::Integer | LogicalType::Real | LogicalType::Decimal { .. }
    )
}

fn orderable(t: &LogicalType) -> bool {
    !matches!(
        t,
        LogicalType::Blob | LogicalType::Json | LogicalType::Boolean
    )
}

impl ReadRuntime {
    /// One value per spec, aggregated over the rows `filters` select. Sum,
    /// average, standard deviation and variance need a numeric column; min and
    /// max keep the column's type; count counts non-null values.
    pub fn totals(
        &self,
        table: &str,
        filters: &[Filter],
        specs: &[TotalSpec],
    ) -> Result<Vec<DataValue>, String> {
        if specs.is_empty() {
            return Ok(vec![]);
        }
        let _gate = self.read_gate()?;
        let plan = self.page_plan(table, &[], filters)?;
        let mut exprs = vec![];
        for spec in specs {
            let column = plan
                .columns
                .iter()
                .find(|c| c.name == spec.column)
                .ok_or_else(|| format!("Unknown column {:?}", spec.column))?;
            let t = &column.logical_type;
            let col = match t {
                LogicalType::Uuid => format!("CAST({} AS VARCHAR)", q(&column.name)),
                _ => q(&column.name),
            };
            let allowed = match spec.function {
                TotalFunction::Count => true,
                TotalFunction::Min | TotalFunction::Max => orderable(t),
                _ => numeric(t),
            };
            if !allowed {
                return Err(format!(
                    "{:?} total is not available for {} column {:?}",
                    spec.function, t, column.name
                ));
            }
            exprs.push(match spec.function {
                TotalFunction::Sum => format!("sum({col})"),
                TotalFunction::Avg => format!("avg({col})"),
                TotalFunction::Count => format!("count({col})"),
                TotalFunction::Min => format!("min({col})"),
                TotalFunction::Max => format!("max({col})"),
                TotalFunction::Stdev => format!("stddev_samp({col})"),
                TotalFunction::Var => format!("var_samp({col})"),
            });
        }
        // The count query is `SELECT count(*) FROM <source> WHERE <filters>`: aggregate over its FROM.
        let from_where = plan
            .count_sql
            .strip_prefix("SELECT count(*) ")
            .ok_or("Unexpected page query shape")?;
        let sql = format!("SELECT {} {from_where}", exprs.join(", "));
        let raw = self
            .connection
            .query_row(&sql, duckdb::params_from_iter(plan.binds.iter()), |r| {
                (0..specs.len())
                    .map(|i| r.get::<_, duckdb::types::Value>(i))
                    .collect::<duckdb::Result<Vec<_>>>()
            })
            .map_err(|e| e.to_string())?;
        Ok(raw
            .into_iter()
            .zip(specs)
            .map(|(v, spec)| {
                let v = duck_value(v);
                match spec.function {
                    TotalFunction::Min | TotalFunction::Max => plan
                        .columns
                        .iter()
                        .find(|c| c.name == spec.column)
                        .map(|c| c.logical_type.coerce_read(v.clone()))
                        .unwrap_or(v),
                    _ => v,
                }
            })
            .collect())
    }
}
