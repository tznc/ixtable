//! Before-change triggers on action queries (`docs/decisions/action-queries.md`).
//!
//! The triggers run in TypeScript, so a run on a table with before-change
//! triggers takes two calls. The first computes the rows the statement would
//! create or update, rolls back, and returns them with a fingerprint. The
//! caller runs the triggers on each row: any rejection ends there, before
//! anything is written. The second call runs the statement again, checks
//! that it changes the same rows to the same values (else CONFLICT, rolled
//! back), and applies the fields the triggers set in the same transaction.
//!
//! Overrides are applied by key: PostgreSQL rows and the embedded-SQLite
//! update copy with an UPDATE, rows an embedded-SQLite insert created by
//! deleting and inserting them again (the attachment cannot UPDATE a date
//! column, see the record).
use super::action::{BeforeChange, Pending, PendingRow, WORK};
use super::action_exec::{
    any_differs, column_types, list, named, run_statement, same_keys, select_rows, Job, BEFORE,
    ROWID,
};
use crate::archive::ActionKind;
use crate::data::{self, DataValue, NamedValue};
use crate::manager::AppError;
use duckdb::types::Value as DuckValue;
use sha2::{Digest, Sha256};

fn invalid(message: String) -> AppError {
    AppError::new("VALIDATION_ERROR", message)
}

/// Pending rows of a direct run, in key order: every row an insert added (by
/// the key snapshot), every row a replace wrote, or every row an update changed
/// with its old values (by the full snapshot).
pub fn pending_direct(connection: &duckdb::Connection, job: &Job) -> Result<Pending, AppError> {
    let table = &job.table;
    let order = list(&job.keys, "t");
    let rows = match job.kind {
        ActionKind::Update => {
            let columns = column_types(connection, BEFORE)?.0;
            let sql = format!(
                "SELECT {}, {} FROM {table} t JOIN {BEFORE} b ON {} WHERE {} ORDER BY {order}",
                list(&columns, "t"),
                list(&columns, "b"),
                same_keys(&job.keys),
                any_differs(&columns, "t", "b")
            );
            select_rows(connection, &sql)?
                .1
                .into_iter()
                .map(|row| {
                    let (new, old) = row.split_at(columns.len());
                    pending_row(job, &columns, new, Some(old))
                })
                .collect()
        }
        _ => {
            let columns = column_types(connection, table)?.0;
            let from = match job.kind {
                ActionKind::Insert => {
                    format!("{table} t ANTI JOIN {BEFORE} b ON {}", same_keys(&job.keys))
                }
                _ => format!("{table} t"),
            };
            let sql = format!(
                "SELECT {} FROM {from} ORDER BY {order}",
                list(&columns, "t")
            );
            select_rows(connection, &sql)?
                .1
                .into_iter()
                .map(|row| pending_row(job, &columns, &row, None))
                .collect()
        }
    };
    Ok(seal(job, rows))
}

/// Pending rows of an embedded-SQLite UPDATE, from the copy it ran on. A row
/// is found by its new key, or its rowid when the table has no key.
pub fn pending_copy(
    connection: &duckdb::Connection,
    job: &Job,
    columns: &[String],
) -> Result<Pending, AppError> {
    let (key, join) = if job.keys.is_empty() {
        (format!("t.{ROWID}"), format!("t.{ROWID} = b.rowid"))
    } else {
        (list(&job.keys, "t"), same_keys(&job.keys))
    };
    let sql = format!(
        "SELECT {key}, {}, {} FROM {WORK} t JOIN {} b ON {join} WHERE {} ORDER BY {key}",
        list(columns, "t"),
        list(columns, "b"),
        job.table,
        any_differs(columns, "t", "b")
    );
    let width = job.keys.len().max(1);
    let rows = select_rows(connection, &sql)?
        .1
        .into_iter()
        .map(|row| {
            let (identity, rest) = row.split_at(width);
            let (new, old) = rest.split_at(columns.len());
            PendingRow {
                identity: identity.to_vec(),
                values: named(columns, new),
                old: Some(named(columns, old)),
            }
        })
        .collect();
    Ok(seal(job, rows))
}

fn pending_row(
    job: &Job,
    columns: &[String],
    new: &[DataValue],
    old: Option<&[DataValue]>,
) -> PendingRow {
    let identity = job
        .keys
        .iter()
        .filter_map(|k| columns.iter().position(|c| c == k).map(|i| new[i].clone()))
        .collect();
    PendingRow {
        identity,
        values: named(columns, new),
        old: old.map(|o| named(columns, o)),
    }
}

/// The rows with the fingerprint the second call must reproduce. Defaulted
/// columns of new rows (sequences, timestamps) and their identities are left
/// out: they can differ between the calls without the triggers' input changing.
fn seal(job: &Job, rows: Vec<PendingRow>) -> Pending {
    let stable: Vec<(&[NamedValue], Vec<&NamedValue>)> = rows
        .iter()
        .map(|r| match &r.old {
            Some(old) => (old.as_slice(), r.values.iter().collect()),
            None => (
                &[][..],
                r.values
                    .iter()
                    .filter(|v| !job.defaulted.contains(&v.column))
                    .collect(),
            ),
        })
        .collect();
    let json = serde_json::to_string(&stable).unwrap_or_default();
    Pending {
        fingerprint: format!("{:x}", Sha256::digest(json.as_bytes())),
        rows,
    }
}

/// Refuses the run when the statement no longer changes the rows the triggers saw.
pub fn verify(before: &BeforeChange, pending: &Pending) -> Result<(), AppError> {
    if before.fingerprint == pending.fingerprint {
        return Ok(());
    }
    Err(AppError::new(
        "CONFLICT",
        "The rows this action query changes changed while its before-change triggers ran; run it again",
    ))
}

