//! Crash recovery of extracted WIP (PRD §7.2, §27.1).
//!
//! Each open session works in `recovery/<sessionId>/` and is registered in the global
//! `recovery_sessions` table with a `dirty` flag. A session that ended without closing
//! (crash, kill) leaves both behind. On the next start, dirty leftovers are offered for
//! recovery; clean leftovers hold nothing new and are removed. Recovering validates the
//! WIP, reopens it, checkpoints the current `.ixt` locally, and saves the WIP back into
//! the `.ixt`. Invalid WIP never touches the archive: the error is surfaced instead.
//! When the `.ixt` is unreadable or cannot be checkpointed first, the work opens
//! with `RECOVERY_NEEDS_SAVE_AS` and the file is left untouched. Live sessions hold
//! an OS lock on `<workspace>.lock`, so another ixtable process never lists,
//! cleans up, or discards their workspaces.
use crate::{
    archive::{ArchiveDocument, ArchiveError, ArchiveMetadata, Attachment, DocumentConfig},
    archive_io::{self, HashingReader},
    logging,
    manager::{AppError, DocumentManager, SessionState},
    storage::RecoveryRecord,
};
use chrono::Utc;
use rusqlite::{Connection, OpenFlags};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{self, BufReader},
    path::{Path, PathBuf},
};

/// `lastError` code of recovered work that must not be saved over its file (the
/// file is unreadable or could not be checkpointed first); the UI asks for Save As.
pub const RECOVERY_NEEDS_SAVE_AS: &str = "RECOVERY_NEEDS_SAVE_AS";

fn storage_error(e: impl ToString) -> AppError {
    AppError::new("IO_ERROR", e)
}

/// Loads the workspace config from `document.json`, falling back to `config.yaml`
/// when `document.json` is missing or unreadable (both are written as a pair, so
/// either one holds the latest or the previous config).
pub fn workspace_config(work: &Path) -> Result<DocumentConfig, String> {
    let from_json = fs::read(work.join("document.json"))
        .map_err(|e| format!("document.json is missing: {e}"))
        .and_then(|json| {
            serde_json::from_slice::<DocumentConfig>(&json)
                .map_err(|e| format!("document.json is invalid: {e}"))
        })
        .and_then(|c| c.upgrade().map_err(|e| e.to_string()))
        .and_then(|c| c.design.validate().map(|_| c));
    let json_error = match from_json {
        Ok(config) => return Ok(config),
        Err(e) => e,
    };
    let yaml = fs::read_to_string(work.join("config.yaml"))
        .map_err(|e| format!("{json_error}; config.yaml is missing: {e}"))?;
    let config = crate::archive::document_config_from_yaml(&yaml)
        .map_err(|e| format!("{json_error}; config.yaml is invalid: {e}"))?;
    logging::warn(
        "recovery",
        &format!("{json_error}; recovered the config from config.yaml"),
    );
    Ok(config)
}

