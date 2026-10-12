//! Files of attachment fields (`docs/decisions/field-formats.md`). The field's column
//! holds a JSON array of `AttachmentRef`; the bytes live in the record store's hidden
//! `_ixtable_attachments` table, written through the RecordStore and read through DuckDB,
//! so they travel with embedded SQLite data and are shared on PostgreSQL.
use crate::{
    authz::Op,
    data::{DataValue, ReadRuntime},
    manager::AppError,
    recordstore::{with_store, RecordStore, StoreError},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub const TABLE: &str = "_ixtable_attachments";
/// Largest file one attachment can hold (decimal megabytes).
pub const MAX_BYTES: usize = 20_000_000;
/// Files younger than this are kept by cleanup: an open form may not have saved them yet.
const CLEANUP_GRACE: chrono::Duration = chrono::Duration::hours(1);

const SQLITE_DDL: &str = "CREATE TABLE IF NOT EXISTS _ixtable_attachments(id TEXT PRIMARY KEY, table_name TEXT NOT NULL, column_name TEXT NOT NULL, name TEXT NOT NULL, mime TEXT NOT NULL, size INTEGER NOT NULL, sha256 TEXT NOT NULL, created_at TEXT NOT NULL, content BLOB NOT NULL)";
const POSTGRES_DDL: &str = "CREATE TABLE IF NOT EXISTS _ixtable_attachments(id TEXT PRIMARY KEY, table_name TEXT NOT NULL, column_name TEXT NOT NULL, name TEXT NOT NULL, mime TEXT NOT NULL, size BIGINT NOT NULL, sha256 TEXT NOT NULL, created_at TEXT NOT NULL, content BYTEA NOT NULL)";
// Binds are text on both stores, so the content travels as hex.
const SQLITE_INSERT: &str =
    "INSERT INTO _ixtable_attachments VALUES (?, ?, ?, ?, ?, CAST(? AS INTEGER), ?, ?, unhex(?))";
const POSTGRES_INSERT: &str = "INSERT INTO _ixtable_attachments VALUES (?, ?, ?, ?, ?, CAST(? AS TEXT)::bigint, ?, ?, decode(CAST(? AS TEXT), 'hex'))";

/// One file of an attachment field, as stored in the field's JSON array.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentRef {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentContent {
    #[serde(flatten)]
    pub file: AttachmentRef,
    pub content_base64: String,
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The ids an attachment column value refers to; anything but a JSON array of files is none.
pub fn referenced_ids(value: &str) -> Vec<String> {
    serde_json::from_str::<Vec<serde_json::Value>>(value)
        .unwrap_or_default()
        .iter()
        .filter_map(|file| file.get("id")?.as_str().map(str::to_string))
        .collect()
}

/// Writes one file through the record store, creating the hidden table on first use.
pub fn store_file(
    store: &mut dyn RecordStore,
    table: &str,
    column: &str,
    name: &str,
    mime: &str,
    bytes: &[u8],
) -> Result<AttachmentRef, StoreError> {
    let postgres = store.kind() == "postgres";
    store.execute_internal(if postgres { POSTGRES_DDL } else { SQLITE_DDL }, &[])?;
    let file = AttachmentRef {
        id: uuid::Uuid::now_v7().to_string(),
        name: crate::assets::safe_file_name(name),
        mime: mime.to_string(),
        size: bytes.len() as u64,
        sha256: sha256_hex(bytes),
    };
    let binds = [
        file.id.clone(),
        table.to_string(),
        column.to_string(),
        file.name.clone(),
        file.mime.clone(),
        file.size.to_string(),
        file.sha256.clone(),
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        hex(bytes),
    ];
    let insert = if postgres {
        POSTGRES_INSERT
    } else {
        SQLITE_INSERT
    };
    store.execute_internal(insert, &binds)?;
    Ok(file)
}

fn text(value: &DataValue) -> String {
    match value {
        DataValue::Text(s) | DataValue::Timestamp(s) => s.clone(),
        DataValue::Integer(i) => i.to_string(),
        _ => String::new(),
    }
}

fn table_exists(reader: &ReadRuntime) -> bool {
    reader.row_count(TABLE).is_ok()
}

/// Reads one file through DuckDB and checks its checksum. Returns its table and column too.
pub fn read_file(
    reader: &ReadRuntime,
    id: &str,
) -> Result<Option<(String, String, AttachmentContent)>, String> {
    let id = uuid::Uuid::parse_str(id).map_err(|_| "Not an attachment id".to_string())?;
    if !table_exists(reader) {
        return Ok(None);
    }
    let sql = format!(
        "SELECT table_name, column_name, name, mime, size, sha256, content FROM {} WHERE id = '{id}'",
        reader.qualified(TABLE)
    );
    let Some(row) = reader.query(&sql)?.rows.into_iter().next() else {
        return Ok(None);
    };
    let DataValue::Blob(content_base64) = &row[6] else {
        return Err("The attachment has no content".into());
    };
    let bytes = STANDARD
        .decode(content_base64)
        .map_err(|e| format!("The attachment content is not readable: {e}"))?;
    let file = AttachmentRef {
        id: id.to_string(),
        name: text(&row[2]),
        mime: text(&row[3]),
        size: bytes.len() as u64,
        sha256: text(&row[5]),
    };
    if sha256_hex(&bytes) != file.sha256 {
        return Err(format!("{} failed its checksum check", file.name));
    }
    Ok(Some((
        text(&row[0]),
        text(&row[1]),
        AttachmentContent {
            file,
            content_base64: content_base64.clone(),
        },
    )))
}

/// Stored files no record refers to that are older than the cleanup grace period.
pub fn unused_files(
    reader: &ReadRuntime,
    fields: &[(String, String)],
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<String>, String> {
    if !table_exists(reader) {
        return Ok(vec![]);
    }
    let mut used = HashSet::new();
    for (table, column) in fields {
        let sql = format!(
            "SELECT CAST({} AS VARCHAR) FROM {} WHERE {} IS NOT NULL",
            crate::data::q(column),
            reader.qualified(table),
            crate::data::q(column)
        );
        // A table or column that no longer exists refers to nothing.
        let Ok(result) = reader.query(&sql) else {
            continue;
        };
        for row in result.rows {
            used.extend(referenced_ids(&text(&row[0])));
        }
    }
    let cutoff = (now - CLEANUP_GRACE).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let sql = format!(
        "SELECT id FROM {} WHERE created_at < '{cutoff}' ORDER BY id",
        reader.qualified(TABLE)
    );
    Ok(reader
        .query(&sql)?
        .rows
        .iter()
        .map(|row| text(&row[0]))
        .filter(|id| !used.contains(id))
        .collect())
}

fn attachment_field(window: &str, table: &str, column: &str) -> Result<bool, AppError> {
    let config = crate::manager()?.config(window)?;
    Ok(config.entities.iter().any(|e| {
        e.table == table
            && e.fields
                .iter()
                .any(|f| f.column == column && f.format.as_deref() == Some("attachment"))
    }))
}

/// Stores a file for an attachment field and returns the entry the form adds to the
/// field's JSON array. Needs permission to create or update records of the table.
#[tauri::command]
pub fn upload_record_attachment(
    window_label: String,
    table: String,
    column: String,
    name: String,
    mime: Option<String>,
    content_base64: String,
) -> Result<AttachmentRef, AppError> {
    let window = window_label.as_str();
    let may_create = crate::authz::check(window, "table", &table, Op::Create);
    if may_create.is_err() {
        crate::authz::check(window, "table", &table, Op::Update)?;
    }
    if !attachment_field(window, &table, &column)? {
        return Err(AppError::new(
            "VALIDATION",
            format!("{table}.{column} is not an attachment field"),
        ));
    }
    let bytes = STANDARD
        .decode(content_base64.as_bytes())
        .map_err(|e| AppError::new("VALIDATION", format!("The file is not valid base64: {e}")))?;
    if bytes.len() > MAX_BYTES {
        return Err(AppError::new(
            "TOO_LARGE",
            format!(
                "{name} is {} MB; attachments can be at most {} MB",
                bytes.len().div_ceil(1_000_000),
                MAX_BYTES / 1_000_000
            ),
        ));
    }
    let mime = mime
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| crate::assets::guess_media_type(&name).to_string());
    let file = with_store(window, |s| {
        store_file(s, &table, &column, &name, &mime, &bytes)
    })?;
    crate::recordstore::after_write(window)?;
    Ok(file)
}

/// Reads one attachment's content, for a role that can read the record's table.
#[tauri::command]
pub fn read_record_attachment(
    window_label: String,
    id: String,
) -> Result<AttachmentContent, AppError> {
    let m = crate::manager()?;
    let found = m.with_session(&window_label, |s| {
        read_file(&s.reader, &id).map_err(|e| AppError::new("IO_ERROR", e))
    })?;
    let Some((table, _, content)) = found else {
        return Err(AppError::new(
            "NOT_FOUND",
            "The attachment no longer exists",
        ));
    };
    crate::authz::check(&window_label, "table", &table, Op::Read)?;
    Ok(content)
}

/// Writes one attachment to a path the user chose in a save dialog.
#[tauri::command]
pub fn save_record_attachment(
    window_label: String,
    id: String,
    path: String,
) -> Result<(), AppError> {
    let content = read_record_attachment(window_label, id)?;
    let bytes = STANDARD
        .decode(content.content_base64.as_bytes())
        .map_err(|e| AppError::new("IO_ERROR", e.to_string()))?;
    std::fs::write(&path, bytes).map_err(|e| AppError::new("IO_ERROR", e))
}

/// Deletes stored files no record refers to any more (Studio only). Returns how many.
#[tauri::command]
pub fn remove_unused_record_attachments(window_label: String) -> Result<usize, AppError> {
    let window = window_label.as_str();
    crate::authz::require_unrestricted(window, "remove unused attachments")?;
    let m = crate::manager()?;
    let config = m.config(window)?;
    let fields: Vec<(String, String)> = config
        .entities
        .iter()
        .flat_map(|e| {
            e.fields
                .iter()
                .filter(|f| f.format.as_deref() == Some("attachment"))
                .map(|f| (e.table.clone(), f.column.clone()))
        })
        .collect();
    let unused = m.with_session(window, |s| {
        unused_files(&s.reader, &fields, chrono::Utc::now())
            .map_err(|e| AppError::new("IO_ERROR", e))
    })?;
    if unused.is_empty() {
        return Ok(0);
    }
    let count = unused.len();
    with_store(window, |s| {
        for id in &unused {
            s.execute_internal(
                "DELETE FROM _ixtable_attachments WHERE id = ?",
                std::slice::from_ref(id),
            )?;
        }
        Ok(())
    })?;
    crate::recordstore::after_write(window)?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    include!("record_attachments_tests.rs");
}
