//! Executes planned action queries (`action::Plan`) on the writer connection.
//!
//! `direct` runs the statements on the read-write attachment in one
//! transaction. When triggers need the changed rows, it first copies the
//! table's keys (inserts) or rows (updates) and compares after the statement.
//! `compute_update` runs an embedded-SQLite UPDATE on a copy of the table and
//! returns the changed rows, joined to the live table by rowid, so even a
//! changed primary key is found; it never writes the file. `write_back` then
//! writes them in one DuckDB transaction (see `write_back`).
use super::action::{ChangedRows, UpdatedRow, WORK};
use crate::archive::ActionKind;
use crate::data::logical::LogicalType;
use crate::data::{self, DataValue, NamedValue};
use crate::manager::AppError;
use duckdb::types::Value as DuckValue;

const BEFORE: &str = "temp.main.__ixtable_before";
const ROWID: &str = "__ixtable_rowid";

/// What a run works on.
pub struct Job {
    /// The table, qualified in the `data` catalog.
    pub table: String,
    /// The table's plain name (RecordStore writes).
    pub name: String,
    /// Primary key columns; empty when the table has none.
    pub keys: Vec<String>,
    /// Whether the table has a rowid (SQLite tables not declared WITHOUT ROWID).
    pub rowid: bool,
    pub kind: ActionKind,
    /// Collect the changed rows for triggers.
    pub watch: bool,
    pub dry_run: bool,
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub changed: u64,
    pub removed: u64,
    pub rows: ChangedRows,
}

/// A computed embedded-SQLite UPDATE: the changes `write_back` still has to write.
pub struct Computed {
    pub outcome: Outcome,
    pub changes: Changes,
}

/// Changed rows of an embedded-SQLite UPDATE.
#[derive(Debug, Default)]
pub struct Changes {
    /// How a row is found: `rowid`, or the primary key columns (old values), with DuckDB types.
    pub key_columns: Vec<(String, String)>,
    /// Every column some row changed, with its DuckDB type in the typed attachment.
    pub columns: Vec<(String, String)>,
    /// Per row: the key values, then per column the new value, or None when unchanged.
    pub rows: Vec<(Vec<DataValue>, Vec<Option<DataValue>>)>,
}

pub fn database_error(message: &str) -> AppError {
    let code = if message.to_ascii_lowercase().contains("constraint") {
        "CONSTRAINT"
    } else {
        "DATABASE_ERROR"
    };
    let message = message.trim_start_matches("Invalid Error: ");
    AppError::new(code, message.to_string())
}

fn failed(e: duckdb::Error) -> AppError {
    database_error(&e.to_string())
}

/// Runs `f` in a transaction that commits only when `commit` and `f` succeeded.
fn in_transaction<T>(
    connection: &duckdb::Connection,
    commit: bool,
    f: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    connection
        .execute_batch("BEGIN TRANSACTION")
        .map_err(failed)?;
    let result = f();
    let end = if result.is_ok() && commit {
        "COMMIT"
    } else {
        "ROLLBACK"
    };
    let ended = connection.execute_batch(end);
    let out = result?;
    ended.map_err(failed)?;
    Ok(out)
}

fn run_statement(
    connection: &duckdb::Connection,
    sql: &str,
    values: &[DuckValue],
) -> Result<u64, AppError> {
    let mut stmt = connection.prepare(sql).map_err(failed)?;
    let n = stmt.parameter_count().min(values.len());
    Ok(stmt
        .execute(duckdb::params_from_iter(values[..n].iter()))
        .map_err(failed)? as u64)
}

fn select_rows(
    connection: &duckdb::Connection,
    sql: &str,
) -> Result<(Vec<String>, Vec<Vec<DataValue>>), AppError> {
    let mut stmt = connection.prepare(sql).map_err(failed)?;
    let mut rows = vec![];
    let mut cursor = stmt.query([]).map_err(failed)?;
    while let Some(row) = cursor.next().map_err(failed)? {
        let count = row.as_ref().column_count();
        rows.push(
            (0..count)
                .map(|i| data::duck_value(row.get::<_, DuckValue>(i).unwrap_or(DuckValue::Null)))
                .collect(),
        );
    }
    drop(cursor);
    let columns = stmt.column_names().iter().map(|c| c.to_string()).collect();
    Ok((columns, rows))
}

