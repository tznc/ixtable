//! Migration commands: status, preview, dry run, apply pending (after a
//! mandatory checkpoint), rollback of the last reversible migration, history.
//! They act on the embedded SQLite store only; a document whose datasource is
//! PostgreSQL gets a `VALIDATION_ERROR` from dry run, apply, and rollback.
use super::{
    applied, history, pending_in, preflight, run_one, validate, Migration, MigrationLog,
    POSTGRES_DOCUMENT, POSTGRES_UNSUPPORTED,
};
use crate::archive::DocumentConfig;
use crate::checkpoints::CheckpointInfo;
use crate::manager::AppError;
use crate::recordstore::{with_store, RecordStore, StoreError};
use crate::validation::Severity;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationStatus {
    pub id: String,
    pub name: String,
    pub order: u32,
    pub applied: bool,
    pub applies_to_store: bool,
    /// The up SQL changed after the migration was applied.
    pub modified: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationRun {
    pub ok: bool,
    pub checkpoint: Option<CheckpointInfo>,
    pub logs: Vec<MigrationLog>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPreview {
    pub id: String,
    pub direction: String,
    pub sql: String,
    pub statements: Vec<String>,
    pub warnings: Vec<String>,
    pub store: String,
    pub transactional: bool,
}

fn config(window: &str) -> Result<DocumentConfig, AppError> {
    crate::manager()?.config(window)
}
/// Migrations run on the embedded SQLite store only (PostgreSQL is out of MVP scope).
pub(crate) fn ensure_sqlite(config: &DocumentConfig) -> Result<(), AppError> {
    if config.datasource.is_postgres() {
        return Err(AppError::new("VALIDATION_ERROR", POSTGRES_DOCUMENT));
    }
    Ok(())
}
fn blocking_issues(config: &DocumentConfig) -> Result<(), AppError> {
    let errors: Vec<String> = validate(config)
        .into_iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| i.message)
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(AppError::new("VALIDATION_ERROR", errors.join("; ")))
    }
}
fn find<'a>(config: &'a DocumentConfig, id: &str) -> Result<&'a Migration, AppError> {
    config
        .migrations
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| AppError::new("NOT_FOUND", format!("Migration {id} not found")))
}

pub fn migration_status(window_label: String) -> Result<Vec<MigrationStatus>, AppError> {
    let config = config(&window_label)?;
    // A PostgreSQL document never runs migrations, so its store is not touched.
    let done = if config.datasource.is_postgres() {
        vec![]
    } else {
        with_store(&window_label, |s| applied(s))?
    };
    let mut out: Vec<MigrationStatus> = config
        .migrations
        .iter()
        .map(|m| {
            let hit = done.iter().find(|d| d.0 == m.id);
            MigrationStatus {
                id: m.id.clone(),
                name: m.name.clone(),
                order: m.order,
                applied: hit.is_some(),
                applies_to_store: !config.datasource.is_postgres() && m.targets("sqlite"),
                modified: hit
                    .and_then(|h| h.1.as_ref())
                    .is_some_and(|c| c != &m.checksum()),
            }
        })
        .collect();
    out.sort_by_key(|s| s.order);
    Ok(out)
}

pub fn migration_history(window_label: String) -> Result<Vec<MigrationLog>, AppError> {
    if config(&window_label)?.datasource.is_postgres() {
        return Ok(vec![]);
    }
    with_store(&window_label, |s| history(s))
}

