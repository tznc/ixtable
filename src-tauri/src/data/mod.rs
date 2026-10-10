//! Shared record-data types and the DuckDB read layer (PRD §10).
//!
//! Reads go through DuckDB (`ReadRuntime`). Writes go to the RecordStore
//! (`crate::recordstore`) and never through DuckDB. Identifiers are
//! discovered from the store and quoted; values are always bound parameters.
pub mod ddl;
#[cfg(test)]
mod ddl_tests;
pub mod extensions;
#[cfg(test)]
mod extensions_tests;
pub mod files;
#[cfg(test)]
mod files_tests;
pub mod gate;
pub mod logical;
#[cfg(test)]
mod logical_tests;
pub mod model;
pub mod page;
#[cfg(test)]
mod race_tests;
pub mod read;
pub mod sqltext;
pub mod support;
#[cfg(test)]
mod tests;
pub mod totals;
#[cfg(test)]
mod totals_tests;
pub mod values;
pub mod write;

pub use ddl::{
    parse_sqlite_create_index, parse_sqlite_create_table, CheckDef, ColumnDef, ForeignKeyDef,
    IndexDef, TableDef, UniqueDef,
};
pub use extensions::{postgres_extension_path, sqlite_extension_path};
pub use logical::{
    LogicalType, LOGICAL_TYPE_NAMES, MAX_DECIMAL_PRECISION, SQLITE_MAX_DECIMAL_PRECISION,
};
pub use read::{ReadRuntime, ReadTarget};
pub use support::{logical_from_duckdb, redact};

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};

/// A value crossing the command boundary. Decimal, date, time, and timestamp
/// values travel as canonical strings (see `logical`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum DataValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(String),
    Boolean(bool),
    Date(String),
    Timestamp(String),
    Decimal(String),
    Time(String),
}

/// Converts a DuckDB value losslessly: decimals keep their scale, temporal
/// values become canonical ISO strings, nested values become JSON text.
pub fn duck_value(v: duckdb::types::Value) -> DataValue {
    use duckdb::types::Value as V;
    let micros_ts = |unit: duckdb::types::TimeUnit, v: i64| {
        chrono::DateTime::from_timestamp_micros(unit.to_micros(v)).map(|d| d.naive_utc())
    };
    match v {
        V::Null => DataValue::Null,
        V::Boolean(v) => DataValue::Boolean(v),
        V::TinyInt(v) => DataValue::Integer(v as i64),
        V::SmallInt(v) => DataValue::Integer(v as i64),
        V::Int(v) => DataValue::Integer(v as i64),
        V::BigInt(v) => DataValue::Integer(v),
        V::UTinyInt(v) => DataValue::Integer(v as i64),
        V::USmallInt(v) => DataValue::Integer(v as i64),
        V::UInt(v) => DataValue::Integer(v as i64),
        V::UBigInt(v) => i64::try_from(v)
            .map(DataValue::Integer)
            .unwrap_or_else(|_| DataValue::Decimal(v.to_string())),
        V::HugeInt(v) => i64::try_from(v)
            .map(DataValue::Integer)
            .unwrap_or_else(|_| DataValue::Decimal(v.to_string())),
        V::UHugeInt(v) => i64::try_from(v)
            .map(DataValue::Integer)
            .unwrap_or_else(|_| DataValue::Decimal(v.to_string())),
        V::Float(v) => DataValue::Real(v as f64),
        V::Double(v) => DataValue::Real(v),
        V::Decimal(d) => DataValue::Decimal(d.to_string()),
        V::Text(v) => DataValue::Text(v),
        V::Enum(v) => DataValue::Text(v),
        V::Blob(v) | V::Geometry(v) => DataValue::Blob(STANDARD.encode(v)),
        V::Date32(days) => chrono::NaiveDate::from_num_days_from_ce_opt(days + 719_163)
            .map(|d| DataValue::Date(d.format("%Y-%m-%d").to_string()))
            .unwrap_or(DataValue::Integer(days as i64)),
        V::Time64(unit, v) => {
            let micros = unit.to_micros(v);
            chrono::NaiveTime::from_num_seconds_from_midnight_opt(
                (micros.div_euclid(1_000_000)) as u32,
                (micros.rem_euclid(1_000_000) * 1000) as u32,
            )
            .map(|t| DataValue::Time(logical::format_time(t)))
            .unwrap_or(DataValue::Integer(micros))
        }
        V::Timestamp(unit, v) => micros_ts(unit, v)
            .map(|t| DataValue::Timestamp(logical::format_timestamp(t)))
            .unwrap_or(DataValue::Integer(v)),
        V::Interval {
            months,
            days,
            nanos,
        } => DataValue::Text(format!(
            "{months} months {days} days {}s",
            nanos as f64 / 1e9
        )),
        other => DataValue::Text(nested_json(other).to_string()),
    }
}
fn nested_json(v: duckdb::types::Value) -> serde_json::Value {
    use duckdb::types::Value as V;
    match v {
        V::List(items) | V::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(nested_json).collect())
        }
        V::Struct(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), nested_json(v.clone())))
                .collect(),
        ),
        V::Union(inner) => nested_json(*inner),
        V::Map(map) => serde_json::Value::Array(
            map.iter()
                .map(|(k, v)| serde_json::json!([nested_json(k.clone()), nested_json(v.clone())]))
                .collect(),
        ),
        other => match duck_value(other) {
            DataValue::Null => serde_json::Value::Null,
            DataValue::Integer(i) => i.into(),
            DataValue::Real(f) => serde_json::json!(f),
            DataValue::Boolean(b) => b.into(),
            v => v.as_text().unwrap_or_default().into(),
        },
    }
}

