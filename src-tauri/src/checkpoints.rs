//! Local recovery checkpoints (PRD §7.2, §24): validated archive copies stored under
//! `<state>/data/checkpoints/<documentId>/<checkpointId>.ixt` with a JSON sidecar.
//!
//! A checkpoint packs the session's current state, including unsaved WIP, so a
//! migration or recovery can always be undone by restoring it as a copy.
use crate::{
    archive::ArchiveMetadata,
    archive_io::{self, Payload, WriteRequest},
    logging,
    manager::{AppError, DocumentManager},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

/// Checkpoints kept per document; older ones are pruned.
pub const KEEP_CHECKPOINTS: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointInfo {
    pub id: String,
    pub document_id: String,
    pub reason: String,
    pub created_at: String,
    pub path: String,
    pub size: u64,
}

fn io_error(e: impl ToString) -> AppError {
    AppError::new("IO_ERROR", e)
}

impl DocumentManager {
    /// `checkpoints/<documentId>`; the id must be a plain id (never a path).
    fn checkpoint_dir(&self, document_id: &str) -> Result<PathBuf, AppError> {
        if !crate::paths::is_safe_id(document_id) {
            return Err(AppError::new(
                "INVALID_ARCHIVE",
                format!("unsafe document id {document_id:?}"),
            ));
        }
        Ok(crate::paths::child(&self.checkpoint_root, document_id))
    }
    fn record_checkpoint(
        &self,
        document_id: &str,
        id: String,
        reason: &str,
        path: &Path,
    ) -> Result<CheckpointInfo, AppError> {
        let info = CheckpointInfo {
            id,
            document_id: document_id.into(),
            reason: reason.into(),
            created_at: Utc::now().to_rfc3339(),
            path: path.to_string_lossy().into(),
            size: fs::metadata(path).map_err(io_error)?.len(),
        };
        crate::bundle::write_atomic(
            &path.with_extension("json"),
            &serde_json::to_vec_pretty(&info).map_err(io_error)?,
        )?;
        logging::info(
            "checkpoint",
            &format!(
                "created checkpoint {} ({reason}) for {document_id}",
                info.id
            ),
        );
        self.prune_checkpoints(document_id);
        Ok(info)
    }
    /// Checkpoints whose sidecar names `<dir>/<id>.ixt` for its own safe id and document; anything else in the directory is ignored (and never pruned).
    fn checkpoints_of(&self, document_id: &str) -> Vec<CheckpointInfo> {
        let Ok(dir) = self.checkpoint_dir(document_id) else {
            return vec![];
        };
        let expected = |c: &CheckpointInfo| {
            crate::paths::is_safe_id(&c.id)
                && c.document_id == document_id
                && Path::new(&c.path) == dir.join(format!("{}.ixt", c.id))
        };
        let mut all: Vec<CheckpointInfo> = fs::read_dir(&dir)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "json"))
                    .filter_map(|p| fs::read(p).ok())
                    .filter_map(|b| serde_json::from_slice::<CheckpointInfo>(&b).ok())
                    .filter(expected)
                    .filter(|c| Path::new(&c.path).is_file())
                    .collect()
            })
            .unwrap_or_default();
        all.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        all
    }
    /// Deletes only `<checkpoint dir>/<id>.{ixt,json}` of listed checkpoints beyond the newest [`KEEP_CHECKPOINTS`].
    fn prune_checkpoints(&self, document_id: &str) {
        let Ok(dir) = self.checkpoint_dir(document_id) else {
            return;
        };
        for old in self
            .checkpoints_of(document_id)
            .into_iter()
            .skip(KEEP_CHECKPOINTS)
        {
            let path = crate::paths::child(&dir, &old.id).with_extension("ixt");
            if path.parent() != Some(dir.as_path()) {
                continue;
            }
            let _ = fs::remove_file(path.with_extension("json"));
            let _ = fs::remove_file(path);
        }
    }
    /// Packs the session's current state (including unsaved edits) into a validated local checkpoint archive.
    pub fn create_checkpoint(
        &self,
        window: &str,
        reason: &str,
    ) -> Result<CheckpointInfo, AppError> {
        let snap = self.with_session(window, |s| Ok(self.snapshot(s)))?;
        let id = Uuid::new_v4().to_string();
        let document_id = snap.metadata.document_id.clone();
        let path = self.checkpoint_dir(&document_id)?.join(format!("{id}.ixt"));
        self.write_snapshot(&snap, &path).inspect_err(|e| {
            logging::error(
                "checkpoint",
                &format!("checkpoint for {document_id} failed: {e}"),
            );
        })?;
        self.record_checkpoint(&document_id, id, reason, &path)
    }
    /// Copies an existing archive file into the checkpoint store after validating it.
    pub fn checkpoint_archive(
        &self,
        archive: &Path,
        reason: &str,
    ) -> Result<CheckpointInfo, AppError> {
        let document_id = archive_io::verify(archive)?.metadata.document_id;
        let id = Uuid::new_v4().to_string();
        let dir = self.checkpoint_dir(&document_id)?;
        fs::create_dir_all(&dir).map_err(io_error)?;
        let path = dir.join(format!("{id}.ixt"));
        let part = dir.join(format!("{id}.part"));
        let copied = fs::copy(archive, &part)
            .map_err(io_error)
            .and_then(|_| Ok(archive_io::verify(&part)?))
            .and_then(|_| fs::rename(&part, &path).map_err(io_error));
        if let Err(e) = copied {
            let _ = fs::remove_file(&part);
            return Err(e);
        }
        self.record_checkpoint(&document_id, id, reason, &path)
    }
    /// Checkpoints of the session's document, newest first.
    pub fn list_checkpoints(&self, window: &str) -> Result<Vec<CheckpointInfo>, AppError> {
        let document_id = self.with_session(window, |s| Ok(s.doc.metadata.document_id.clone()))?;
        Ok(self.checkpoints_of(&document_id))
    }
    /// Writes a checkpoint to `dest` as a separate document (new document id), leaving the open session and its archive untouched. Returns the written path.
    pub fn restore_checkpoint_as_copy(
        &self,
        window: &str,
        checkpoint_id: &str,
        dest: &Path,
    ) -> Result<String, AppError> {
        let info = self
            .list_checkpoints(window)?
            .into_iter()
            .find(|c| c.id == checkpoint_id)
            .ok_or_else(|| AppError::new("NOT_FOUND", "Checkpoint not found"))?;
        let cache = self
            .recovery_root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir);
        let work = cache.join(format!("restore-{}", Uuid::new_v4()));
        let result = (|| {
            let doc = archive_io::extract_to(Path::new(&info.path), &work)?;
            let now = Utc::now().to_rfc3339();
            let metadata = ArchiveMetadata {
                document_id: Uuid::new_v4().to_string(),
                created_at: now.clone(),
                updated_at: now,
                application_version: env!("CARGO_PKG_VERSION").into(),
            };
            let data = work.join("data.db");
            let assets: Vec<PathBuf> = doc
                .attachments
                .iter()
                .map(|a| archive_io::asset_content(&work, &a.id))
                .collect();
            archive_io::write(
                dest,
                WriteRequest {
                    metadata: &metadata,
                    config: &doc.config,
                    data: Payload::File(&data),
                    attachments: doc
                        .attachments
                        .iter()
                        .zip(&assets)
                        .map(|(a, p)| (a, Payload::File(p)))
                        .collect(),
                    // A copy keeps the checkpoint's unknown (newer-build) tables.
                    preserve_from: Some(Path::new(&info.path)),
                    preserve_copy: true,
                    reuse_from: None,
                },
            )?;
            Ok::<_, AppError>(())
        })();
        let _ = fs::remove_dir_all(&work);
        result?;
        let _ = self.global.add_recent(dest);
        logging::info(
            "checkpoint",
            &format!("restored checkpoint {checkpoint_id} as {}", dest.display()),
        );
        Ok(dest.to_string_lossy().into())
    }
}