/// Checks that a leftover workspace can be reopened: `data.db` passes SQLite's
/// integrity check, the config loads (see [`workspace_config`]), and every asset matches its checksum.
pub fn validate_workspace(work: &Path, record: &RecoveryRecord) -> Result<ArchiveDocument, String> {
    let db = work.join("data.db");
    if !db.is_file() {
        return Err("data.db is missing".into());
    }
    // Read-write (never create): a crash mid-transaction leaves a hot rollback
    // journal, and only a writable connection may roll it back. Read-only opens
    // fail with SQLITE_READONLY_ROLLBACK and would reject recoverable work.
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let integrity: String = Connection::open_with_flags(&db, flags)
        .and_then(|c| c.query_row("PRAGMA integrity_check", [], |r| r.get(0)))
        .map_err(|e| format!("data.db is not a valid database: {e}"))?;
    if integrity != "ok" {
        return Err(format!("data.db failed its integrity check: {integrity}"));
    }
    let config = workspace_config(work)?;
    let metadata = match fs::read(work.join("archive.json")) {
        Ok(bytes) => serde_json::from_slice::<ArchiveMetadata>(&bytes)
            .map_err(|e| format!("archive.json is invalid: {e}"))?,
        Err(_) => {
            let now = Utc::now().to_rfc3339();
            ArchiveMetadata {
                document_id: record.document_id.clone(),
                created_at: now.clone(),
                updated_at: now,
                application_version: env!("CARGO_PKG_VERSION").into(),
            }
        }
    };
    if metadata.document_id != record.document_id {
        return Err("the workspace belongs to a different document".into());
    }
    if !crate::paths::is_safe_id(&metadata.document_id) {
        return Err(format!("unsafe document id {:?}", metadata.document_id));
    }
    let mut attachments = vec![];
    if let Ok(entries) = fs::read_dir(work.join("attachments")) {
        for entry in entries.filter_map(Result::ok) {
            let dir = entry.path();
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            if hidden || !dir.is_dir() {
                continue;
            }
            let meta: Attachment = fs::read(dir.join("metadata.json"))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .ok_or_else(|| format!("asset {} has no valid metadata", dir.display()))?;
            // The id names the asset directory: it must be a plain id and match it.
            if !crate::paths::is_safe_id(&meta.id) || entry.file_name().to_str() != Some(&meta.id) {
                return Err(format!(
                    "asset {} has an invalid id {:?}",
                    dir.display(),
                    meta.id
                ));
            }
            let mut reader = HashingReader::new(BufReader::new(
                File::open(dir.join("content"))
                    .map_err(|e| format!("asset {} is missing: {e}", meta.display_name))?,
            ));
            io::copy(&mut reader, &mut io::sink()).map_err(|e| e.to_string())?;
            if reader.bytes != meta.size || reader.hex() != meta.checksum {
                return Err(format!(
                    "asset {} failed checksum validation",
                    meta.display_name
                ));
            }
            attachments.push(meta);
        }
    }
    attachments.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    Ok(ArchiveDocument {
        metadata,
        data: vec![],
        config,
        attachments,
    })
}

/// OS advisory lock on `<workspace>.lock`. A live session holds it for its whole
/// lifetime; the OS releases it when the process exits, even after a crash. Dropping
/// it unlocks and removes the lock file.
#[derive(Debug)]
pub struct WorkspaceLock {
    file: Option<File>,
    path: PathBuf,
}
impl Drop for WorkspaceLock {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}

/// `<workspace>.lock`, next to (not inside) the workspace so deleting the workspace never races the lock.
pub fn lock_path(workspace: &Path) -> PathBuf {
    let mut name = workspace.as_os_str().to_owned();
    name.push(".lock");
    PathBuf::from(name)
}

/// Locks a workspace for this session. `Ok(None)` when a live session (in this or
/// another ixtable process) already holds it.
pub fn try_lock_workspace(workspace: &Path) -> io::Result<Option<WorkspaceLock>> {
    let path = lock_path(workspace);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(WorkspaceLock {
            file: Some(file),
            path,
        })),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(e)) => Err(e),
    }
}

/// True while a live session holds the workspace's lock.
pub fn workspace_in_use(workspace: &Path) -> bool {
    matches!(try_lock_workspace(workspace), Ok(None))
}