pub fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// ILIKE pattern for a contains/starts-with search: `\`, `%` and `_` in the
/// typed text match literally (pair it with `ESCAPE '\'`).
pub fn like_pattern(text: &str, contains: bool) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    if contains {
        format!("%{escaped}%")
    } else {
        format!("{escaped}%")
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbObject {
    pub name: String,
    pub object_type: String,
    pub row_count: Option<u64>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub name: String,
    pub declared_type: String,
    pub logical_type: LogicalType,
    pub nullable: bool,
    pub default_value: Option<String>,
    pub primary_key_position: u32,
    pub generated: bool,
    pub unique: bool,
    /// The database fills the key in on insert: a PostgreSQL identity or `nextval` (serial) default.
    pub auto_increment: bool,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKey {
    pub id: i64,
    pub name: Option<String>,
    pub from_columns: Vec<String>,
    pub target_table: String,
    pub target_columns: Vec<String>,
    pub on_update: String,
    pub on_delete: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableSchema {
    pub name: String,
    pub object_type: String,
    pub columns: Vec<Column>,
    pub foreign_keys: Vec<ForeignKey>,
    pub without_rowid: bool,
    pub primary_key: Vec<String>,
    pub uniques: Vec<UniqueDef>,
    pub checks: Vec<CheckDef>,
    pub indexes: Vec<IndexDef>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<DataValue>>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<DataValue>>,
    pub identities: Vec<Vec<DataValue>>,
    pub total: u64,
    pub offset: u64,
    pub limit: u64,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sort {
    pub column: String,
    pub descending: bool,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOperator {
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
    /// Matches any of `values` (an empty list matches nothing).
    In,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub column: String,
    pub operator: FilterOperator,
    pub value: Option<DataValue>,
    /// The candidates of an `in` filter.
    #[serde(default)]
    pub values: Option<Vec<DataValue>>,
}
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NamedValue {
    pub column: String,
    pub value: DataValue,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowUpdate {
    pub identity: Vec<DataValue>,
    pub values: Vec<NamedValue>,
}
/// A column in a create/alter request. `logicalType` is preferred;
/// `declaredType` alone (SQLite affinity names) keeps working.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreateColumn {
    pub name: String,
    #[serde(default)]
    pub declared_type: String,
    #[serde(default)]
    pub logical_type: Option<LogicalType>,
    #[serde(default = "yes")]
    pub nullable: bool,
    #[serde(default)]
    pub primary_key_position: u32,
    #[serde(default)]
    pub unique: bool,
    #[serde(default)]
    pub default_expression: Option<String>,
    #[serde(default)]
    pub generated_expression: Option<String>,
    #[serde(default)]
    pub check: Option<String>,
}
fn yes() -> bool {
    true
}
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreateForeignKey {
    #[serde(default)]
    pub name: Option<String>,
    pub columns: Vec<String>,
    pub target_table: String,
    pub target_columns: Vec<String>,
    #[serde(default)]
    pub on_update: Option<String>,
    #[serde(default)]
    pub on_delete: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndexSpec {
    pub name: String,
    pub columns: Vec<String>,
    #[serde(default)]
    pub unique: bool,
}
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreateTable {
    pub name: String,
    pub columns: Vec<CreateColumn>,
    #[serde(default)]
    pub foreign_keys: Vec<CreateForeignKey>,
    #[serde(default)]
    pub checks: Vec<String>,
    #[serde(default)]
    pub without_rowid: bool,
    /// Multi-column unique constraints.
    #[serde(default)]
    pub uniques: Vec<Vec<String>>,
    #[serde(default)]
    pub indexes: Vec<IndexSpec>,
}
/// One typed schema change. The store decides whether it runs in place or
/// needs a table rebuild (see `recordstore::ChangePlan`).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(
    tag = "operation",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AlterTable {
    RenameTable {
        new_name: String,
    },
    RenameColumn {
        column: String,
        new_name: String,
    },
    AddColumn {
        column: CreateColumn,
    },
    DropColumn {
        column: String,
    },
    AlterColumn {
        column: String,
        definition: CreateColumn,
    },
    SetPrimaryKey {
        columns: Vec<String>,
    },
    AddForeignKey {
        foreign_key: CreateForeignKey,
    },
    DropForeignKey {
        columns: Vec<String>,
    },
    AddUnique {
        columns: Vec<String>,
        #[serde(default)]
        name: Option<String>,
    },
    DropUnique {
        columns: Vec<String>,
    },
    AddCheck {
        expression: String,
        #[serde(default)]
        name: Option<String>,
    },
    DropCheck {
        expression: String,
    },
}

/// The read-only guard used by `ReadRuntime::query` (and saved queries): exactly one
/// SELECT/WITH/VALUES/SHOW/DESCRIBE statement with no mutating or file-access keyword.
/// Strings, quoted identifiers and comments are ignored, and one trailing `;` is
/// allowed. Returns the statement without that `;`. Defense in depth: the reader
/// also runs with DuckDB external access disabled (see `ReadRuntime`).
pub fn read_only_guard(sql: &str) -> Result<&str, String> {
    let trimmed = sql.trim();
    let masked = sqltext::mask(trimmed);
    let code = masked.trim_end();
    let code = code.strip_suffix(';').unwrap_or(code);
    if code.trim().is_empty() || code.contains(';') {
        return Err("Exactly one query statement is required".into());
    }
    let statement = trimmed[..code.len()].trim_end();
    let upper = code.to_ascii_uppercase();
    let words: Vec<&str> = upper
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
        .collect();
    let first = words.first().copied().unwrap_or("");
    const FORBIDDEN: [&str; 44] = [
        "INSERT",
        "UPDATE",
        "DELETE",
        "CREATE",
        "DROP",
        "ALTER",
        "ATTACH",
        "DETACH",
        "INSTALL",
        "LOAD",
        "COPY",
        "EXPORT",
        "IMPORT",
        "CALL",
        "PRAGMA",
        "SET",
        "RESET",
        "READ_CSV",
        "READ_CSV_AUTO",
        "READ_JSON",
        "READ_JSON_AUTO",
        "READ_JSON_OBJECTS",
        "READ_NDJSON",
        "READ_NDJSON_AUTO",
        "READ_PARQUET",
        "PARQUET_SCAN",
        "PARQUET_METADATA",
        "PARQUET_SCHEMA",
        "SQLITE_SCAN",
        "SQLITE_ATTACH",
        "SQLITE_QUERY",
        "READ_BLOB",
        "READ_TEXT",
        "READ_XLSX",
        "SNIFF_CSV",
        "GLOB",
        "HTTPFS",
        "POSTGRES_QUERY",
        "POSTGRES_EXECUTE",
        "POSTGRES_SCAN",
        "POSTGRES_ATTACH",
        "DUCKDB_DATABASES",
        "DUCKDB_SECRETS",
        "WHICH_SECRET",
    ];
    // Any other file reader called as a function: read_*(...) or *_scan(...).
    let file_function = |word: &str| {
        (word.starts_with("READ_") || word.ends_with("_SCAN"))
            && upper
                .match_indices(word)
                .any(|(i, _)| upper[i + word.len()..].trim_start().starts_with('('))
    };
    if !matches!(first, "SELECT" | "WITH" | "VALUES" | "SHOW" | "DESCRIBE")
        || words
            .iter()
            .any(|w| FORBIDDEN.contains(w) || file_function(w))
    {
        return Err("Only one read-only query is allowed".into());
    }
    Ok(statement)
}
