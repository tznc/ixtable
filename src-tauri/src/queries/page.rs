//! Paged saved-query reads: the saved SQL runs as a subquery, and filters,
//! sorts, LIMIT/OFFSET and the total count are applied around it in DuckDB.
//! Column names are checked against the query's result columns and quoted;
//! every value (query parameters, filter values, limit, offset) is bound.
use super::*;
use crate::data::{like_pattern, q, Filter, FilterOperator, Sort};

/// Largest page one call returns (matches table pages).
pub const MAX_PAGE_SIZE: u64 = 1000;

/// One page of a saved query's rows, plus the exact filtered total.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryPage {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<DataValue>>,
    pub total: u64,
    pub offset: u64,
    pub limit: u64,
}

/// Paging, sort and filter for `page_on`.
#[derive(Clone, Copy)]
pub struct PageSpec<'a> {
    pub offset: u64,
    pub limit: u64,
    pub sorts: &'a [Sort],
    pub filters: &'a [Filter],
}

/// Filter predicates numbered from `$first`, with the values they bind.
fn predicates(filters: &[Filter], first: usize) -> Result<(String, Vec<DuckValue>), AppError> {
    let invalid = |m: &str| AppError::new("VALIDATION_ERROR", m);
    let mut binds = vec![];
    let mut parts = vec![];
    for f in filters {
        let col = q(&f.column);
        let slot = first + binds.len();
        parts.push(match f.operator {
            FilterOperator::IsNull => format!("{col} IS NULL"),
            FilterOperator::IsNotNull => format!("{col} IS NOT NULL"),
            FilterOperator::In => {
                let values = f.values.as_deref().unwrap_or_default();
                if values.is_empty() {
                    "FALSE".to_string()
                } else {
                    let mut slots = vec![];
                    for v in values {
                        slots.push(format!("${}", first + binds.len()));
                        binds.push(bind_value(&f.column, None, v).map_err(|e| invalid(&e))?);
                    }
                    format!("{col} IN ({})", slots.join(", "))
                }
            }
            FilterOperator::Contains | FilterOperator::StartsWith => {
                let text = match &f.value {
                    Some(DataValue::Text(v)) => v,
                    _ => return Err(invalid("Text filter value is required")),
                };
                binds.push(DuckValue::Text(like_pattern(
                    text,
                    matches!(f.operator, FilterOperator::Contains),
                )));
                format!("CAST({col} AS VARCHAR) ILIKE ${slot} ESCAPE '\\'")
            }
            _ => {
                let value = f
                    .value
                    .as_ref()
                    .ok_or_else(|| invalid("Filter value is required"))?;
                binds.push(bind_value(&f.column, None, value).map_err(|e| invalid(&e))?);
                let op = match f.operator {
                    FilterOperator::Eq => "=",
                    FilterOperator::Ne => "<>",
                    FilterOperator::Lt => "<",
                    FilterOperator::Lte => "<=",
                    FilterOperator::Gt => ">",
                    _ => ">=",
                };
                format!("{col} {op} ${slot}")
            }
        });
    }
    let wh = if parts.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", parts.join(" AND "))
    };
    Ok((wh, binds))
}

/// ` ORDER BY ...` for `sorts`, or nothing.
fn order_by(sorts: &[Sort]) -> String {
    let order = sorts
        .iter()
        .map(|s| {
            format!(
                "{} {}",
                q(&s.column),
                if s.descending { "DESC" } else { "ASC" }
            )
        })
        .collect::<Vec<_>>();
    if order.is_empty() {
        String::new()
    } else {
        format!(" ORDER BY {}", order.join(", "))
    }
}

/// Every row of `sql` (a read-only query with `$name` parameters) that
/// `filters` match, in `sorts` order: the SQL to run and the values it binds.
/// Exports run it; `page_on` adds paging to the same shape.
pub fn filtered_sql(
    sql: &str,
    declared: &[QueryParameter],
    supplied: &[NamedValue],
    sorts: &[Sort],
    filters: &[Filter],
) -> Result<(String, Vec<DuckValue>), AppError> {
    let rewritten = prepare_sql(sql)?;
    let mut values = resolve_params(&rewritten.names, declared, supplied, true)?;
    let (wh, binds) = predicates(filters, values.len() + 1)?;
    values.extend(binds);
    // The newline keeps a trailing `-- comment` from swallowing the parenthesis.
    let sql = format!(
        "SELECT * FROM (\n{}\n) AS ixt_page{wh}{}",
        rewritten.sql,
        order_by(sorts)
    );
    Ok((sql, values))
}