/// One override, checked: the pending row it sets and its normalized values.
struct Change<'a> {
    row: &'a PendingRow,
    values: Vec<NamedValue>,
}

fn changes<'a>(
    job: &Job,
    before: &BeforeChange,
    pending: &'a Pending,
) -> Result<Vec<Change<'a>>, AppError> {
    let mut out = vec![];
    for o in before.overrides.iter().filter(|o| !o.values.is_empty()) {
        let row = pending.rows.get(o.row).ok_or_else(|| {
            invalid(
                "A before-change trigger set fields of a row this action query does not change"
                    .into(),
            )
        })?;
        let mut values = vec![];
        for v in &o.values {
            if job.keys.contains(&v.column) {
                return Err(invalid(format!(
                    "A before-change trigger cannot change the key column {} of a row an action query writes",
                    v.column
                )));
            }
            if !row.values.iter().any(|c| c.column == v.column) {
                return Err(invalid(format!("{} has no column {}", job.name, v.column)));
            }
            let value = match job.logical.iter().find(|(n, _)| *n == v.column) {
                Some((_, l)) => l.normalize(&v.column, &v.value).map_err(invalid)?,
                None => v.value.clone(),
            };
            values.push(NamedValue {
                column: v.column.clone(),
                value,
            });
        }
        // Several overrides of one row merge, later fields winning.
        match out
            .iter_mut()
            .find(|c: &&mut Change| c.row.identity == row.identity)
        {
            Some(existing) => {
                existing
                    .values
                    .retain(|v| !values.iter().any(|n| n.column == v.column));
                existing.values.extend(values);
            }
            None => out.push(Change { row, values }),
        }
    }
    Ok(out)
}

/// The key columns of a pending row with their values.
fn key_values(job: &Job, row: &PendingRow) -> Vec<NamedValue> {
    job.keys
        .iter()
        .zip(&row.identity)
        .map(|(k, v)| NamedValue {
            column: k.clone(),
            value: v.clone(),
        })
        .collect()
}

fn bind(value: &DataValue) -> Result<DuckValue, AppError> {
    data::read::duck_bind(value).map_err(invalid)
}

/// `"c" = CAST(? AS type)` for each value, and the bound values.
fn casts(
    values: &[NamedValue],
    types: &(Vec<String>, Vec<String>),
    separator: &str,
) -> Result<(String, Vec<DuckValue>), AppError> {
    let type_of = |c: &str| {
        types
            .0
            .iter()
            .position(|n| n == c)
            .map(|i| types.1[i].clone())
            .unwrap_or_else(|| "VARCHAR".into())
    };
    let sql = values
        .iter()
        .map(|v| format!("{} = CAST(? AS {})", data::q(&v.column), type_of(&v.column)))
        .collect::<Vec<_>>()
        .join(separator);
    let bound = values
        .iter()
        .map(|v| bind(&v.value))
        .collect::<Result<_, _>>()?;
    Ok((sql, bound))
}

/// Applies the triggers' fields to the rows a direct run changed, in its transaction.
pub fn apply_direct(
    connection: &duckdb::Connection,
    job: &Job,
    before: &BeforeChange,
    pending: &Pending,
) -> Result<(), AppError> {
    let types = column_types(connection, &job.table)?;
    for change in changes(job, before, pending)? {
        let (find, found) = casts(&key_values(job, change.row), &types, " AND ")?;
        if job.sqlite {
            // The attachment cannot UPDATE a date column: write the row again instead.
            run_statement(
                connection,
                &format!("DELETE FROM {} WHERE {find}", job.table),
                &found,
            )?;
            let row: Vec<NamedValue> = change
                .row
                .values
                .iter()
                .filter(|v| !job.generated.contains(&v.column))
                .map(|v| {
                    change
                        .values
                        .iter()
                        .find(|o| o.column == v.column)
                        .unwrap_or(v)
                        .clone()
                })
                .collect();
            let (_, bound) = casts(&row, &types, ", ")?;
            let columns: Vec<String> = row.iter().map(|v| data::q(&v.column)).collect();
            let slots: Vec<String> = row
                .iter()
                .map(|v| {
                    let i = types.0.iter().position(|n| *n == v.column);
                    let ty = i.map(|i| types.1[i].as_str()).unwrap_or("VARCHAR");
                    format!("CAST(? AS {ty})")
                })
                .collect();
            run_statement(
                connection,
                &format!(
                    "INSERT INTO {} ({}) VALUES ({})",
                    job.table,
                    columns.join(", "),
                    slots.join(", ")
                ),
                &bound,
            )?;
        } else {
            let (set, mut bound) = casts(&change.values, &types, ", ")?;
            bound.extend(found);
            run_statement(
                connection,
                &format!("UPDATE {} SET {set} WHERE {find}", job.table),
                &bound,
            )?;
        }
    }
    Ok(())
}

/// Applies the triggers' fields to the copy an embedded-SQLite UPDATE ran on,
/// before it is compared with the live table.
pub fn apply_copy(
    connection: &duckdb::Connection,
    job: &Job,
    before: &BeforeChange,
    pending: &Pending,
) -> Result<(), AppError> {
    let types = column_types(connection, WORK)?;
    for change in changes(job, before, pending)? {
        let (set, mut bound) = casts(&change.values, &types, ", ")?;
        let find = if job.keys.is_empty() {
            bound.push(bind(&change.row.identity[0])?);
            format!("{ROWID} = ?")
        } else {
            let (find, found) = casts(&key_values(job, change.row), &types, " AND ")?;
            bound.extend(found);
            find
        };
        run_statement(
            connection,
            &format!("UPDATE {WORK} SET {set} WHERE {find}"),
            &bound,
        )?;
    }
    Ok(())
}
