//! Action queries of an imported document, once its data is in place
//! (`docs/decisions/action-queries.md`): make-table targets the file does not
//! have become tables (migration 002, typed from DuckDB's DESCRIBE of the
//! query), and every action statement is bound once to report bad SQL.
use super::report::ImportReport;
use crate::archive::DocumentConfig;
use crate::data;
use crate::data::support::logical_from_duckdb;
use crate::manager::{AppError, DocumentManager};

/// `sql` with each `$param` replaced by a typed NULL, so DuckDB can bind it.
pub fn with_null_params(q: &crate::archive::SavedQuery) -> String {
    let mut sql = q.sql.clone();
    // Longest names first so `$a` does not replace part of `$ab`.
    let mut params: Vec<_> = q.parameters.iter().collect();
    params.sort_by_key(|p| std::cmp::Reverse(p.name.len()));
    for p in params {
        let ty = match p.logical_type.as_str() {
            "integer" => "BIGINT",
            "number" => "DOUBLE",
            "boolean" => "BOOLEAN",
            "date" => "DATE",
            "timestamp" => "TIMESTAMP",
            _ => "VARCHAR",
        };
        sql = sql.replace(&format!("${}", p.name), &format!("CAST(NULL AS {ty})"));
    }
    sql
}

/// Creates the make-table targets `(query id, table)` and records them in the config.
pub fn create_targets(
    m: &DocumentManager,
    window: &str,
    config: &mut DocumentConfig,
    targets: &[(String, String)],
    report: &mut ImportReport,
) -> Result<(), AppError> {
    let mut ddl = vec![];
    let mut created = vec![];
    for (id, table) in targets {
        let Some(q) = config.saved_queries.iter().find(|q| &q.id == id) else {
            continue;
        };
        let described = m.read_query(window, &format!("DESCRIBE {}", with_null_params(q)));
        let result = match described {
            Ok(r) => r,
            Err(e) => {
                report.note(
                    "query",
                    &q.name,
                    format!("the table {table} was not created: {}", e.message),
                );
                continue;
            }
        };
        let columns: Vec<String> = result
            .rows
            .iter()
            .filter_map(|row| match (row.first(), row.get(1)) {
                (Some(data::DataValue::Text(name)), Some(data::DataValue::Text(ty))) => {
                    Some(format!(
                        "{} {}",
                        data::q(name),
                        logical_from_duckdb(ty).sqlite_declared()
                    ))
                }
                _ => None,
            })
            .collect();
        ddl.push(format!(
            "CREATE TABLE {} (\n  {}\n);",
            data::q(table),
            columns.join(",\n  ")
        ));
        created.push(table.clone());
    }
    if created.is_empty() {
        return Ok(());
    }
    config.migrations.push(
        serde_json::from_value(serde_json::json!({
            "id": uuid::Uuid::now_v7().to_string(),
            "name": "002 Create make-table query targets",
            "order": 2,
            "targetStore": "sqlite",
            "up": ddl.join("\n"),
        }))
        .map_err(|e| AppError::new("INTERNAL", e.to_string()))?,
    );
    for table in created {
        config.entities.push(
            serde_json::from_value(serde_json::json!({
                "id": uuid::Uuid::now_v7().to_string(), "table": table, "concurrency": "optimistic"
            }))
            .map_err(|e| AppError::new("INTERNAL", e.to_string()))?,
        );
    }
    m.update_config(window, config.clone())?;
    crate::migrations::apply_sqlite(&m.database_path(window)?, &config.migrations)
        .map_err(|e| AppError::new("MIGRATION_FAILED", e))?;
    m.mark_data_dirty(window)?;
    Ok(())
}

/// Binds each insert, update and delete statement on a writer connection.
pub fn check_statements(
    m: &DocumentManager,
    window: &str,
    config: &DocumentConfig,
    report: &mut ImportReport,
) -> Result<(), AppError> {
    let queries: Vec<_> = config
        .saved_queries
        .iter()
        .filter(|q| {
            q.action
                .as_ref()
                .is_some_and(|a| a.kind != crate::archive::ActionKind::Replace)
        })
        .collect();
    if queries.is_empty() {
        return Ok(());
    }
    let db = m.database_path(window)?;
    let extension = m.with_session(window, |s| Ok(s.reader.sqlite_extension().to_path_buf()))?;
    let workspace = db.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let _gate = data::gate::exclusive(&db).map_err(|e| AppError::new("BUSY", e))?;
    let connection = data::write::open_writer(
        &workspace,
        &extension,
        &data::read::ReadTarget::Sqlite,
        false,
    )
    .map_err(|e| AppError::new("CONNECTION", e))?;
    for q in queries {
        let Some(spec) = &q.action else { continue };
        if let Err(e) = crate::queries::action::prepare_check(&connection, q, spec, "main") {
            report.note(
                "query",
                &q.name,
                format!("DuckDB rejects the converted SQL: {e}"),
            );
        }
    }
    Ok(())
}