/// Runs one page of `sql` (a read-only query with `$name` parameters) on
/// `connection`, with cancellation registered under `window`/`run_id`.
#[allow(clippy::too_many_arguments)]
pub fn page_on(
    connection: &duckdb::Connection,
    window: &str,
    run_id: Option<String>,
    sql: &str,
    declared: &[QueryParameter],
    supplied: &[NamedValue],
    spec: &PageSpec,
) -> Result<QueryPage, AppError> {
    if spec.limit == 0 || spec.limit > MAX_PAGE_SIZE {
        return Err(AppError::new(
            "VALIDATION_ERROR",
            format!("Page size must be between 1 and {MAX_PAGE_SIZE}"),
        ));
    }
    let rewritten = prepare_sql(sql)?;
    let mut values = resolve_params(&rewritten.names, declared, supplied, true)?;
    // The newline keeps a trailing `-- comment` from swallowing the parenthesis.
    let from = format!("(\n{}\n) AS ixt_page", rewritten.sql);
    let run_id = run_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let guard = RunGuard::register(window, &run_id, connection);
    let fail = |e: String| {
        if guard.cancelled.load(Ordering::SeqCst) || e.to_ascii_lowercase().contains("interrupt") {
            AppError::new("CANCELLED", "Query cancelled")
        } else {
            AppError::new("DATABASE_ERROR", e)
        }
    };
    // Sort/filter names must be result columns; a LIMIT 0 probe checks them only after the page query fails.
    let unknown_column = |values: &[DuckValue]| -> Result<(), AppError> {
        let columns = execute(
            connection,
            &format!("SELECT * FROM {from} LIMIT 0"),
            values,
            1,
        )
        .map_err(fail)?
        .columns;
        for name in spec
            .sorts
            .iter()
            .map(|s| &s.column)
            .chain(spec.filters.iter().map(|f| &f.column))
        {
            if !columns.contains(name) {
                return Err(AppError::new(
                    "VALIDATION_ERROR",
                    format!("Unknown column {name:?}"),
                ));
            }
        }
        Ok(())
    };
    let query_values = values.clone();
    let (wh, binds) = predicates(spec.filters, values.len() + 1)?;
    values.extend(binds);
    let order = order_by(spec.sorts);
    let n = values.len();
    values.push(DuckValue::BigInt(spec.limit as i64));
    values.push(DuckValue::BigInt(spec.offset as i64));
    // The filtered total rides along with the page as a window count.
    let sql = format!(
        "SELECT *, count(*) OVER () AS ixt_page_total FROM {from}{wh}{order} LIMIT ${} OFFSET ${}",
        n + 1,
        n + 2
    );
    let mut run = match execute(connection, &sql, &values, spec.limit) {
        Ok(run) => run,
        Err(e) => {
            unknown_column(&query_values)?;
            return Err(fail(e));
        }
    };
    run.columns.pop();
    let mut total = None;
    for row in &mut run.rows {
        if let Some(DataValue::Integer(n)) = row.pop() {
            total = Some(n.max(0) as u64);
        }
    }
    let total = match total {
        Some(total) => total,
        // An empty page (offset past the end) still needs the filtered total.
        None if spec.offset == 0 => 0,
        None => {
            let count_values = &values[..n];
            connection
                .query_row(
                    &format!("SELECT count(*) FROM {from}{wh}"),
                    duckdb::params_from_iter(count_values.iter()),
                    |r| r.get::<_, u64>(0),
                )
                .map_err(|e| fail(e.to_string()))?
        }
    };
    Ok(QueryPage {
        columns: run.columns,
        rows: run.rows,
        total,
        offset: spec.offset,
        limit: spec.limit,
    })
}
