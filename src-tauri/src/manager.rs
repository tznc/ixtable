use crate::{
    archive::{self, ArchiveDocument, ArchiveMetadata, Attachment, DocumentConfig, SavedQuery},
    archive_io::{self, DataStamp, Payload, WriteReport, WriteRequest},
    data::{self, ReadRuntime},
    logging,
    storage::{GlobalStorage, RecoveryRecord},
};
use chrono::Utc;
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: String,
    pub message: String,
}
impl AppError {
    pub fn new(code: &str, e: impl ToString) -> Self {
        Self {
            code: code.into(),
            message: e.to_string(),
        }
    }
}
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl From<archive::ArchiveError> for AppError {
    fn from(e: archive::ArchiveError) -> Self {
        let code = match &e {
            archive::ArchiveError::Unsupported(_)
            | archive::ArchiveError::NewerFormat(_)
            | archive::ArchiveError::NewerConfig(_) => "UNSUPPORTED_VERSION",
            archive::ArchiveError::Corrupt(_) => "CORRUPT_PAYLOAD",
            archive::ArchiveError::Invalid(_) | archive::ArchiveError::Sql(_) => "INVALID_ARCHIVE",
            archive::ArchiveError::Io(x) if x.kind() == std::io::ErrorKind::NotFound => {
                "MISSING_FILE"
            }
            _ => "IO_ERROR",
        };
        Self::new(code, e)
    }
}

#[derive(Clone)]
struct Fingerprint {
    modified: Option<SystemTime>,
    size: u64,
    identity: String,
}
pub struct Session {
    pub id: String,
    pub window: String,
    pub path: Option<PathBuf>,
    pub workspace: PathBuf,
    pub doc: ArchiveDocument,
    pub reader: ReadRuntime,
    pub dirty: bool,
    pub conflict: bool,
    pub saving: bool,
    fingerprint: Option<Fingerprint>,
    /// RFC 3339 time of the last successful save in this session.
    pub last_saved_at: Option<String>,
    /// Failure of the most recent save/autosave; cleared by the next successful save.
    pub last_error: Option<AppError>,
    /// Bumped on every edit; a save only clears `dirty` when no edit raced it.
    pub revision: u64,
    /// Bumped on every backend config mutation (see `SessionState::config_revision`).
    pub config_revision: u64,
    /// Archive the session was opened from or last saved to (source of preserved tables).
    pub origin: Option<PathBuf>,
    /// The archive whose data payload equals `data.db` at a known stamp (incremental saves).
    saved_data: Option<SavedData>,
    save_lock: Arc<Mutex<()>>,
    /// OS advisory lock on `<workspace>.lock`, held while the session is open so other ixtable processes never treat the workspace as abandoned.
    pub(crate) workspace_lock: Option<crate::recovery::WorkspaceLock>,
    /// Runtime role enforced at command entry points (see `authz`).
    pub access: crate::authz::Access,
    /// Tables the current role may read (see `authz::ReadableCache`).
    pub(crate) authz_cache: crate::authz::ReadableCache,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
    pub session_id: String,
    pub document_id: String,
    pub name: String,
    pub path: Option<String>,
    pub workspace: String,
    pub dirty: bool,
    pub conflict: bool,
    pub saving: bool,
    pub active_mode: String,
    pub attachment_count: usize,
    pub autosave_eligible: bool,
    pub last_saved_at: Option<String>,
    pub last_error: Option<AppError>,
    /// Runtime-only bundle session: Studio modes hidden, definition read-only.
    pub runtime_only: bool,
    pub bundle_version: Option<String>,
    /// Bumped on every config change made by the backend (update_config and every
    /// command built on it). The frontend config store reloads when it sees a
    /// revision newer than the last one it wrote or loaded.
    pub config_revision: u64,
}
impl Session {
    pub(crate) fn state(&self) -> SessionState {
        SessionState {
            session_id: self.id.clone(),
            document_id: self.doc.metadata.document_id.clone(),
            name: self.doc.config.name.clone(),
            path: self.path.as_ref().map(|p| p.to_string_lossy().into()),
            workspace: self.workspace.to_string_lossy().into(),
            dirty: self.dirty,
            conflict: self.conflict,
            saving: self.saving,
            active_mode: self.doc.config.active_mode.clone(),
            attachment_count: self.doc.attachments.len(),
            autosave_eligible: self.path.is_some() && !self.conflict && !self.saving,
            last_saved_at: self.last_saved_at.clone(),
            last_error: self.last_error.clone(),
            runtime_only: crate::installation::runtime_session(&self.id).is_some(),
            bundle_version: crate::installation::runtime_version(&self.id),
            config_revision: self.config_revision,
        }
    }
}