impl DocumentManager {
    fn open_session_ids(&self) -> HashSet<String> {
        self.sessions
            .lock()
            .unwrap()
            .values()
            .map(|s| s.id.clone())
            .collect()
    }
    /// Sessions that ended without closing and still hold unsaved work. Leftovers without unsaved work are deleted here.
    pub fn recoverable_sessions(&self) -> Result<Vec<RecoveryRecord>, AppError> {
        let sessions = self.sessions.lock().unwrap();
        let open: HashSet<&str> = sessions.values().map(|s| s.id.as_str()).collect();
        let mut out = vec![];
        for r in self.global.recoveries().map_err(storage_error)? {
            // Live sessions of this process, or of another ixtable process (lock held).
            if open.contains(r.session_id.as_str()) || workspace_in_use(Path::new(&r.workspace)) {
                continue;
            }
            if r.dirty {
                out.push(r);
                continue;
            }
            let _ = fs::remove_dir_all(&r.workspace);
            let _ = self.global.remove_recovery(&r.session_id);
            logging::info(
                "recovery",
                &format!(
                    "removed clean leftover workspace of session {}",
                    r.session_id
                ),
            );
        }
        Ok(out)
    }
    fn recovery_record(&self, session_id: &str) -> Result<RecoveryRecord, AppError> {
        if self.open_session_ids().contains(session_id) {
            return Err(AppError::new(
                "SESSION_OPEN",
                "That session is still open in this window",
            ));
        }
        let record = self
            .global
            .recoveries()
            .map_err(storage_error)?
            .into_iter()
            .find(|r| r.session_id == session_id)
            .ok_or_else(|| AppError::new("NOT_FOUND", "No unsaved work to recover"))?;
        if workspace_in_use(Path::new(&record.workspace)) {
            return Err(AppError::new(
                "SESSION_OPEN",
                "That work is open in another ixtable window",
            ));
        }
        Ok(record)
    }
    pub fn discard_recovery(&self, session_id: &str) -> Result<(), AppError> {
        match self.recovery_record(session_id) {
            Ok(r) => {
                let _ = fs::remove_dir_all(&r.workspace);
            }
            Err(e) if e.code == "SESSION_OPEN" => return Err(e),
            Err(_) => {}
        }
        self.global
            .remove_recovery(session_id)
            .map_err(storage_error)?;
        logging::info(
            "recovery",
            &format!("discarded unsaved work of session {session_id}"),
        );
        Ok(())
    }
    /// Reopens a crashed session's WIP in `window`, then checkpoints it back into its `.ixt` (after saving a local checkpoint of the archive it replaces). Untitled work opens dirty and needs Save As.
    pub fn recover_session(
        &self,
        window: &str,
        session_id: &str,
    ) -> Result<SessionState, AppError> {
        let record = self.recovery_record(session_id)?;
        let work = PathBuf::from(&record.workspace);
        let doc = validate_workspace(&work, &record).map_err(|reason| {
            logging::error(
                "recovery",
                &format!("session {session_id} could not be recovered: {reason}"),
            );
            AppError::new(
                "RECOVERY_FAILED",
                format!("The unsaved work could not be recovered ({reason}). The last saved archive was kept."),
            )
        })?;
        let path = record.document_path.as_ref().map(PathBuf::from);
        let conflict = |message: &str| Some(AppError::new("EXTERNAL_CONFLICT", message));
        // The saved file is only replaced after a safety checkpoint of it exists.
        let needs_save_as = |why: String| {
            logging::warn(
                "recovery",
                &format!("not saving recovered work in place: {why}"),
            );
            Some(AppError::new(
                RECOVERY_NEEDS_SAVE_AS,
                format!("{why}. The saved file was left untouched; use Save As to keep the recovered work."),
            ))
        };
        let mut blocked = None;
        if let Some(p) = path.as_ref().filter(|p| p.exists()) {
            match archive_io::read_header(p) {
                Ok(h) if h.metadata.document_id == doc.metadata.document_id => {
                    if let Err(e) = self.checkpoint_archive(p, "before-recovery") {
                        blocked = needs_save_as(format!(
                            "A safety copy of {} could not be made ({e})",
                            p.display()
                        ));
                    }
                }
                Ok(_) => blocked = conflict("The file now holds a different document. Use Save As to keep the recovered work."),
                Err(ArchiveError::NewerFormat(_)) => blocked = conflict("The file was saved by a newer ixtable. Use Save As to keep the recovered work."),
                Err(e) => blocked = needs_save_as(format!("{} is unreadable ({e})", p.display())),
            }
        }
        let state =
            self.install_workspace(window, session_id.into(), path.clone(), doc, work, true)?;
        logging::info("recovery", &format!("recovered session {session_id}"));
        if let Some(error) = blocked {
            return self.with_session(window, |s| {
                s.conflict = true;
                s.last_error = Some(error);
                Ok(s.state())
            });
        }
        if path.is_none() {
            return Ok(state);
        }
        match self.save(window, None) {
            Ok(state) => Ok(state),
            Err(_) => self.state(window),
        }
    }
}

