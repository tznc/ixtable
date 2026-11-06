//! Values that do not fit their column's declared type. DuckDB reads a SQLite column by
//! its declared type and fails the whole table on one such value, so they are emptied.
use rusqlite::Connection;

fn count(conn: &Connection, sql: &str) -> Result<i64, String> {
    conn.query_row(sql, [], |r| r.get::<_, i64>(0)).map_err(|e| e.to_string())
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
pub fn empty_misfits(conn: &Connection, table: &str, col: &str, declared: &str) -> Option<String> {
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