/// `archive`'s data payload was packed from `data.db` when it had `stamp`.
#[derive(Clone)]
pub(crate) struct SavedData {
    archive: PathBuf,
    stamp: DataStamp,
}

/// Everything a save needs, captured under the sessions lock so the archive write
/// itself runs without holding it.
#[derive(Clone)]
pub(crate) struct Snapshot {
    pub session_id: String,
    pub metadata: ArchiveMetadata,
    pub config: DocumentConfig,
    pub attachments: Vec<Attachment>,
    pub workspace: PathBuf,
    pub origin: Option<PathBuf>,
    pub revision: u64,
    /// `origin` when it is the session's file and unchanged on disk: unchanged payloads
    /// are copied from it instead of recompressed.
    pub reuse_from: Option<PathBuf>,
    saved_data: Option<SavedData>,
}

pub struct DocumentManager {
    pub(crate) sessions: Mutex<HashMap<String, Session>>,
    pub global: GlobalStorage,
    pub(crate) recovery_root: PathBuf,
    pub(crate) checkpoint_root: PathBuf,
}
impl DocumentManager {
    pub fn new(app_data: PathBuf, cache: PathBuf) -> Result<Self, AppError> {
        fs::create_dir_all(&cache).map_err(|e| AppError::new("IO_ERROR", e))?;
        if let Some(state) = app_data.parent() {
            logging::init(state.join("logs"));
        }
        Ok(Self {
            sessions: Mutex::new(HashMap::new()),
            global: GlobalStorage::new(app_data.join("global.db"))
                .map_err(|e| AppError::new("IO_ERROR", e))?,
            recovery_root: cache.join("recovery"),
            checkpoint_root: app_data.join("checkpoints"),
        })
    }
    pub fn new_session(&self, window: &str) -> Result<SessionState, AppError> {
        let mut doc = archive::create_document("Untitled")?;
        let id = Uuid::new_v4().to_string();
        let workspace = self.recovery_root.join(&id);
        archive_io::extract_document(&doc, &workspace)?;
        doc.data = vec![];
        self.install_workspace(window, id, None, doc, workspace, false)
    }
    /// Starts a session from an in-memory document, extracted into a fresh workspace.
    fn install(
        &self,
        window: &str,
        path: Option<PathBuf>,
        mut doc: ArchiveDocument,
    ) -> Result<SessionState, AppError> {
        let id = Uuid::new_v4().to_string();
        let workspace = self.recovery_root.join(&id);
        archive_io::extract_document(&doc, &workspace)?;
        doc.data = vec![];
        for a in &mut doc.attachments {
            a.contents = vec![];
        }
        self.install_workspace(window, id, path, doc, workspace, false)
    }
    pub fn open(&self, window: &str, path: &Path) -> Result<SessionState, AppError> {
        let id = Uuid::new_v4().to_string();
        let workspace = self.recovery_root.join(&id);
        let mut doc = archive_io::extract_to(path, &workspace).map_err(|e| {
            let _ = fs::remove_dir_all(&workspace);
            logging::warn("open", &format!("could not open {}: {e}", path.display()));
            AppError::from(e)
        })?;
        // Before anything can write: data.db now equals the archive's data payload.
        let saved_data = archive_io::data_stamp(&workspace).map(|stamp| SavedData {
            archive: path.to_owned(),
            stamp,
        });
        // Studio upgrade: the old default form/navigation id `main` becomes a UUIDv7.
        match crate::design::upgrade::rekey_legacy_ids(&mut doc.config) {
            Ok(ids) if ids != Default::default() => {
                archive_io::write_config_files(&workspace, &doc.config)?;
                logging::info("open", "replaced legacy `main` design ids with UUIDv7 ids");
            }
            Ok(_) => {}
            Err(e) => logging::warn("open", &format!("could not upgrade legacy ids: {e}")),
        }
        self.global
            .add_recent(path)
            .map_err(|e| AppError::new("IO_ERROR", e))?;
        logging::info("open", &format!("opened {}", path.display()));
        let state = self.install_workspace(
            window,
            id.clone(),
            Some(path.to_owned()),
            doc,
            workspace,
            false,
        )?;
        if let Some(s) = self.sessions.lock().unwrap().get_mut(window) {
            if s.id == id {
                s.saved_data = saved_data;
            }
        }
        Ok(state)
    }
    /// Starts a session over an extracted workspace (`recovery/<sessionId>`). Any session already in `window` is replaced and its workspace removed.
    pub(crate) fn install_workspace(
        &self,
        window: &str,
        id: String,
        path: Option<PathBuf>,
        doc: ArchiveDocument,
        workspace: PathBuf,
        dirty: bool,
    ) -> Result<SessionState, AppError> {
        let workspace_lock = match crate::recovery::try_lock_workspace(&workspace) {
            Ok(Some(lock)) => Some(lock),
            Ok(None) => {
                return Err(AppError::new(
                    "SESSION_OPEN",
                    "That work is open in another ixtable window",
                ))
            }
            Err(e) => {
                logging::warn(
                    "recovery",
                    &format!("could not lock workspace of session {id}: {e}"),
                );
                None
            }
        };
        let mut reader = ReadRuntime::new(&workspace, &sqlite_extension_path()?)
            .map_err(|e| AppError::new("EXTENSION_STARTUP", e))?;
        crate::recordstore::attach_configured(&mut reader, &doc.config);
        let fp = path
            .as_ref()
            .filter(|p| p.exists())
            .map(|p| fingerprint(p, &doc.metadata.document_id))
            .transpose()?;
        let r = RecoveryRecord {
            session_id: id.clone(),
            document_id: doc.metadata.document_id.clone(),
            workspace: workspace.to_string_lossy().into(),
            document_path: path.as_ref().map(|x| x.to_string_lossy().into()),
            updated_at: Utc::now().to_rfc3339(),
            dirty,
            name: Some(doc.config.name.clone()),
        };
        let s = Session {
            id: id.clone(),
            window: window.into(),
            origin: path.clone(),
            saved_data: None,
            path,
            workspace,
            doc,
            reader,
            dirty,
            conflict: false,
            saving: false,
            fingerprint: fp,
            last_saved_at: None,
            last_error: None,
            revision: 0,
            config_revision: 0,
            save_lock: Arc::default(),
            workspace_lock,
            access: Default::default(),
            authz_cache: Default::default(),
        };
        let state = s.state();
        // Registered under the sessions lock so recovery listing never sees it as abandoned.
        let mut all = self.sessions.lock().unwrap();
        let replaced = all.insert(window.into(), s);
        if let Err(e) = self.global.register_recovery(&r) {
            logging::warn("recovery", &format!("could not register session {id}: {e}"));
        }
        drop(all);
        if let Some(old) = replaced {
            self.dispose(old);
        }
        Ok(state)
    }
    /// Drops a session that is no longer open and deletes its WIP workspace.
    fn dispose(&self, mut old: Session) {
        let (id, workspace) = (old.id.clone(), old.workspace.clone());
        // Released only after the workspace and its record are gone.
        let lock = old.workspace_lock.take();
        crate::cloud::grants::release(&old.doc.config.datasource);
        drop(old);
        if crate::installation::runtime_session(&id).is_some() {
            crate::installation::forget_runtime(&id);
            return;
        }
        let _ = fs::remove_dir_all(workspace);
        let _ = self.global.remove_recovery(&id);
        drop(lock);
    }
    /// Records an edit: bumps the revision and flags the recovery record dirty once.
    pub(crate) fn touch(&self, s: &mut Session) {
        s.revision += 1;
        if !s.dirty {
            s.dirty = true;
            let _ = self
                .global
                .update_recovery(&s.id, true, s.path.as_deref(), &s.doc.config.name);
        }
    }
    pub(crate) fn with_session<T>(
        &self,
        window: &str,
        f: impl FnOnce(&mut Session) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let mut all = self.sessions.lock().unwrap();
        let s = all
            .get_mut(window)
            .ok_or_else(|| AppError::new("NO_DOCUMENT", "No document is open"))?;
        f(s)
    }
    pub fn state(&self, window: &str) -> Result<SessionState, AppError> {
        self.sessions
            .lock()
            .unwrap()
            .get(window)
            .map(Session::state)
            .ok_or_else(|| AppError::new("NO_DOCUMENT", "No document is open"))
    }
    pub fn config(&self, window: &str) -> Result<DocumentConfig, AppError> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .get(window)
            .ok_or_else(|| AppError::new("NO_DOCUMENT", "No document is open"))?
            .doc
            .config
            .clone())
    }
    pub fn config_yaml(&self, window: &str) -> Result<String, AppError> {
        archive::document_config_yaml(&self.config(window)?)
            .map_err(|e| AppError::new("INVALID_CONFIG", e.to_string()))
    }
    pub fn apply_config_yaml(&self, window: &str, yaml: &str) -> Result<SessionState, AppError> {
        let config = archive::document_config_from_yaml(yaml)
            .map_err(|e| AppError::new("INVALID_CONFIG", e.to_string()))?;
        self.update_config(window, config)
    }
    pub fn database_path(&self, window: &str) -> Result<PathBuf, AppError> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .get(window)
            .ok_or_else(|| AppError::new("NO_DOCUMENT", "No document is open"))?
            .workspace
            .join("data.db"))
    }
    pub fn mark_data_dirty(&self, window: &str) -> Result<SessionState, AppError> {
        let mut all = self.sessions.lock().unwrap();
        let s = all
            .get_mut(window)
            .ok_or_else(|| AppError::new("NO_DOCUMENT", "No document is open"))?;
        // The write is already committed: record it before anything that can fail.
        // Runtime installation writes are durable immediately; there is nothing to save.
        if crate::installation::runtime_session(&s.id).is_none() {
            self.touch(s);
        }
        refresh_after_write(s);
        Ok(s.state())
    }
    pub fn database_objects(&self, window: &str) -> Result<Vec<data::DbObject>, AppError> {
        let all = self.sessions.lock().unwrap();
        all.get(window)
            .ok_or_else(|| AppError::new("NO_DOCUMENT", "No document is open"))?
            .reader
            .objects()
            .map_err(|e| AppError::new("IO_ERROR", e))
    }
    pub fn table_schema(&self, window: &str, table: &str) -> Result<data::TableSchema, AppError> {
        let all = self.sessions.lock().unwrap();
        let session = all
            .get(window)
            .ok_or_else(|| AppError::new("NO_DOCUMENT", "No document is open"))?;
        session.reader.schema(table).map_err(|e| {
            let code = if e.contains("does not exist") {
                "NOT_FOUND"
            } else {
                "IO_ERROR"
            };
            AppError::new(code, e)
        })
    }
    pub fn table_totals(
        &self,
        window: &str,
        table: &str,
        filters: &[data::Filter],
        specs: &[data::totals::TotalSpec],
    ) -> Result<Vec<data::DataValue>, AppError> {
        let all = self.sessions.lock().unwrap();
        all.get(window)
            .ok_or_else(|| AppError::new("NO_DOCUMENT", "No document is open"))?
            .reader
            .totals(table, filters, specs)
            .map_err(|e| AppError::new("IO_ERROR", e))
    }
    pub fn table_page(
        &self,
        window: &str,
        table: &str,
        offset: u64,
        limit: u64,
        sorts: &[data::Sort],
        filters: &[data::Filter],
    ) -> Result<data::Page, AppError> {
        let all = self.sessions.lock().unwrap();
        all.get(window)
            .ok_or_else(|| AppError::new("NO_DOCUMENT", "No document is open"))?
            .reader
            .page(table, offset, limit, sorts, filters)
            .map_err(|e| AppError::new("IO_ERROR", e))
    }
    /// The unpaged page query of `table` (exports stream it on `read_connection`).
    pub fn table_plan(
        &self,
        window: &str,
        table: &str,
        sorts: &[data::Sort],
        filters: &[data::Filter],
    ) -> Result<data::page::PagePlan, AppError> {
        let all = self.sessions.lock().unwrap();
        let reader = &all
            .get(window)
            .ok_or_else(|| AppError::new("NO_DOCUMENT", "No document is open"))?
            .reader;
        let _gate = reader.read_gate().map_err(|e| AppError::new("BUSY", e))?;
        reader
            .page_plan(table, sorts, filters)
            .map_err(|e| AppError::new("IO_ERROR", e))
    }
    pub fn read_query(&self, window: &str, sql: &str) -> Result<data::QueryResult, AppError> {
        let all = self.sessions.lock().unwrap();
        all.get(window)
            .ok_or_else(|| AppError::new("NO_DOCUMENT", "No document is open"))?
            .reader
            .query(sql)
            .map_err(|e| read_error(sql, e))
    }
    /// A fresh DuckDB connection to the session's read database (queries.rs runs long, cancellable reads on it without holding the session lock).
    /// It holds the shared gate of the reader's `data` catalog until dropped, so neither a RecordStore write to the embedded file nor a reader refresh (SQLite or PostgreSQL) overlaps the read (see `data::gate`).
    pub fn read_connection(
        &self,
        window: &str,
    ) -> Result<data::gate::Gated<duckdb::Connection>, AppError> {
        let all = self.sessions.lock().unwrap();
        let reader = &all
            .get(window)
            .ok_or_else(|| AppError::new("NO_DOCUMENT", "No document is open"))?
            .reader;
        let connection = reader
            .connection()
            .try_clone()
            .map_err(|e| AppError::new("DATABASE_ERROR", e.to_string()))?;
        let db = reader.workspace.join("data.db");
        drop(all);
        let gate = data::gate::shared(&db).map_err(|e| AppError::new("BUSY", e))?;
        let _ = connection.execute_batch("USE data");
        Ok(data::gate::Gated::new(connection, Some(gate)))
    }
    pub fn save_query(
        &self,
        window: &str,
        id: Option<String>,
        name: String,
        sql: String,
        filter_state: Option<serde_json::Value>,
    ) -> Result<DocumentConfig, AppError> {
        // Prepared, not run: `$name` parameters need no values to be saved.
        crate::queries::check_on(&*self.read_connection(window)?, &sql)?;
        let mut config = self.config(window)?;
        let id = id.unwrap_or_else(|| Uuid::now_v7().to_string());
        let previous = config.saved_queries.iter().find(|query| query.id == id);
        let query = SavedQuery {
            id: id.clone(),
            name,
            sql,
            filter_state,
            ..previous.cloned().unwrap_or_default()
        };
        if let Some(saved) = config.saved_queries.iter_mut().find(|query| query.id == id) {
            *saved = query;
        } else {
            config.saved_queries.push(query);
        }
        self.update_config(window, config.clone())?;
        Ok(config)
    }
    pub fn update_config(&self, window: &str, c: DocumentConfig) -> Result<SessionState, AppError> {
        c.design
            .validate()
            .map_err(|e| AppError::new("INVALID_DESIGN_SCHEMA", e))?;
        let mut all = self.sessions.lock().unwrap();
        let s = all
            .get_mut(window)
            .ok_or_else(|| AppError::new("NO_DOCUMENT", "No document is open"))?;
        read_only_guard(s)?;
        let reattach = |old: &DocumentConfig| {
            old.datasource != c.datasource || old.file_sources != c.file_sources
        };
        let datasource_changed = reattach(&s.doc.config);
        let workspace = s.workspace.clone();
        drop(all);
        // Attaching a datasource can block (PostgreSQL connect timeout), so a new
        // reader is built outside the sessions lock and swapped in below.
        let prepared = if datasource_changed {
            let mut reader = ReadRuntime::new(&workspace, &sqlite_extension_path()?)
                .map_err(|e| AppError::new("EXTENSION_STARTUP", e))?;
            crate::recordstore::attach_configured(&mut reader, &c);
            Some(((c.datasource.clone(), c.file_sources.clone()), reader))
        } else {
            None
        };
        let mut all = self.sessions.lock().unwrap();
        let s = all
            .get_mut(window)
            .ok_or_else(|| AppError::new("NO_DOCUMENT", "No document is open"))?;
        read_only_guard(s)?;
        if reattach(&s.doc.config) {
            match prepared {
                Some(((ds, files), reader))
                    if ds == c.datasource
                        && files == c.file_sources
                        && s.workspace == workspace =>
                {
                    s.reader = reader
                }
                // Another edit changed the datasource meanwhile: attach inline.
                _ => crate::recordstore::attach_configured(&mut s.reader, &c),
            }
        }
        s.doc.config = c;
        s.config_revision += 1;
        s.authz_cache.clear();
        self.touch(s);
        archive_io::write_config_files(&s.workspace, &s.doc.config)?;
        Ok(s.state())
    }
    /// Saves into the session's archive (or `new_path`). Waits for an in-flight autosave first; the archive write runs without holding the sessions lock.
    pub fn save(&self, window: &str, new_path: Option<PathBuf>) -> Result<SessionState, AppError> {
        let lock = self.with_session(window, |s| Ok(s.save_lock.clone()))?;
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        self.save_now(window, new_path, "save")
    }
    /// Saves only when the session is dirty, has a path, and is not conflicted. Skips (returning the current state) while another save is running.
    pub fn autosave(&self, window: &str) -> Result<SessionState, AppError> {
        let lock = self.with_session(window, |s| Ok(s.save_lock.clone()))?;
        let Ok(_guard) = lock.try_lock() else {
            return self.state(window);
        };
        let eligible = self.with_session(window, |s| {
            Ok(s.dirty
                && s.path.is_some()
                && !s.conflict
                && crate::installation::runtime_session(&s.id).is_none())
        })?;
        if !eligible {
            return self.state(window);
        }
        self.save_now(window, None, "autosave")
    }
    /// Captures what a save needs. Callers write the archive outside the lock.
    pub(crate) fn snapshot(&self, s: &Session) -> Snapshot {
        Snapshot {
            session_id: s.id.clone(),
            metadata: s.doc.metadata.clone(),
            config: s.doc.config.clone(),
            attachments: s.doc.attachments.clone(),
            workspace: s.workspace.clone(),
            origin: s.origin.clone(),
            revision: s.revision,
            reuse_from: s
                .origin
                .clone()
                .filter(|o| s.path.as_ref() == Some(o) && s.fingerprint.is_some())
                .filter(|o| matches!(changed(o, s.fingerprint.as_ref()), Ok(false))),
            saved_data: s.saved_data.clone(),
        }
    }
    /// Packs a snapshot into a validated archive at `dest` (atomic replace).
    pub(crate) fn write_snapshot(
        &self,
        snap: &Snapshot,
        dest: &Path,
    ) -> Result<WriteReport, AppError> {
        // Read before the snapshot: a commit in between makes the next save re-pack.
        let stamp = {
            let db = snap.workspace.join("data.db");
            data::gate::shared(&db)
                .ok()
                .and_then(|_gate| archive_io::data_stamp(&snap.workspace))
        };
        let reuse_data = match (&snap.reuse_from, &snap.saved_data, &stamp) {
            (Some(from), Some(saved), Some(now)) => &saved.archive == from && &saved.stamp == now,
            _ => false,
        };
        let result = self.pack(snap, dest, reuse_data);
        // A previous archive that cannot be reused (moved, damaged) falls back to a full write.
        let result = match result {
            Err(e) if snap.reuse_from.is_some() => {
                logging::warn(
                    "save",
                    &format!("incremental save failed, writing in full: {e}"),
                );
                self.pack(
                    &Snapshot {
                        reuse_from: None,
                        saved_data: None,
                        ..snap.clone()
                    },
                    dest,
                    false,
                )
            }
            other => other,
        };
        result.map(|report| WriteReport {
            data_stamp: stamp,
            ..report
        })
    }
    /// Writes `snap` to `dest`, copying the data payload from `reuse_from` when `reuse_data`.
    fn pack(
        &self,
        snap: &Snapshot,
        dest: &Path,
        reuse_data: bool,
    ) -> Result<WriteReport, AppError> {
        let data = snap
            .workspace
            .join(format!(".snapshot-{}.db", Uuid::new_v4()));
        if !reuse_data {
            vacuum_into(&snap.workspace.join("data.db"), &data)?;
        }
        let assets: Vec<PathBuf> = snap
            .attachments
            .iter()
            .map(|a| archive_io::asset_content(&snap.workspace, &a.id))
            .collect();
        let result = archive_io::write(
            dest,
            WriteRequest {
                metadata: &snap.metadata,
                config: &snap.config,
                data: if reuse_data {
                    Payload::Reuse
                } else {
                    Payload::File(&data)
                },
                attachments: snap
                    .attachments
                    .iter()
                    .zip(&assets)
                    .map(|(a, p)| (a, Payload::File(p)))
                    .collect(),
                preserve_from: snap.origin.as_deref(),
                preserve_copy: false,
                reuse_from: snap.reuse_from.as_deref(),
            },
        );
        let _ = fs::remove_file(&data);
        Ok(result?)
    }
    fn save_now(
        &self,
        window: &str,
        new_path: Option<PathBuf>,
        kind: &str,
    ) -> Result<SessionState, AppError> {
        let (snap, path) = self.with_session(window, |s| {
            read_only_guard(s)?;
            let path = new_path.or_else(|| s.path.clone()).ok_or_else(|| {
                AppError::new("SAVE_AS_REQUIRED", "Untitled documents need a location")
            })?;
            let external = match s.path.as_ref() == Some(&path) {
                true => changed(&path, s.fingerprint.as_ref()),
                false => Ok(false),
            };
            let blocked = match external {
                Ok(false) => None,
                Ok(true) => {
                    s.conflict = true;
                    Some(AppError::new(
                        "EXTERNAL_CONFLICT",
                        "The file changed outside ixtable. Reload it or Save As.",
                    ))
                }
                Err(e) => Some(e),
            };
            if let Some(error) = blocked {
                s.last_error = Some(error.clone());
                logging::warn(kind, &format!("{}: {error}", path.display()));
                return Err(error);
            }
            s.saving = true;
            Ok((self.snapshot(s), path))
        })?;
        let started = Instant::now();
        // Extracted data.db holds WIP record edits. Pack it into the archive so the `.ixt` file remains the source of truth after a successful save.
        let result = self.write_snapshot(&snap, &path);
        let mut all = self.sessions.lock().unwrap();
        let Some(s) = all.get_mut(window).filter(|s| s.id == snap.session_id) else {
            return Err(AppError::new(
                "NO_DOCUMENT",
                "The document was closed while saving",
            ));
        };
        s.saving = false;
        let report = match result {
            Ok(report) => report,
            Err(error) => {
                logging::error(kind, &format!("{} failed: {error}", path.display()));
                s.last_error = Some(error.clone());
                return Err(error);
            }
        };
        for stale in archive_io::stale_temp_files(&path) {
            let _ = fs::remove_file(stale);
        }
        s.path = Some(path.clone());
        s.origin = Some(path.clone());
        s.saved_data = report.data_stamp.clone().map(|stamp| SavedData {
            archive: path.clone(),
            stamp,
        });
        s.fingerprint = Some(fingerprint(&path, &s.doc.metadata.document_id)?);
        s.dirty = s.revision != snap.revision;
        s.conflict = false;
        s.last_saved_at = Some(Utc::now().to_rfc3339());
        s.last_error = None;
        let _ = self
            .global
            .update_recovery(&s.id, s.dirty, Some(&path), &s.doc.config.name);
        logging::info(
            kind,
            &format!(
                "saved {} ({} bytes, {} payloads reused) in {} ms{}",
                path.display(),
                report.bytes,
                report.reused.len(),
                started.elapsed().as_millis(),
                if report.preserved_tables.is_empty() {
                    String::new()
                } else {
                    format!("; preserved {}", report.preserved_tables.join(", "))
                }
            ),
        );
        let state = s.state();
        drop(all);
        self.global
            .add_recent(&path)
            .map_err(|e| AppError::new("IO_ERROR", e))?;
        Ok(state)
    }
    pub fn reload(&self, window: &str) -> Result<SessionState, AppError> {
        let path = self
            .sessions
            .lock()
            .unwrap()
            .get(window)
            .and_then(|s| s.path.clone())
            .ok_or_else(|| AppError::new("MISSING_FILE", "No saved file"))?;
        self.open(window, &path)
    }
    pub fn close(&self, window: &str, force: bool) -> Result<(), AppError> {
        let mut all = self.sessions.lock().unwrap();
        let s = all
            .get(window)
            .ok_or_else(|| AppError::new("NO_DOCUMENT", "No document is open"))?;
        if !force && (s.dirty || s.conflict || s.saving) {
            return Err(AppError::new(
                "CLOSE_BLOCKED",
                "Save or resolve the document before closing",
            ));
        }
        let mut s = all.remove(window).unwrap();
        // Released only after the workspace and its record are gone.
        let lock = s.workspace_lock.take();
        crate::cloud::grants::release(&s.doc.config.datasource);
        if crate::installation::runtime_session(&s.id).is_some() {
            // The workspace is the installation itself; keep it.
            crate::installation::forget_runtime(&s.id);
            return Ok(());
        }
        let (id, workspace) = (s.id.clone(), s.workspace.clone());
        drop(s);
        drop(all);
        fs::remove_dir_all(&workspace).map_err(|e| AppError::new("IO_ERROR", e))?;
        self.global
            .remove_recovery(&id)
            .map_err(|e| AppError::new("IO_ERROR", e))?;
        drop(lock);
        logging::info("close", &format!("closed session {id}"));
        Ok(())
    }
}
impl DocumentManager {
    /// Opens an installed runtime bundle (bundle agent). The session's workspace is the installation directory, so record writes land in the installation's `data.db`; it is not a recovery workspace and is never deleted on close.
    pub fn open_runtime_session(
        &self,
        window: &str,
        installation: &Path,
        mut doc: ArchiveDocument,
        runtime: crate::installation::RuntimeSession,
    ) -> Result<SessionState, AppError> {
        // Cloud and manual installs share bundle ids: only a manual install names one.
        let ds = &mut doc.config.datasource;
        if crate::cloud::install::is_cloud_dir(&runtime.dir) {
            ds.grant_scope = Some(window.to_string());
        } else {
            ds.installation = Some(runtime.bundle_id.clone());
        }
        // The reader follows the bundle's datasource, like the record store does
        // (built before the sessions lock: a PostgreSQL attach can block).
        crate::import::sources::materialize(installation, &doc);
        let mut reader = ReadRuntime::new(installation, &sqlite_extension_path()?)
            .map_err(|e| AppError::new("EXTENSION_STARTUP", e))?;
        crate::recordstore::attach_configured(&mut reader, &doc.config);
        let state = self.install(window, None, doc)?;
        let mut all = self.sessions.lock().unwrap();
        let s = all
            .get_mut(window)
            .ok_or_else(|| AppError::new("NO_DOCUMENT", "No document is open"))?;
        let extracted = std::mem::replace(&mut s.workspace, installation.to_owned());
        s.reader = reader;
        s.authz_cache.clear();
        let _ = fs::remove_dir_all(extracted);
        let _ = self.global.remove_recovery(&state.session_id);
        crate::installation::register_runtime(&state.session_id, runtime);
        Ok(s.state())
    }
}
/// Error code of a committed write whose read refresh failed (reads may be stale).
pub const READ_REFRESH_FAILED: &str = "READ_REFRESH_FAILED";