/// The SQL that would run, split into statements, with execution warnings.
pub fn preview_migration(
    window_label: String,
    id: String,
    direction: Option<String>,
) -> Result<MigrationPreview, AppError> {
    let config = config(&window_label)?;
    let m = find(&config, &id)?;
    let down = direction.as_deref() == Some("down");
    let sql = if down {
        m.down.clone().unwrap_or_default()
    } else {
        m.up.clone()
    };
    let statements = super::split::statements(&sql);
    let upper = sql.to_ascii_uppercase();
    let mut warnings = vec![];
    for (word, why) in [
        (
            "BEGIN",
            "explicit transactions are not allowed; ixtable wraps the migration in one transaction",
        ),
        (
            "COMMIT",
            "explicit transactions are not allowed; ixtable wraps the migration in one transaction",
        ),
        ("CONCURRENTLY", "cannot run inside a transaction"),
        ("VACUUM", "cannot run inside a transaction"),
        ("PRAGMA FOREIGN_KEYS", "has no effect inside a transaction"),
        ("DROP TABLE", "destructive: drops a table and its records"),
        ("DROP COLUMN", "destructive: drops a column and its data"),
    ] {
        if upper.contains(word) {
            warnings.push(format!("{word}: {why}"));
        }
    }
    if config.datasource.is_postgres() {
        warnings.push(POSTGRES_DOCUMENT.into());
    } else if m.target_store == "postgres" {
        warnings.push(POSTGRES_UNSUPPORTED.into());
    }
    if down && !m.reversible {
        warnings.push("This migration is not marked reversible".into());
    }
    Ok(MigrationPreview {
        id,
        direction: if down { "down" } else { "up" }.into(),
        sql,
        statements,
        warnings,
        store: "sqlite".into(),
        // SQLite runs DDL inside the migration's transaction.
        transactional: true,
    })
}

/// Validates migrations without keeping changes by running them on a copy of the
/// SQLite database. `ids` defaults to every pending migration.
pub fn dry_run_migrations(
    window_label: String,
    ids: Option<Vec<String>>,
) -> Result<MigrationLog, AppError> {
    crate::installation::ensure_studio(&window_label)?;
    let config = config(&window_label)?;
    ensure_sqlite(&config)?;
    blocking_issues(&config)?;
    let selected: Vec<Migration> = match ids {
        Some(ids) => {
            let mut v = ids
                .iter()
                .map(|id| find(&config, id).cloned())
                .collect::<Result<Vec<_>, _>>()?;
            v.sort_by_key(|m| m.order);
            v
        }
        None => with_store(&window_label, |s| pending_in(s, &config.migrations))?,
    };
    // Same checks as Apply, so a dry run never passes what Apply would refuse.
    let done = with_store(&window_label, |s| applied(s))?;
    let to_run: Vec<Migration> = selected
        .iter()
        .filter(|m| !done.iter().any(|d| d.0 == m.id))
        .cloned()
        .collect();
    preflight(&done, &config.migrations, &to_run)
        .map_err(|e| AppError::new("VALIDATION_ERROR", e))?;
    let names: Vec<String> = selected.iter().map(|m| m.name.clone()).collect();
    let script = selected
        .iter()
        .map(|m| m.up.trim().trim_end_matches(';').to_string() + ";")
        .collect::<Vec<_>>()
        .join("\n");
    let started_at = chrono::Utc::now().to_rfc3339();
    let mut log = MigrationLog {
        migration_id: selected
            .iter()
            .map(|m| m.id.clone())
            .collect::<Vec<_>>()
            .join(","),
        name: names.join(", "),
        direction: "up".into(),
        started_at,
        ..Default::default()
    };
    if let Some(message) = crate::data::sqltext::transaction_control_error(&script) {
        log.status = "dry_run_failed".into();
        log.error = Some(message);
        log.finished_at = chrono::Utc::now().to_rfc3339();
        return Ok(log);
    }
    if selected.is_empty() {
        log.status = "dry_run_ok".into();
        log.log.push("Nothing to run.".into());
        return Ok(log);
    }
    match with_store(&window_label, |s| Ok(s.run_script(&script, &[], true)))? {
        Ok(report) => {
            log.status = "dry_run_ok".into();
            log.log = report.log;
            log.health = report.health;
        }
        Err(StoreError { message, .. }) => {
            log.status = "dry_run_failed".into();
            log.error = Some(message);
        }
    }
    log.finished_at = chrono::Utc::now().to_rfc3339();
    Ok(log)
}