pub fn create_checkpoint(
    window_label: String,
    reason: Option<String>,
) -> Result<CheckpointInfo, AppError> {
    // A checkpoint copies every table, so a role may not take one.
    crate::authz::require_unrestricted(&window_label, "create checkpoints")?;
    crate::manager()?.create_checkpoint(&window_label, reason.as_deref().unwrap_or("manual"))
}
pub fn list_checkpoints(window_label: String) -> Result<Vec<CheckpointInfo>, AppError> {
    crate::manager()?.list_checkpoints(&window_label)
}
pub fn restore_checkpoint_as_copy(
    window_label: String,
    checkpoint_id: String,
    path: String,
) -> Result<String, AppError> {
    crate::authz::require_unrestricted(&window_label, "restore checkpoints")?;
    crate::manager()?.restore_checkpoint_as_copy(&window_label, &checkpoint_id, Path::new(&path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restored_copies_keep_unknown_archive_tables() {
        let base = std::env::temp_dir().join(format!("ixtable-ckpt-{}", Uuid::new_v4()));
        let m = DocumentManager::new(base.join("data"), base.join("cache")).unwrap();
        m.new_session("w").unwrap();
        let info = m.create_checkpoint("w", "test").unwrap();
        // A newer build's table inside the checkpoint archive.
        rusqlite::Connection::open(&info.path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE future_notes(body TEXT); INSERT INTO future_notes VALUES('kept');",
            )
            .unwrap();
        let dest = base.join("copy.ixt");
        m.restore_checkpoint_as_copy("w", &info.id, &dest).unwrap();
        let header = crate::archive::read_header(&dest).unwrap();
        assert_ne!(header.metadata.document_id, info.document_id);
        let body: String = rusqlite::Connection::open(&dest)
            .unwrap()
            .query_row("SELECT body FROM future_notes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(body, "kept");
        m.close("w", true).unwrap();
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn checkpoint_files_never_leave_the_document_directory() {
        let base = std::env::temp_dir().join(format!("ixtable-ckpt-{}", Uuid::new_v4()));
        let m = DocumentManager::new(base.join("data"), base.join("cache")).unwrap();
        assert_eq!(
            m.checkpoint_dir("../../x").unwrap_err().code,
            "INVALID_ARCHIVE"
        );

        m.new_session("w").unwrap();
        let info = m.create_checkpoint("w", "test").unwrap();
        let dir = m.checkpoint_dir(&info.document_id).unwrap();
        let mut names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        // The sidecar is written atomically: no temp file is left behind.
        assert_eq!(
            names,
            vec![format!("{}.ixt", info.id), format!("{}.json", info.id)]
        );

        // Planted sidecars that point elsewhere are ignored, so they are never pruned or restored.
        let victim = base.join("victim.txt");
        fs::write(&victim, b"keep").unwrap();
        for (id, path) in [
            ("planted", victim.clone()),
            ("../../victim", victim.clone()),
            ("aaaa", dir.join("bbbb.ixt")),
        ] {
            let planted = CheckpointInfo {
                id: id.into(),
                path: path.to_string_lossy().into(),
                created_at: "0000".into(),
                ..info.clone()
            };
            fs::write(
                dir.join(format!("{}.json", Uuid::new_v4())),
                serde_json::to_vec(&planted).unwrap(),
            )
            .unwrap();
        }
        fs::copy(&info.path, dir.join("bbbb.ixt")).unwrap();
        assert_eq!(m.list_checkpoints("w").unwrap(), vec![info.clone()]);

        // Fill past the limit with real (older) checkpoints, then prune.
        for n in 0..KEEP_CHECKPOINTS {
            let id = Uuid::new_v4().to_string();
            let path = dir.join(format!("{id}.ixt"));
            fs::copy(&info.path, &path).unwrap();
            let old = CheckpointInfo {
                id,
                path: path.to_string_lossy().into(),
                created_at: format!("2000-01-01T00:00:{n:02}Z"),
                ..info.clone()
            };
            fs::write(
                path.with_extension("json"),
                serde_json::to_vec(&old).unwrap(),
            )
            .unwrap();
        }
        m.prune_checkpoints(&info.document_id);
        assert_eq!(m.list_checkpoints("w").unwrap().len(), KEEP_CHECKPOINTS);
        assert!(Path::new(&info.path).exists(), "the newest is kept");
        assert_eq!(fs::read(&victim).unwrap(), b"keep");
        assert!(
            dir.join("bbbb.ixt").exists(),
            "files of mismatched sidecars are left alone"
        );
        m.close("w", true).unwrap();
        fs::remove_dir_all(base).unwrap();
    }
}