/// Reopens unsaved work left by a session that did not close, and saves it back.
pub fn recover_session(window_label: String, session_id: String) -> Result<SessionState, AppError> {
    crate::manager()?.recover_session(&window_label, &session_id)
}

/// Tests only: forgets a session the way a crash would (no close, no cleanup),
/// so integration tests can exercise recovery inside one process.
#[cfg(test)]
pub fn simulate_crash(window_label: String) -> Result<(), AppError> {
    let m = crate::manager()?;
    let session = m.sessions.lock().unwrap().remove(&window_label);
    session
        .map(drop)
        .ok_or_else(|| AppError::new("NO_DOCUMENT", "No document is open"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{add_attachment, create_document};

    fn workspace() -> (PathBuf, RecoveryRecord) {
        let work = std::env::temp_dir().join(format!("ixtable-wip-{}", uuid::Uuid::new_v4()));
        let mut doc = create_document("WIP").unwrap();
        add_attachment(
            &mut doc,
            "a.txt".into(),
            "text/plain".into(),
            b"asset".to_vec(),
        );
        archive_io::extract_document(&doc, &work).unwrap();
        Connection::open(work.join("data.db"))
            .unwrap()
            .execute_batch("CREATE TABLE item(id INTEGER PRIMARY KEY); INSERT INTO item VALUES(1);")
            .unwrap();
        let record = RecoveryRecord {
            session_id: "s1".into(),
            document_id: doc.metadata.document_id.clone(),
            workspace: work.to_string_lossy().into(),
            document_path: None,
            updated_at: Utc::now().to_rfc3339(),
            dirty: true,
            name: Some("WIP".into()),
        };
        (work, record)
    }

    /// A manager over fresh state dirs; a second one over the same dirs stands in for a second ixtable process.
    fn managers() -> (DocumentManager, DocumentManager, PathBuf) {
        let base = std::env::temp_dir().join(format!("ixtable-mgr-{}", uuid::Uuid::new_v4()));
        let make = || DocumentManager::new(base.join("data"), base.join("cache")).unwrap();
        (make(), make(), base)
    }

    /// Opens a titled document in `window`, then leaves it dirty with one new table.
    fn dirty_session(m: &DocumentManager, window: &str, path: &Path) -> SessionState {
        m.new_session(window).unwrap();
        m.save(window, Some(path.to_owned())).unwrap();
        Connection::open(m.database_path(window).unwrap())
            .unwrap()
            .execute_batch(
                "CREATE TABLE notes(id INTEGER PRIMARY KEY); INSERT INTO notes VALUES(1);",
            )
            .unwrap();
        m.mark_data_dirty(window).unwrap()
    }

    #[test]
    fn another_process_never_lists_cleans_or_discards_live_workspaces() {
        let (first, second, base) = managers();
        let clean = first.new_session("clean").unwrap();
        let dirty = dirty_session(&first, "dirty", &base.join("live.ixt"));
        assert!(dirty.dirty);

        // The second process sees both records but neither as abandoned.
        assert!(second.recoverable_sessions().unwrap().is_empty());
        assert!(
            Path::new(&clean.workspace).is_dir(),
            "a clean live workspace is not cleaned up"
        );
        assert_eq!(
            second.discard_recovery(&dirty.session_id).unwrap_err().code,
            "SESSION_OPEN"
        );
        assert_eq!(
            second
                .recover_session("w", &dirty.session_id)
                .unwrap_err()
                .code,
            "SESSION_OPEN"
        );
        assert!(Path::new(&dirty.workspace).join("data.db").is_file());

        // Once the owner is gone (a crash releases the OS lock), the work is recoverable.
        drop(first.sessions.lock().unwrap().remove("dirty"));
        let listed = second.recoverable_sessions().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].session_id, dirty.session_id);
        first.close("clean", true).unwrap();
        assert!(!lock_path(Path::new(&clean.workspace)).exists());
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn committed_writes_stay_dirty_when_the_read_refresh_fails() {
        let (m, _, base) = managers();
        m.new_session("w").unwrap();
        // The write committed, but the reader cannot re-attach the database.
        let db = m.database_path("w").unwrap();
        fs::remove_file(&db).unwrap();
        fs::create_dir(&db).unwrap();
        let state = m.mark_data_dirty("w").unwrap();
        assert!(state.dirty, "the committed write is kept for the next save");
        assert_eq!(
            state.last_error.map(|e| e.code).as_deref(),
            Some(crate::manager::READ_REFRESH_FAILED)
        );
        m.close("w", true).unwrap();
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn recovery_never_overwrites_a_file_it_could_not_checkpoint() {
        let (m, _, base) = managers();
        for (name, damage) in [("corrupt", "checksum"), ("unreadable", "garbage")] {
            let path = base.join(format!("{name}.ixt"));
            let crashed = dirty_session(&m, name, &path);
            drop(m.sessions.lock().unwrap().remove(name));
            if damage == "checksum" {
                // Header still reads, but the archive fails validation, so no checkpoint can be taken.
                Connection::open(&path)
                    .unwrap()
                    .execute_batch("UPDATE data_payload SET checksum='0'")
                    .unwrap();
            } else {
                fs::write(&path, b"not an archive at all").unwrap();
            }
            let before = fs::read(&path).unwrap();
            let state = m.recover_session("w", &crashed.session_id).unwrap();
            assert_eq!(
                state.last_error.map(|e| e.code).as_deref(),
                Some(RECOVERY_NEEDS_SAVE_AS),
                "{name}"
            );
            assert!(
                state.conflict && state.dirty && !state.autosave_eligible,
                "{name}"
            );
            assert_eq!(
                fs::read(&path).unwrap(),
                before,
                "{name}: the file is untouched"
            );
            let rescued = base.join(format!("{name}-rescued.ixt"));
            let saved = m.save("w", Some(rescued.clone())).unwrap();
            assert!(!saved.dirty && saved.last_error.is_none());
            assert!(archive_io::verify(&rescued).is_ok());
            m.close("w", true).unwrap();
        }
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn workspace_assets_must_live_under_their_own_id() {
        let (work, record) = workspace();
        let id = validate_workspace(&work, &record).unwrap().attachments[0]
            .id
            .clone();
        let dir = archive_io::asset_dir(&work, &id);
        let meta = dir.join("metadata.json");
        let mut a: Attachment = serde_json::from_slice(&fs::read(&meta).unwrap()).unwrap();
        a.id = "../../elsewhere".into();
        fs::write(&meta, serde_json::to_vec(&a).unwrap()).unwrap();
        assert!(validate_workspace(&work, &record)
            .unwrap_err()
            .contains("invalid id"));
        fs::remove_dir_all(work).unwrap();
    }

    #[test]
    fn valid_wip_workspaces_load_with_their_assets() {
        let (work, record) = workspace();
        let doc = validate_workspace(&work, &record).unwrap();
        assert_eq!(doc.config.name, "WIP");
        assert_eq!(doc.metadata.document_id, record.document_id);
        assert_eq!(doc.attachments.len(), 1);
        assert!(doc.attachments[0].contents.is_empty());
        fs::remove_dir_all(work).unwrap();
    }

    #[test]
    fn a_truncated_or_missing_config_file_falls_back_to_its_twin() {
        let (work, record) = workspace();
        let json = fs::read(work.join("document.json")).unwrap();
        fs::write(work.join("document.json"), &json[..json.len() / 2]).unwrap();
        assert_eq!(
            validate_workspace(&work, &record).unwrap().config.name,
            "WIP"
        );
        fs::remove_file(work.join("document.json")).unwrap();
        assert_eq!(
            validate_workspace(&work, &record).unwrap().config.name,
            "WIP"
        );
        fs::write(work.join("document.json"), &json).unwrap();
        fs::write(work.join("config.yaml"), b"").unwrap();
        assert_eq!(
            validate_workspace(&work, &record).unwrap().config.name,
            "WIP"
        );
        fs::remove_dir_all(work).unwrap();
    }

    #[test]
    fn config_files_are_replaced_atomically() {
        let (work, _) = workspace();
        let config = DocumentConfig {
            name: "Next".into(),
            ..Default::default()
        };
        archive_io::write_config_files(&work, &config).unwrap();
        assert_eq!(workspace_config(&work).unwrap().name, "Next");
        let leftovers: Vec<_> = fs::read_dir(&work)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        fs::remove_dir_all(work).unwrap();
    }

    #[test]
    fn a_hot_journal_from_a_crash_is_rolled_back_not_rejected() {
        let (work, record) = workspace();
        let crashed = std::env::temp_dir().join(format!("ixtable-hot-{}", uuid::Uuid::new_v4()));
        copy_dir(&work, &crashed);
        // A transaction large enough to spill pages into data.db before commit.
        let c = Connection::open(work.join("data.db")).unwrap();
        c.execute_batch(
            "PRAGMA cache_size=1; BEGIN; CREATE TABLE big(x TEXT);
             WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 2000)
             INSERT INTO big SELECT printf('%0500d', i) FROM n;",
        )
        .unwrap();
        let journal = work.join("data.db-journal");
        assert!(
            journal.is_file(),
            "the open transaction keeps a rollback journal"
        );
        // Copying mid-transaction is what a crash leaves behind: no lock, a hot journal.
        fs::copy(work.join("data.db"), crashed.join("data.db")).unwrap();
        fs::copy(&journal, crashed.join("data.db-journal")).unwrap();
        drop(c);
        let record = RecoveryRecord {
            workspace: crashed.to_string_lossy().into(),
            ..record
        };
        validate_workspace(&crashed, &record).unwrap();
        let rows: i64 = Connection::open(crashed.join("data.db"))
            .unwrap()
            .query_row("SELECT count(*) FROM item", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1, "committed rows survive");
        let big: i64 = Connection::open(crashed.join("data.db"))
            .unwrap()
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'big'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(big, 0, "the uncommitted transaction is rolled back");
        fs::remove_dir_all(work).unwrap();
        fs::remove_dir_all(crashed).unwrap();
    }

    fn copy_dir(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for e in fs::read_dir(from).unwrap().filter_map(Result::ok) {
            if e.path().is_dir() {
                copy_dir(&e.path(), &to.join(e.file_name()));
            } else {
                fs::copy(e.path(), to.join(e.file_name())).unwrap();
            }
        }
    }

    #[test]
    fn invalid_wip_is_reported_not_loaded() {
        let (work, record) = workspace();
        let id = validate_workspace(&work, &record).unwrap().attachments[0]
            .id
            .clone();
        fs::write(archive_io::asset_content(&work, &id), b"tampered").unwrap();
        assert!(validate_workspace(&work, &record)
            .unwrap_err()
            .contains("failed checksum validation"));
        fs::write(work.join("document.json"), b"{\"name\":").unwrap();
        fs::write(work.join("config.yaml"), b"name: [").unwrap();
        let err = validate_workspace(&work, &record).unwrap_err();
        assert!(err.contains("document.json is invalid"), "{err}");
        assert!(err.contains("config.yaml is invalid"), "{err}");
        fs::write(work.join("data.db"), b"not sqlite at all, just text").unwrap();
        assert!(validate_workspace(&work, &record)
            .unwrap_err()
            .contains("data.db"));
        let other = RecoveryRecord {
            document_id: "someone-else".into(),
            ..record
        };
        fs::remove_file(work.join("data.db")).unwrap();
        assert_eq!(
            validate_workspace(&work, &other).unwrap_err(),
            "data.db is missing"
        );
        fs::remove_dir_all(work).unwrap();
    }
}