fn checkpoint(window: &str, reason: String) -> Result<CheckpointInfo, AppError> {
    crate::manager()?
        .create_checkpoint(window, &reason)
        .map_err(|e| {
            AppError::new(
                "CHECKPOINT_FAILED",
                format!(
                    "Migrations need a checkpoint first and it could not be created: {}",
                    e.message
                ),
            )
        })
}

/// Applies every pending migration in order after a mandatory checkpoint.
/// Stops at the first failure; the failed migration's transaction is rolled back.
pub fn apply_migrations(window_label: String) -> Result<MigrationRun, AppError> {
    crate::installation::ensure_studio(&window_label)?;
    let config = config(&window_label)?;
    ensure_sqlite(&config)?;
    blocking_issues(&config)?;
    let (done, pending) = with_store(&window_label, |s| {
        Ok((applied(s)?, pending_in(s, &config.migrations)?))
    })?;
    preflight(&done, &config.migrations, &pending)
        .map_err(|e| AppError::new("VALIDATION_ERROR", e))?;
    if pending.is_empty() {
        return Ok(MigrationRun {
            ok: true,
            checkpoint: None,
            logs: vec![],
        });
    }
    let names: Vec<&str> = pending.iter().map(|m| m.name.as_str()).collect();
    let cp = checkpoint(
        &window_label,
        format!("Before migrations: {}", names.join(", ")),
    )?;
    let mut applied_now: Vec<String> = done.into_iter().map(|d| d.0).collect();
    let logs = with_store(&window_label, |store| {
        Ok(run_pending(store, &pending, &mut applied_now, &cp.id))
    })?;
    crate::recordstore::after_write(&window_label)?;
    Ok(MigrationRun {
        ok: logs.iter().all(|l| l.status == "applied"),
        checkpoint: Some(cp),
        logs,
    })
}

fn run_pending(
    store: &mut dyn RecordStore,
    pending: &[Migration],
    applied_now: &mut Vec<String>,
    checkpoint: &str,
) -> Vec<MigrationLog> {
    let mut logs = vec![];
    for m in pending {
        if let Some(dep) = m.depends_on.iter().find(|d| !applied_now.contains(d)) {
            logs.push(MigrationLog {
                migration_id: m.id.clone(),
                name: m.name.clone(),
                direction: "up".into(),
                status: "blocked".into(),
                error: Some(format!("Dependency {dep} is not applied on this store")),
                ..Default::default()
            });
            break;
        }
        let log = run_one(store, m, false, Some(checkpoint));
        let failed = log.status == "failed";
        logs.push(log);
        if failed {
            break;
        }
        applied_now.push(m.id.clone());
    }
    logs
}

/// Runs the `down` SQL of the most recently applied migration (reversible only).
pub fn rollback_migration(window_label: String) -> Result<MigrationRun, AppError> {
    crate::installation::ensure_studio(&window_label)?;
    let config = config(&window_label)?;
    ensure_sqlite(&config)?;
    let done = with_store(&window_label, |s| applied(s))?;
    let last = done
        .last()
        .ok_or_else(|| AppError::new("NOT_FOUND", "No migration has been applied"))?;
    let m = find(&config, &last.0)?.clone();
    if !m.reversible || m.down.as_deref().is_none_or(|d| d.trim().is_empty()) {
        return Err(AppError::new(
            "VALIDATION_ERROR",
            format!(
                "{} is not reversible; restore a checkpoint or write a new migration instead",
                m.name
            ),
        ));
    }
    let cp = checkpoint(&window_label, format!("Before rolling back {}", m.name))?;
    let log = with_store(&window_label, |s| Ok(run_one(s, &m, true, Some(&cp.id))))?;
    crate::recordstore::after_write(&window_label)?;
    Ok(MigrationRun {
        ok: log.status == "rolled_back",
        checkpoint: Some(cp),
        logs: vec![log],
    })
}