fn list(columns: &[String], alias: &str) -> String {
    columns
        .iter()
        .map(|c| format!("{alias}.{}", data::q(c)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn same_keys(keys: &[String]) -> String {
    keys.iter()
        .map(|k| format!("t.{0} IS NOT DISTINCT FROM b.{0}", data::q(k)))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn any_differs(columns: &[String], new: &str, old: &str) -> String {
    columns
        .iter()
        .map(|c| format!("{new}.{0} IS DISTINCT FROM {old}.{0}", data::q(c)))
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn columns_of(connection: &duckdb::Connection, table: &str) -> Result<Vec<String>, AppError> {
    Ok(select_rows(connection, &format!("SELECT * FROM {table} LIMIT 0"))?.0)
}

/// Runs the statements on the attachment in one transaction.
pub fn direct(
    connection: &duckdb::Connection,
    sql: &[String],
    values: &[DuckValue],
    job: &Job,
) -> Result<Outcome, AppError> {
    in_transaction(connection, !job.dry_run, || {
        let table = &job.table;
        if job.watch && matches!(job.kind, ActionKind::Insert | ActionKind::Update) {
            // Updates compare every column; inserts only need the keys that existed.
            let snapshot = match job.kind {
                ActionKind::Update => "*".to_string(),
                _ => list(&job.keys, "t"),
            };
            connection
                .execute_batch(&format!(
                    "CREATE OR REPLACE TEMP TABLE __ixtable_before AS SELECT {snapshot} FROM {table} t"
                ))
                .map_err(failed)?;
        }
        let counts = sql
            .iter()
            .map(|s| run_statement(connection, s, values))
            .collect::<Result<Vec<_>, _>>()?;
        let (removed, changed) = match counts.as_slice() {
            [removed, inserted] => (*removed, *inserted),
            [n] => (0, *n),
            _ => (0, 0),
        };
        let rows = match (job.watch, job.kind) {
            (false, _) | (_, ActionKind::Delete) => ChangedRows::default(),
            (true, ActionKind::Update) => updated_since_snapshot(connection, job)?,
            (true, ActionKind::Replace) => ChangedRows {
                created: select_rows(
                    connection,
                    &format!("SELECT {} FROM {table} t", list(&job.keys, "t")),
                )?
                .1,
                updated: vec![],
            },
            (true, ActionKind::Insert) => ChangedRows {
                created: select_rows(
                    connection,
                    &format!(
                        "SELECT {} FROM {table} t ANTI JOIN {BEFORE} b ON {}",
                        list(&job.keys, "t"),
                        same_keys(&job.keys)
                    ),
                )?
                .1,
                updated: vec![],
            },
        };
        Ok(Outcome {
            changed,
            removed,
            rows,
        })
    })
}

/// Rows with the same key and some different column, with their old values.
fn updated_since_snapshot(
    connection: &duckdb::Connection,
    job: &Job,
) -> Result<ChangedRows, AppError> {
    let columns = columns_of(connection, BEFORE)?;
    let sql = format!(
        "SELECT {}, {} FROM {} t JOIN {BEFORE} b ON {} WHERE {}",
        list(&job.keys, "t"),
        list(&columns, "b"),
        job.table,
        same_keys(&job.keys),
        any_differs(&columns, "t", "b")
    );
    let (_, rows) = select_rows(connection, &sql)?;
    let updated = rows
        .into_iter()
        .map(|row| {
            let (identity, old) = row.split_at(job.keys.len());
            UpdatedRow {
                identity: identity.to_vec(),
                old: named(&columns, old),
            }
        })
        .collect();
    Ok(ChangedRows {
        created: vec![],
        updated,
    })
}

fn named(columns: &[String], values: &[DataValue]) -> Vec<NamedValue> {
    columns
        .iter()
        .zip(values)
        .map(|(column, value)| NamedValue {
            column: column.clone(),
            value: value.clone(),
        })
        .collect()
}

/// Runs an embedded-SQLite UPDATE on a copy and returns the changed rows and
/// columns, plus the old values. The transaction always rolls back: the
/// attachment is never written.
pub fn compute_update(
    connection: &duckdb::Connection,
    sql: &str,
    values: &[DuckValue],
    job: &Job,
) -> Result<Computed, AppError> {
    if !job.rowid && job.keys.is_empty() {
        return Err(AppError::new(
            "VALIDATION_ERROR",
            format!(
                "The table \"{}\" has neither a rowid nor a primary key",
                job.name
            ),
        ));
    }
    in_transaction(connection, false, || {
        let table = &job.table;
        let (columns, types) = column_types(connection, table)?;
        let row_key = if job.rowid {
            format!("rowid AS {ROWID}, ")
        } else {
            String::new()
        };
        connection
            .execute_batch(&format!(
                "CREATE OR REPLACE TEMP TABLE __ixtable_work AS SELECT {row_key}* FROM {table}"
            ))
            .map_err(failed)?;
        let changed = run_statement(connection, sql, values)?;
        // The key that finds the row: its rowid, else its old primary key.
        let type_of = |c: &str| {
            columns
                .iter()
                .position(|x| x == c)
                .map(|i| types[i].clone())
                .unwrap_or_else(|| "VARCHAR".into())
        };
        let (key_columns, key, join) = if job.rowid {
            (
                vec![("rowid".to_string(), "BIGINT".to_string())],
                "b.rowid".to_string(),
                format!("t.{ROWID} = b.rowid"),
            )
        } else {
            (
                job.keys.iter().map(|k| (k.clone(), type_of(k))).collect(),
                list(&job.keys, "b"),
                same_keys(&job.keys),
            )
        };
        let width = key_columns.len();
        let diff = format!(
            "SELECT {key}, {}, {} FROM {WORK} t JOIN {table} b ON {join} WHERE {}",
            list(&columns, "t"),
            list(&columns, "b"),
            any_differs(&columns, "t", "b")
        );
        let (_, rows) = select_rows(connection, &diff)?;
        let mut touched = vec![false; columns.len()];
        let mut changes = vec![];
        let mut updated = vec![];
        for row in rows {
            let (key, rest) = row.split_at(width);
            let (new, old) = rest.split_at(columns.len());
            let values: Vec<Option<DataValue>> = new
                .iter()
                .zip(old)
                .enumerate()
                .map(|(i, (n, o))| {
                    (n != o).then(|| {
                        touched[i] = true;
                        n.clone()
                    })
                })
                .collect();
            changes.push((key.to_vec(), values));
            // Triggers read the row by its key after the update (a changed key included).
            let current = if job.keys.is_empty() {
                key.to_vec()
            } else {
                job.keys
                    .iter()
                    .filter_map(|k| columns.iter().position(|c| c == k).map(|i| new[i].clone()))
                    .collect()
            };
            updated.push(UpdatedRow {
                identity: current,
                old: named(&columns, old),
            });
        }
        // Keep only the columns some row changed.
        let keep: Vec<usize> = (0..columns.len()).filter(|&i| touched[i]).collect();
        let changes = Changes {
            key_columns,
            columns: keep
                .iter()
                .map(|&i| (columns[i].clone(), types[i].clone()))
                .collect(),
            rows: changes
                .into_iter()
                .map(|(k, v)| (k, keep.iter().map(|&i| v[i].clone()).collect()))
                .collect(),
        };
        let rows = if job.watch {
            ChangedRows {
                created: vec![],
                updated,
            }
        } else {
            ChangedRows::default()
        };
        Ok(Computed {
            outcome: Outcome {
                changed,
                removed: 0,
                rows,
            },
            changes,
        })
    })
}

fn column_types(
    connection: &duckdb::Connection,
    table: &str,
) -> Result<(Vec<String>, Vec<String>), AppError> {
    let (_, rows) = select_rows(connection, &format!("DESCRIBE {table}"))?;
    Ok(rows
        .into_iter()
        .filter_map(|r| match (r.first(), r.get(1)) {
            (Some(DataValue::Text(n)), Some(DataValue::Text(t))) => Some((n.clone(), t.clone())),
            _ => None,
        })
        .unzip())
}

fn is_date_type(duck: &str) -> bool {
    let upper = duck.to_ascii_uppercase();
    upper.starts_with("DATE") || upper.starts_with("TIMESTAMP")
}

/// Whether `write_back` must use the text attachment: some changed column is a
/// date or timestamp, which the typed attachment cannot UPDATE. Binary values
/// cannot go through text, so a change to both kinds is refused.
pub fn needs_text(changes: &Changes, table: &str) -> Result<bool, AppError> {
    let dates = changes.columns.iter().any(|(_, t)| is_date_type(t));
    let blobs = changes
        .columns
        .iter()
        .any(|(_, t)| t.eq_ignore_ascii_case("BLOB"));
    if dates && blobs {
        return Err(AppError::new(
            "VALIDATION_ERROR",
            format!("An update of \"{table}\" cannot change a date or time column and a binary column at once; split it into two queries"),
        ));
    }
    Ok(dates)
}

/// The text SQLite stores for a value of a column (`text` attachment): the
/// value in the column's canonical form, booleans as 0 and 1. SQLite's column
/// affinity turns numbers back into numbers.
fn storage_text(
    column: &str,
    logical: Option<&LogicalType>,
    value: &DataValue,
) -> Result<DuckValue, AppError> {
    let value = match logical {
        Some(l) => l
            .normalize(column, value)
            .map_err(|e| AppError::new("VALIDATION_ERROR", e))?,
        None => value.clone(),
    };
    Ok(match value {
        DataValue::Null => DuckValue::Null,
        DataValue::Boolean(b) => DuckValue::Text(if b { "1" } else { "0" }.into()),
        DataValue::Blob(_) => {
            return Err(AppError::new(
                "VALIDATION_ERROR",
                format!("{column} is binary"),
            ))
        }
        other => DuckValue::Text(other.as_text().unwrap_or_default()),
    })
}

/// Writes computed changes in one DuckDB transaction: the rows go into a temp
/// table, then one `UPDATE … FROM` sets each changed column of each row (and
/// leaves the others as they are). `connection` is a writer opened with
/// `text` as `needs_text` says; `logical` gives each column's logical type.
pub fn write_back(
    connection: &duckdb::Connection,
    job: &Job,
    changes: &Changes,
    logical: &[(String, LogicalType)],
    text: bool,
) -> Result<u64, AppError> {
    if changes.rows.is_empty() {
        return Ok(0);
    }
    in_transaction(connection, true, || {
        let typed = |duck: &str| if text { "VARCHAR" } else { duck }.to_string();
        let mut defs = vec![];
        for (i, (k, t)) in changes.key_columns.iter().enumerate() {
            let ty = if k == "rowid" {
                "BIGINT".to_string()
            } else {
                typed(t)
            };
            defs.push(format!("__k{i} {ty}"));
        }
        for (i, (_, t)) in changes.columns.iter().enumerate() {
            defs.push(format!("__c{i} {}", typed(t)));
            defs.push(format!("__s{i} BOOLEAN"));
        }
        connection
            .execute_batch(&format!(
                "CREATE OR REPLACE TEMP TABLE __ixtable_changes ({})",
                defs.join(", ")
            ))
            .map_err(failed)?;
        let slots = vec!["?"; defs.len()].join(", ");
        let mut insert = connection
            .prepare(&format!(
                "INSERT INTO temp.main.__ixtable_changes VALUES ({slots})"
            ))
            .map_err(failed)?;
        let logical_of = |c: &str| logical.iter().find(|(n, _)| n == c).map(|(_, l)| l);
        let bind = |c: &str, v: &DataValue| -> Result<DuckValue, AppError> {
            if text {
                storage_text(c, logical_of(c), v)
            } else {
                data::read::duck_bind(v).map_err(|e| AppError::new("VALIDATION_ERROR", e))
            }
        };
        for (key, values) in &changes.rows {
            let mut row = vec![];
            for ((k, _), v) in changes.key_columns.iter().zip(key) {
                row.push(if k == "rowid" {
                    data::read::duck_bind(v).map_err(|e| AppError::new("VALIDATION_ERROR", e))?
                } else {
                    bind(k, v)?
                });
            }
            for ((c, _), v) in changes.columns.iter().zip(values) {
                row.push(match v {
                    Some(v) => bind(c, v)?,
                    None => DuckValue::Null,
                });
                row.push(DuckValue::Boolean(v.is_some()));
            }
            insert
                .execute(duckdb::params_from_iter(row.iter()))
                .map_err(failed)?;
        }
        let sets: Vec<String> = changes
            .columns
            .iter()
            .enumerate()
            .map(|(i, (c, _))| {
                let c = data::q(c);
                format!("{c} = CASE WHEN w.__s{i} THEN w.__c{i} ELSE t.{c} END")
            })
            .collect();
        let found: Vec<String> = changes
            .key_columns
            .iter()
            .enumerate()
            .map(|(i, (k, _))| {
                if k == "rowid" {
                    format!("t.rowid = w.__k{i}")
                } else {
                    format!("t.{} = w.__k{i}", data::q(k))
                }
            })
            .collect();
        let sql = format!(
            "UPDATE {} AS t SET {} FROM temp.main.__ixtable_changes w WHERE {}",
            job.table,
            sets.join(", "),
            found.join(" AND ")
        );
        run_statement(connection, &sql, &[])
    })
}