/// Refreshes the DuckDB reader after a committed write. A failure never fails the
/// write: it is surfaced as `lastError` (`READ_REFRESH_FAILED`) and cleared by the
/// next successful refresh.
pub(crate) fn refresh_after_write(s: &mut Session) {
    // A write may have changed the schema (foreign keys feed `readable_tables`).
    s.authz_cache.clear();
    match s.reader.refresh() {
        Ok(()) => {
            if s.last_error
                .as_ref()
                .is_some_and(|e| e.code == READ_REFRESH_FAILED)
            {
                s.last_error = None;
            }
        }
        Err(e) => {
            logging::warn(
                "data",
                &format!("read refresh after a committed write failed: {e}"),
            );
            s.last_error = Some(AppError::new(
                READ_REFRESH_FAILED,
                format!("The change was saved, but the view could not be refreshed and may be out of date ({e})."),
            ));
        }
    }
}
/// Classifies a failed ad-hoc read: only SQL the read-only guard rejects is
/// `READ_ONLY`; a busy file is `BUSY` (never mislabelled as read-only, see
/// `data::gate`); anything else is a `DATABASE_ERROR`.
fn read_error(sql: &str, message: String) -> AppError {
    let lower = message.to_ascii_lowercase();
    let code = if data::read_only_guard(sql).is_err() {
        "READ_ONLY"
    } else if message == data::gate::BUSY_MESSAGE
        || lower.contains("database is locked")
        || lower.contains("database is busy")
    {
        "BUSY"
    } else {
        "DATABASE_ERROR"
    };
    AppError::new(code, message)
}
pub(crate) fn read_only_guard(s: &Session) -> Result<(), AppError> {
    if crate::installation::runtime_session(&s.id).is_some() {
        return Err(AppError::new(
            "READ_ONLY",
            "This application is a runtime-only bundle; its definition cannot be changed",
        ));
    }
    Ok(())
}
fn fingerprint(path: &Path, identity: &str) -> Result<Fingerprint, AppError> {
    let m = fs::metadata(path).map_err(|e| AppError::new("IO_ERROR", e))?;
    Ok(Fingerprint {
        modified: m.modified().ok(),
        size: m.len(),
        identity: identity.into(),
    })
}
fn changed(path: &Path, old: Option<&Fingerprint>) -> Result<bool, AppError> {
    let Some(old) = old else { return Ok(false) };
    let m = fs::metadata(path).map_err(|e| AppError::new("MISSING_FILE", e))?;
    if m.len() != old.size || m.modified().ok() != old.modified {
        return Ok(true);
    };
    match archive::read_header(path) {
        Ok(h) => Ok(h.metadata.document_id != old.identity),
        Err(_) => Ok(true),
    }
}
/// Consistent copy of a live SQLite database, even while other connections write.
pub(crate) fn vacuum_into(db: &Path, dest: &Path) -> Result<(), AppError> {
    let io = |e: rusqlite::Error| AppError::new("IO_ERROR", format!("snapshot data.db: {e}"));
    let conn = rusqlite::Connection::open_with_flags(
        db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(io)?;
    conn.busy_timeout(Duration::from_secs(10)).map_err(io)?;
    conn.execute("VACUUM INTO ?1", [dest.to_string_lossy().as_ref()])
        .map_err(io)?;
    Ok(())
}

fn sqlite_extension_path() -> Result<PathBuf, AppError> {
    crate::data::sqlite_extension_path().map_err(|e| AppError::new("EXTENSION_STARTUP", e))
}

/// Saves the document when it is dirty, has a path, and is not conflicted; otherwise
/// returns the current state unchanged (PRD §7.2 debounced autosave).
#[tauri::command]
pub fn autosave_document(window_label: String) -> Result<SessionState, AppError> {
    crate::manager()?.autosave(&window_label)
}
#[cfg(test)]
mod config_tests {
    include!("manager_config_tests.rs");
}
