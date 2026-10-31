//! Loads Access rows into a scratch SQLite file, checks every planned
//! constraint against the data, and copies the rows into the new document.
//!
//! Access lets data predate a rule (a validation rule or a Required flag added
//! later, or relationships without referential integrity at the time), so a
//! constraint the rows break is dropped and reported instead of losing rows.
use super::schema::{Conv, TablePlan, TableRole};
use crate::access::blob;
use crate::access::model::{AccessFile, Value};
use crate::access::translate::sql::quote;
use rusqlite::types::Value as Sql;
use rusqlite::{params_from_iter, Connection};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct Staging {
    pub conn: Connection,
    pub path: PathBuf,
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Notes per Access object name, filled while checking.
pub type NoteMap = BTreeMap<String, Vec<String>>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// The URL part of an Access hyperlink (`text#address#subaddress#`).
pub fn hyperlink_address(v: &str) -> String {
    let parts: Vec<&str> = v.split('#').collect();
    match parts.as_slice() {
        [text, address, ..] if !address.is_empty() => {
            let _ = text;
            address.to_string()
        }
        [text, _, sub, ..] if !sub.is_empty() => format!("{text}#{sub}"),
        [text, ..] => text.to_string(),
        [] => String::new(),
    }
}

pub fn sql_value(v: &Value, hyperlink: bool, ole: bool) -> Sql {
    match v {
        Value::Null => Sql::Null,
        Value::Bool(b) => Sql::Integer(*b as i64),
        Value::Int(i) => Sql::Integer(*i),
        Value::Double(d) if d.is_finite() => Sql::Real(*d),
        // Kept as text so the type check empties it with a note instead of silently.
        Value::Double(d) => Sql::Text(format!("{d}")),
        Value::Decimal(d) => Sql::Text(d.clone()),
        Value::Text(t) if hyperlink => Sql::Text(hyperlink_address(t)),
        Value::Text(t) | Value::Guid(t) | Value::DateTime(t) => Sql::Text(t.clone()),
        Value::Binary(b) if ole => Sql::Blob(blob::ole_payload(b)),
        Value::Binary(b) => Sql::Blob(b.clone()),
        Value::Attachments(_) | Value::Multi(_) => Sql::Null,
    }
}

/// Creates the staging tables (untyped columns, no constraints).
fn create(conn: &Connection, plans: &[TablePlan]) -> Result<(), String> {
    for t in plans {
        // Child tables number their rows; other columns stay untyped.
        let cols: Vec<String> = t
            .columns
            .iter()
            .map(|c| {
                if t.role != TableRole::Main && c.name == "ID" {
                    format!("{} INTEGER PRIMARY KEY", quote(&c.name))
                } else {
                    quote(&c.name)
                }
            })
            .collect();
        conn.execute_batch(&format!(
            "CREATE TABLE {} ({});",
            quote(&t.name),
            cols.join(", ")
        ))
        .map_err(err)?;
    }
    Ok(())
}

/// Streams every table of the Access file into the staging database.
pub fn load(
    file: &mut dyn AccessFile,
    plans: &[TablePlan],
    dir: &Path,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(&str, u64),
) -> Result<(Staging, BTreeMap<String, u64>), String> {
    let path = dir.join(format!("ixtable-access-{}.db", uuid::Uuid::new_v4()));
    let conn = Connection::open(&path).map_err(err)?;
    conn.execute_batch("PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;")
        .map_err(err)?;
    let staging = Staging { conn, path };
    create(&staging.conn, plans)?;
    let mut counts = BTreeMap::new();
    for t in plans.iter().filter(|t| t.role == TableRole::Main) {
        let table = file
            .db()
            .table(&t.access)
            .cloned()
            .ok_or_else(|| format!("missing table {}", t.access))?;
        let children: Vec<(usize, &TablePlan)> = plans
            .iter()
            .filter_map(|c| match &c.role {
                TableRole::Attachments(p, col) | TableRole::Values(p, col)
                    if p.eq_ignore_ascii_case(&t.access) =>
                {
                    table
                        .columns
                        .iter()
                        .position(|x| x.name.eq_ignore_ascii_case(col))
                        .map(|i| (i, c))
                }
                _ => None,
            })
            .collect();
        let placeholders = vec!["?"; t.columns.len()].join(", ");
        let insert = format!(
            "INSERT INTO {} ({}) VALUES ({placeholders})",
            quote(&t.name),
            t.columns
                .iter()
                .map(|c| quote(&c.name))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let key_index = t.single_key().and_then(|k| {
            t.columns
                .iter()
                .position(|c| c.name.eq_ignore_ascii_case(k))
        });
        let tx = staging.conn.unchecked_transaction().map_err(err)?;
        let mut count = 0u64;
        {
            let mut stmt = tx.prepare(&insert).map_err(err)?;
            let mut child_stmts = vec![];
            for (_, c) in &children {
                let sql = format!(
                    "INSERT INTO {} ({}) VALUES ({})",
                    quote(&c.name),
                    c.columns[1..]
                        .iter()
                        .map(|x| quote(&x.name))
                        .collect::<Vec<_>>()
                        .join(", "),
                    vec!["?"; c.columns.len() - 1].join(", ")
                );
                child_stmts.push(tx.prepare(&sql).map_err(err)?);
            }
            file.rows(&t.access, &mut |values| {
                if count % 256 == 0 && cancelled() {
                    return Err(super::CANCELLED.into());
                }
                let row: Vec<Sql> = t
                    .columns
                    .iter()
                    .map(|c| {
                        let src = &table.columns[c.source.unwrap_or(0)];
                        c.source
                            .and_then(|i| values.get(i))
                            .map(|v| {
                                sql_value(
                                    v,
                                    src.hyperlink,
                                    src.ty == crate::access::model::ColType::Ole,
                                )
                            })
                            .unwrap_or(Sql::Null)
                    })
                    .collect();
                let parent = match key_index {
                    Some(k) => row[k].clone(),
                    None => Sql::Null,
                };
                stmt.execute(params_from_iter(row.iter())).map_err(err)?;
                let parent = if parent == Sql::Null {
                    Sql::Integer(tx.last_insert_rowid())
                } else {
                    parent
                };
                for ((i, _), cs) in children.iter().zip(child_stmts.iter_mut()) {
                    match values.get(*i) {
                        Some(Value::Attachments(files)) => {
                            for f in files {
                                cs.execute(rusqlite::params![
                                    parent,
                                    f.file_name,
                                    f.file_type,
                                    f.data
                                ])
                                .map_err(err)?;
                            }
                        }
                        Some(Value::Multi(items)) => {
                            for v in items.iter().filter(|v| **v != Value::Null) {
                                cs.execute(rusqlite::params![parent, sql_value(v, false, false)])
                                    .map_err(err)?;
                            }
                        }
                        _ => {}
                    }
                }
                count += 1;
                if count % 1000 == 0 {
                    progress(&t.access, count);
                }
                Ok(())
            })?;
        }
        tx.commit().map_err(err)?;
        progress(&t.access, count);
        if cancelled() {
            return Err(super::CANCELLED.into());
        }
        counts.insert(t.name.clone(), count);
    }
    Ok((staging, counts))
}

/// SQL that is true for a stored value the declared type cannot hold. DuckDB reads a
/// SQLite column by its declared type and fails the whole table on one such value.
fn misfit_sql(col: &str, declared: &str) -> Option<String> {
    let hex = |n: usize| "[0-9a-fA-F]".repeat(n);
    let kind = declared.split('(').next().unwrap_or(declared);
    match kind {
        // Staging columns are untyped, so numbers can arrive as text (template sample data).
        "INTEGER" | "BOOLEAN" => Some(format!(
            "typeof({col}) = 'blob' OR (typeof({col}) = 'real' AND {col} <> CAST({col} AS INTEGER)) \
             OR (typeof({col}) = 'text' AND (trim({col}) GLOB '*[^0-9+-]*' OR trim({col}) NOT GLOB '*[0-9]*'))"
        )),
        "DECIMAL" | "REAL" => Some(format!(
            "typeof({col}) = 'blob' OR (typeof({col}) = 'text' AND (trim({col}) GLOB '*[^0-9eE.+-]*' OR trim({col}) NOT GLOB '*[0-9]*'))"
        )),
        "TIMESTAMP" | "DATE" => Some(format!(
            "typeof({col}) <> 'text' OR datetime({col}) IS NULL OR substr({col}, 5, 1) <> '-'"
        )),
        "UUID" => Some(format!(
            "typeof({col}) <> 'text' OR {col} NOT GLOB '{}-{}-{}-{}-{}'",
            hex(8), hex(4), hex(4), hex(4), hex(12)
        )),
        _ => None,
    }
}

/// Empties the values of a column that do not fit its type; returns a note naming a
/// few of them (`{column}` stands for the column name).
fn empty_misfits(conn: &Connection, table: &str, col: &str, declared: &str) -> Option<String> {
    let bad = misfit_sql(col, declared)?;
    let filter = format!("{col} IS NOT NULL AND ({bad})");
    let n = count(conn, &format!("SELECT count(*) FROM {table} WHERE {filter}")).ok()?;
    if n == 0 {
        return None;
    }
    let examples: Vec<String> = conn
        .prepare(&format!("SELECT DISTINCT CAST({col} AS TEXT) FROM {table} WHERE {filter} LIMIT 3"))
        .and_then(|mut st| {
            st.query_map([], |r| r.get::<_, Option<String>>(0))
                .map(|rows| rows.flatten().flatten().map(|v| format!("\"{v}\"")).collect())
        })
        .unwrap_or_default();
    conn.execute_batch(&format!("UPDATE {table} SET {col} = NULL WHERE {filter}")).ok()?;
    let values = if n == 1 { "value does not fit" } else { "values do not fit" };
    Some(format!(
        "{{column}}: {n} {values} the column type and {} left empty ({})",
        if n == 1 { "was" } else { "were" },
        examples.join(", ")
    ))
}

fn count(conn: &Connection, sql: &str) -> Result<i64, String> {
    conn.query_row(sql, [], |r| r.get::<_, i64>(0)).map_err(err)
}

/// Drops the constraints the staged rows break and settles date types.
pub fn check(staging: &Staging, plans: &mut [TablePlan], notes: &mut NoteMap) {
    let conn = &staging.conn;
    for t in plans.iter_mut() {
        let owner = t.access.clone();
        let mut note = |n: String| notes.entry(owner.clone()).or_default().push(n);
        let table = quote(&t.name);
        for c in t.columns.iter_mut() {
            let col = quote(&c.name);
            if let Some(n) = empty_misfits(conn, &table, &col, &c.declared) {
                note(n.replace("{column}", &c.name));
            }
            if c.declared.starts_with("DECIMAL") {
                let digits = format!("length(ltrim(replace(replace(replace(CAST({col} AS TEXT), '-', ''), '+', ''), '.', ''), '0'))");
                if let Ok(n @ 1..) = count(conn, &format!("SELECT count(*) FROM {table} WHERE typeof({col}) = 'text' AND {digits} > 15")) {
                    let (values, has) = if n == 1 { ("value", "has") } else { ("values", "have") };
                    note(format!(
                        "{}: {n} {values} {has} more than 15 significant digits and {} stored rounded",
                        c.name,
                        if n == 1 { "is" } else { "are" }
                    ));
                }
            }
            match c.date_candidate {
                Conv::DateOnly => {
                    let timed = count(conn, &format!("SELECT count(*) FROM {table} WHERE {col} IS NOT NULL AND substr({col}, 11) NOT IN ('', 'T00:00:00')"));
                    if timed == Ok(0) {
                        c.conv = Conv::DateOnly;
                    }
                }
                Conv::TimeOnly => {
                    let dated = count(conn, &format!("SELECT count(*) FROM {table} WHERE {col} IS NOT NULL AND substr({col}, 1, 10) <> '1899-12-30'"));
                    if dated == Ok(0) {
                        c.conv = Conv::TimeOnly;
                    }
                }
                Conv::AsIs => {}
            }
            if c.not_null
                && count(
                    conn,
                    &format!("SELECT count(*) FROM {table} WHERE {col} IS NULL"),
                ) != Ok(0)
            {
                c.not_null = false;
                note(format!(
                    "{}: Required is not enforced because existing rows leave it empty",
                    c.name
                ));
            }
            if let Some((rule, _)) = &c.check {
                match count(
                    conn,
                    &format!("SELECT count(*) FROM {table} WHERE NOT ({rule})"),
                ) {
                    Ok(0) => {}
                    Ok(n) => {
                        note(format!("{}: the validation rule is checked in forms only because {n} existing rows break it", c.name));
                        c.check = None;
                    }
                    Err(e) => {
                        note(format!(
                            "{}: the validation rule is checked in forms only ({e})",
                            c.name
                        ));
                        c.check = None;
                    }
                }
            }
            if let Some(d) = &c.default_sql {
                if let Err(e) = count(conn, &format!("SELECT count(*) FROM (SELECT {d})")) {
                    note(format!("{}: default value dropped ({e})", c.name));
                    c.default_sql = None;
                }
            }
            if let Some(calc) = &c.calc_sql {
                if let Err(e) = count(
                    conn,
                    &format!("SELECT count(*) FROM (SELECT {calc} FROM {table} LIMIT 1)"),
                ) {
                    note(format!(
                        "{}: calculated column keeps its stored values ({e})",
                        c.name
                    ));
                    c.calc_sql = None;
                }
            }
        }
        let mut kept = vec![];
        for (rule, message) in std::mem::take(&mut t.checks) {
            match count(conn, &format!("SELECT count(*) FROM {table} WHERE NOT ({rule})")) {
                Ok(0) => kept.push((rule, message)),
                Ok(n) => note(format!("the table validation rule is checked in forms only because {n} existing rows break it")),
                Err(e) => note(format!("the table validation rule is checked in forms only ({e})")),
            }
        }
        t.checks = kept;
        if !t.primary_key.is_empty() {
            let cols = t
                .primary_key
                .iter()
                .map(|k| super::schema::key_expr(t, "", k))
                .collect::<Vec<_>>()
                .join(", ");
            let not_null = t
                .primary_key
                .iter()
                .map(|k| format!("{} IS NOT NULL", quote(k)))
                .collect::<Vec<_>>()
                .join(" AND ");
            let dupes = count(conn, &format!("SELECT count(*) FROM (SELECT 1 FROM {table} WHERE {not_null} GROUP BY {cols} HAVING count(*) > 1)"));
            let nulls = count(
                conn,
                &format!("SELECT count(*) FROM {table} WHERE NOT ({not_null})"),
            );
            let integer_key = t
                .single_key()
                .and_then(|k| t.column(k))
                .is_some_and(|c| c.declared == "INTEGER");
            if dupes != Ok(0) || (nulls != Ok(0) && !integer_key) {
                note("the primary key has duplicate or empty values, so the table has no primary key".into());
                t.primary_key.clear();
            }
        }
        let collated: Vec<Vec<String>> = t
            .indexes
            .iter()
            .map(|i| i.columns.iter().map(|k| super::schema::key_expr(t, "", k)).collect())
            .collect();
        for (i, collated) in t.indexes.iter_mut().zip(collated).filter(|(i, _)| i.unique) {
            let cols = collated.join(", ");
            let not_null = i
                .columns
                .iter()
                .map(|k| format!("{} IS NOT NULL", quote(k)))
                .collect::<Vec<_>>()
                .join(" AND ");
            if count(conn, &format!("SELECT count(*) FROM (SELECT 1 FROM {table} WHERE {not_null} GROUP BY {cols} HAVING count(*) > 1)")) != Ok(0) {
                i.unique = false;
                note(format!("index {}: existing rows repeat values, so it is not unique", i.name));
            }
        }
    }
    check_foreign_keys(conn, plans, notes);
}

/// Keeps a foreign key when its target columns are still a key and every row finds its parent.
fn check_foreign_keys(conn: &Connection, plans: &mut [TablePlan], notes: &mut NoteMap) {
    let keys: Vec<(String, Vec<Vec<String>>)> = plans
        .iter()
        .map(|p| {
            let mut k: Vec<Vec<String>> = p
                .indexes
                .iter()
                .filter(|i| i.unique)
                .map(|i| i.columns.clone())
                .collect();
            if !p.primary_key.is_empty() {
                k.push(p.primary_key.clone());
            }
            (p.name.clone(), k)
        })
        .collect();
    let nocase: std::collections::BTreeSet<(String, String)> = plans
        .iter()
        .flat_map(|t| t.columns.iter().filter(|c| c.nocase).map(|c| (t.name.clone(), c.name.to_lowercase())))
        .collect();
    for t in plans.iter_mut() {
        let owner = t.access.clone();
        let mut note = |n: String| notes.entry(owner.clone()).or_default().push(n);
        let table = quote(&t.name);
        let mut fks = vec![];
        for fk in std::mem::take(&mut t.foreign_keys) {
            let target_keyed = keys.iter().any(|(n, k)| {
                n == &fk.target_table
                    && k.iter().any(|cols| {
                        cols.len() == fk.target_columns.len()
                            && cols.iter().all(|c| {
                                fk.target_columns.iter().any(|x| x.eq_ignore_ascii_case(c))
                            })
                    })
            });
            let join = fk
                .columns
                .iter()
                .zip(&fk.target_columns)
                .map(|(c, p)| {
                    let collate = if nocase.contains(&(fk.target_table.clone(), p.to_lowercase())) {
                        " COLLATE NOCASE"
                    } else {
                        ""
                    };
                    format!("p.{} = c.{}{collate}", quote(p), quote(c))
                })
                .collect::<Vec<_>>()
                .join(" AND ");
            let present = fk
                .columns
                .iter()
                .map(|c| format!("c.{} IS NOT NULL", quote(c)))
                .collect::<Vec<_>>()
                .join(" AND ");
            let sql = format!(
                "SELECT count(*) FROM {table} c WHERE {present} AND NOT EXISTS (SELECT 1 FROM {} p WHERE {join})",
                quote(&fk.target_table)
            );
            match (target_keyed, count(conn, &sql)) {
                (false, _) => note(format!(
                    "relationship {}: no foreign key, because {} has no key on the related columns",
                    fk.relationship, fk.target_table
                )),
                (true, Ok(0)) => fks.push(fk),
                (true, Ok(n)) => note(format!(
                    "relationship {}: no foreign key, because {n} rows refer to missing {} rows",
                    fk.relationship, fk.target_table
                )),
                (true, Err(e)) => note(format!(
                    "relationship {}: no foreign key ({e})",
                    fk.relationship
                )),
            }
        }
        t.foreign_keys = fks;
    }
}

/// Converts a staged column to its final form in the copy.
fn select_expr(c: &super::schema::ColumnPlan) -> String {
    let col = quote(&c.name);
    match c.conv {
        Conv::DateOnly => format!("substr({col}, 1, 10)"),
        Conv::TimeOnly => format!("substr({col}, 12)"),
        Conv::AsIs => col,
    }
}

/// Copies the staged rows into the document database (tables already created).
pub fn copy_into(staging: &Staging, db: &Path, plans: &[TablePlan]) -> Result<(), String> {
    let conn = Connection::open(db).map_err(err)?;
    conn.execute_batch("PRAGMA foreign_keys=OFF;")
        .map_err(err)?;
    conn.execute(
        "ATTACH DATABASE ?1 AS staging",
        [staging.path.to_string_lossy().to_string()],
    )
    .map_err(err)?;
    let tx = conn.unchecked_transaction().map_err(err)?;
    for t in plans {
        let cols: Vec<String> = t.columns.iter().map(|c| quote(&c.name)).collect();
        let exprs: Vec<String> = t.columns.iter().map(select_expr).collect();
        tx.execute_batch(&format!(
            "INSERT INTO main.{} ({}) SELECT {} FROM staging.{} ORDER BY rowid;",
            quote(&t.name),
            cols.join(", "),
            exprs.join(", "),
            quote(&t.name)
        ))
        .map_err(|e| format!("copying {}: {e}", t.name))?;
    }
    tx.commit().map_err(err)?;
    conn.execute_batch("DETACH DATABASE staging;")
        .map_err(err)?;
    let broken: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .map_err(err)?;
    if broken > 0 {
        return Err(format!("{broken} rows break a foreign key after the copy"));
    }
    Ok(())
}
