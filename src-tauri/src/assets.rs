//! Application assets (PRD §18): streaming import/export, checksum validation,
//! content-hash deduplication, safe filenames, orphan cleanup, and archive size
//! reports against the cloud limit (PRD §7.4).
//!
//! While a session is open, each asset lives uncompressed in its workspace at
//! `attachments/<id>/content` (+ `metadata.json`); saving streams it into the archive.
use crate::{
    archive::Attachment,
    archive_io::{self, HashingReader, HashingWriter, SizeEntry},
    logging,
    manager::{read_only_guard, AppError, DocumentManager, SessionState},
};
use chrono::Utc;
use serde::Serialize;
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{self, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

/// MVP cloud-synchronized archive limit: 500 MB (decimal megabytes).
pub const CLOUD_LIMIT_BYTES: u64 = 500_000_000;
const LARGEST_ENTRIES: usize = 10;
const MAX_NAME_BYTES: usize = 200;
const WINDOWS_RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetImport {
    pub asset: Attachment,
    /// True when identical content already existed and its asset id was reused.
    pub deduplicated: bool,
    pub state: SessionState,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanCleanup {
    pub removed: Vec<Attachment>,
    pub state: SessionState,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveSizeReport {
    pub total_bytes: u64,
    pub records_bytes: u64,
    pub config_bytes: u64,
    pub assets_bytes: u64,
    /// SQLite page overhead, indexes, and preserved unknown tables.
    pub other_bytes: u64,
    /// Fraction (0–1) of the archive taken by assets.
    pub asset_share: f64,
    pub largest: Vec<SizeEntry>,
    pub cloud_limit_bytes: u64,
    pub over_cloud_limit: bool,
    /// `saved` (measured on the saved archive) or `snapshot` (packed from unsaved WIP).
    pub measured: String,
}

/// Builds a size report from stored entry sizes and the archive file's total size.
pub fn size_report(total: u64, mut entries: Vec<SizeEntry>, measured: &str) -> ArchiveSizeReport {
    let sum = |section: &str| -> u64 {
        entries
            .iter()
            .filter(|e| e.section == section)
            .map(|e| e.bytes)
            .sum()
    };
    let (records, config, assets) = (sum("records"), sum("config"), sum("assets"));
    entries.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    entries.truncate(LARGEST_ENTRIES);
    ArchiveSizeReport {
        total_bytes: total,
        records_bytes: records,
        config_bytes: config,
        assets_bytes: assets,
        other_bytes: total.saturating_sub(records + config + assets),
        asset_share: if total == 0 {
            0.0
        } else {
            (assets as f64 / total as f64).min(1.0)
        },
        largest: entries,
        cloud_limit_bytes: CLOUD_LIMIT_BYTES,
        over_cloud_limit: total > CLOUD_LIMIT_BYTES,
        measured: measured.into(),
    }
}

/// A display/export-safe file name: last path component only, no control or
/// reserved characters, no Windows device names, bounded length.
pub fn safe_file_name(raw: &str) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = last
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        .collect();
    let mut name = cleaned
        .trim()
        .trim_end_matches(['.', ' '])
        .trim_start_matches('.')
        .to_string();
    if name.is_empty() {
        name = "asset".into();
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_uppercase();
    if WINDOWS_RESERVED.contains(&stem.as_str()) {
        name = format!("_{name}");
    }
    if name.len() > MAX_NAME_BYTES {
        let ext = name
            .rfind('.')
            .map(|i| name[i..].to_string())
            .filter(|e| e.len() <= 16)
            .unwrap_or_default();
        let mut cut = MAX_NAME_BYTES - ext.len();
        while !name.is_char_boundary(cut) {
            cut -= 1;
        }
        name = format!("{}{ext}", &name[..cut]);
    }
    name
}

/// MIME type from a file extension; `application/octet-stream` when unknown.
pub fn guess_media_type(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("pdf") => "application/pdf",
        Some("json") => "application/json",
        Some("csv") => "text/csv",
        Some("txt" | "md") => "text/plain",
        Some("html" | "htm") => "text/html",
        Some("css") => "text/css",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
}

fn collect_strings<'a>(value: &'a serde_json::Value, out: &mut Vec<&'a str>) {
    match value {
        serde_json::Value::String(s) => out.push(s),
        serde_json::Value::Array(items) => items.iter().for_each(|v| collect_strings(v, out)),
        serde_json::Value::Object(map) => map.values().for_each(|v| collect_strings(v, out)),
        _ => {}
    }
}

/// Asset ids not referenced by any string in the config JSON. A string references an
/// asset when it equals or contains the asset id (e.g. `asset://<id>`).
pub fn orphan_ids(config: &serde_json::Value, ids: &[String]) -> Vec<String> {
    let mut strings = Vec::new();
    collect_strings(config, &mut strings);
    let exact: HashSet<&str> = strings.iter().copied().collect();
    ids.iter()
        .filter(|id| {
            !exact.contains(id.as_str()) && !strings.iter().any(|s| s.contains(id.as_str()))
        })
        .cloned()
        .collect()
}

/// The stored asset with identical content (same SHA-256 and size), if any.
pub fn find_duplicate<'a>(
    assets: &'a [Attachment],
    checksum: &str,
    size: u64,
) -> Option<&'a Attachment> {
    assets
        .iter()
        .find(|a| a.size == size && a.checksum == checksum)
}

fn asset_error(e: impl ToString) -> AppError {
    AppError::new("ATTACHMENT_FAILURE", e)
}

/// Streams `src` to `dest` while hashing; returns (sha256, bytes).
fn copy_hashed(src: &Path, dest: &Path) -> io::Result<(String, u64)> {
    let mut reader = HashingReader::new(BufReader::new(File::open(src)?));
    let mut out = BufWriter::new(File::create(dest)?);
    io::copy(&mut reader, &mut out)?;
    out.flush()?;
    Ok((reader.hex(), reader.bytes))
}

impl DocumentManager {
    /// Streams a file into the session workspace. Identical content (same SHA-256 and size) reuses the existing asset id instead of storing a second copy.
    pub fn import_asset(
        &self,
        window: &str,
        src: &Path,
        media_type: Option<&str>,
    ) -> Result<AssetImport, AppError> {
        let workspace = self.with_session(window, |s| {
            read_only_guard(s)?;
            Ok(s.workspace.clone())
        })?;
        let root = workspace.join("attachments");
        fs::create_dir_all(&root).map_err(asset_error)?;
        let staging = root.join(format!(".import-{}", Uuid::new_v4()));
        let (checksum, size) = copy_hashed(src, &staging).map_err(|e| {
            let _ = fs::remove_file(&staging);
            asset_error(format!("{}: {e}", src.display()))
        })?;
        let name = safe_file_name(&src.file_name().unwrap_or_default().to_string_lossy());
        let media = media_type
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| guess_media_type(&name))
            .to_string();
        let result = self.with_session(window, |s| {
            if s.workspace != workspace {
                return Err(AppError::new(
                    "NO_DOCUMENT",
                    "The document changed during import",
                ));
            }
            if let Some(existing) = find_duplicate(&s.doc.attachments, &checksum, size).cloned() {
                let _ = fs::remove_file(&staging);
                logging::info(
                    "assets",
                    &format!("import of {name} reused identical asset {}", existing.id),
                );
                return Ok(AssetImport {
                    asset: existing,
                    deduplicated: true,
                    state: s.state(),
                });
            }
            let now = Utc::now().to_rfc3339();
            let asset = Attachment {
                id: Uuid::now_v7().to_string(),
                display_name: name.clone(),
                media_type: media.clone(),
                checksum: checksum.clone(),
                size,
                created_at: now.clone(),
                updated_at: now,
                contents: vec![],
            };
            fs::create_dir_all(archive_io::asset_dir(&workspace, &asset.id))
                .map_err(asset_error)?;
            fs::rename(&staging, archive_io::asset_content(&workspace, &asset.id))
                .map_err(asset_error)?;
            archive_io::write_asset_metadata(&workspace, &asset)?;
            s.doc.attachments.push(asset.clone());
            self.touch(s);
            logging::info(
                "assets",
                &format!("imported {name} as {} ({size} bytes)", asset.id),
            );
            Ok(AssetImport {
                asset,
                deduplicated: false,
                state: s.state(),
            })
        });
        if result.is_err() {
            let _ = fs::remove_file(&staging);
        }
        result
    }
    /// Compatibility wrapper used by the `import_attachment` command.
    pub fn import_attachment(
        &self,
        window: &str,
        path: &Path,
        media: &str,
    ) -> Result<SessionState, AppError> {
        Ok(self.import_asset(window, path, Some(media))?.state)
    }
    /// Streams an asset to `dest`, verifying its checksum; `dest` only appears when valid.
    pub fn export_attachment(&self, window: &str, id: &str, dest: &Path) -> Result<(), AppError> {
        let (asset, src) = self.with_session(window, |s| {
            let a = s
                .doc
                .attachments
                .iter()
                .find(|a| a.id == id)
                .cloned()
                .ok_or_else(|| asset_error("Attachment not found"))?;
            Ok((a, archive_io::asset_content(&s.workspace, id)))
        })?;
        let parent = dest.parent().unwrap_or_else(|| Path::new("."));
        let part = parent.join(format!(
            ".{}.{}.part",
            safe_file_name(&asset.display_name),
            Uuid::new_v4()
        ));
        let outcome = (|| {
            let mut reader = BufReader::new(File::open(&src)?);
            let mut out = HashingWriter::new(BufWriter::new(File::create(&part)?));
            io::copy(&mut reader, &mut out)?;
            out.flush()?;
            if out.bytes != asset.size || out.hex() != asset.checksum {
                return Err(io::Error::other(format!(
                    "asset {} failed checksum validation",
                    asset.display_name
                )));
            }
            drop(out.into_inner());
            fs::rename(&part, dest)
        })();
        if let Err(e) = outcome {
            let _ = fs::remove_file(&part);
            logging::warn("assets", &format!("export of {id} failed: {e}"));
            return Err(asset_error(e));
        }
        Ok(())
    }
    pub fn remove_attachment(&self, window: &str, id: &str) -> Result<SessionState, AppError> {
        self.with_session(window, |s| {
            read_only_guard(s)?;
            let before = s.doc.attachments.len();
            s.doc.attachments.retain(|a| a.id != id);
            if s.doc.attachments.len() == before {
                return Err(asset_error("Attachment not found"));
            }
            let _ = fs::remove_dir_all(archive_io::asset_dir(&s.workspace, id));
            self.touch(s);
            logging::info("assets", &format!("removed asset {id}"));
            Ok(s.state())
        })
    }
    /// Asset metadata only (`contents` empty); cheap to list.
    pub fn asset_list(&self, window: &str) -> Result<Vec<Attachment>, AppError> {
        self.with_session(window, |s| Ok(s.doc.attachments.clone()))
    }
    /// Path of an asset's uncompressed content in the session workspace (for streaming).
    pub fn asset_path(&self, window: &str, id: &str) -> Result<PathBuf, AppError> {
        self.with_session(window, |s| {
            s.doc
                .attachments
                .iter()
                .any(|a| a.id == id)
                .then(|| archive_io::asset_content(&s.workspace, id))
                .ok_or_else(|| asset_error("Attachment not found"))
        })
    }
    /// Assets with `contents` loaded into memory. Prefer [`Self::asset_list`] plus [`Self::asset_path`] for large assets.
    pub fn attachments(&self, window: &str) -> Result<Vec<Attachment>, AppError> {
        let (workspace, mut list) = self.with_session(window, |s| {
            Ok((s.workspace.clone(), s.doc.attachments.clone()))
        })?;
        for a in &mut list {
            a.contents =
                fs::read(archive_io::asset_content(&workspace, &a.id)).map_err(asset_error)?;
        }
        Ok(list)
    }
    pub fn orphan_assets(&self, window: &str) -> Result<Vec<Attachment>, AppError> {
        self.with_session(window, |s| {
            let config = serde_json::to_value(&s.doc.config).map_err(asset_error)?;
            let ids: Vec<String> = s.doc.attachments.iter().map(|a| a.id.clone()).collect();
            let orphans = orphan_ids(&config, &ids);
            Ok(s.doc
                .attachments
                .iter()
                .filter(|a| orphans.contains(&a.id))
                .cloned()
                .collect())
        })
    }
    pub fn cleanup_orphan_assets(&self, window: &str) -> Result<OrphanCleanup, AppError> {
        let removed = self.orphan_assets(window)?;
        let mut state = self.state(window)?;
        for a in &removed {
            state = self.remove_attachment(window, &a.id)?;
        }
        Ok(OrphanCleanup { removed, state })
    }
    /// Archive size by section and largest entries. Measures the saved archive when the session is clean; otherwise packs a temporary snapshot of the WIP.
    pub fn archive_size_report(&self, window: &str) -> Result<ArchiveSizeReport, AppError> {
        let (saved, snap) = self.with_session(window, |s| {
            let saved = s.path.clone().filter(|p| !s.dirty && p.exists());
            Ok((saved, self.snapshot(s)))
        })?;
        if let Some(path) = saved {
            let (total, entries) = archive_io::size_entries(&path)?;
            return Ok(size_report(total, entries, "saved"));
        }
        let cache = self
            .recovery_root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir);
        let tmp: PathBuf = cache.join(format!("size-report-{}.ixt", Uuid::new_v4()));
        let result = self
            .write_snapshot(&snap, &tmp)
            .and_then(|_| Ok(archive_io::size_entries(&tmp)?));
        let _ = fs::remove_file(&tmp);
        let (total, entries) = result?;
        Ok(size_report(total, entries, "snapshot"))
    }
}

/// Imports a file as an application asset; reports whether identical content was reused.
pub fn import_asset(
    window_label: String,
    path: String,
    media_type: Option<String>,
) -> Result<AssetImport, AppError> {
    crate::manager()?.import_asset(&window_label, Path::new(&path), media_type.as_deref())
}
pub fn list_orphan_assets(window_label: String) -> Result<Vec<Attachment>, AppError> {
    crate::manager()?.orphan_assets(&window_label)
}
pub fn cleanup_orphan_assets(window_label: String) -> Result<OrphanCleanup, AppError> {
    crate::manager()?.cleanup_orphan_assets(&window_label)
}
pub fn archive_size_report(window_label: String) -> Result<ArchiveSizeReport, AppError> {
    crate::manager()?.archive_size_report(&window_label)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(section: &str, name: &str, bytes: u64) -> SizeEntry {
        SizeEntry {
            section: section.into(),
            id: name.into(),
            name: name.into(),
            bytes,
        }
    }

    #[test]
    fn size_report_flags_only_archives_over_the_cloud_limit() {
        let at_limit = size_report(
            CLOUD_LIMIT_BYTES,
            vec![
                entry("records", "data", 100_000_000),
                entry("assets", "video.mp4", 390_000_000),
                entry("config", "config", 1_000),
            ],
            "saved",
        );
        assert!(!at_limit.over_cloud_limit);
        assert_eq!(at_limit.assets_bytes, 390_000_000);
        assert_eq!(at_limit.other_bytes, 9_999_000);
        assert!((at_limit.asset_share - 0.78).abs() < 1e-9);
        let over = size_report(CLOUD_LIMIT_BYTES + 1, vec![], "snapshot");
        assert!(over.over_cloud_limit);
        assert_eq!(over.cloud_limit_bytes, 500_000_000);
    }

    #[test]
    fn size_report_lists_the_ten_largest_entries() {
        let entries = (1..=12)
            .map(|n| entry("assets", &format!("a{n:02}"), n * 10))
            .collect();
        let report = size_report(10_000, entries, "saved");
        assert_eq!(report.largest.len(), 10);
        assert_eq!(report.largest[0].name, "a12");
        assert_eq!(report.largest[9].name, "a03");
        assert_eq!(report.assets_bytes, 780);
        assert_eq!(size_report(0, vec![], "saved").asset_share, 0.0);
    }

    #[test]
    fn safe_file_names_strip_paths_controls_and_device_names() {
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name(r"C:\Users\me\logo.png"), "logo.png");
        assert_eq!(safe_file_name("bad\u{0}na\nme?.txt"), "badname.txt");
        assert_eq!(safe_file_name("CON.txt"), "_CON.txt");
        assert_eq!(safe_file_name("lpt1"), "_lpt1");
        assert_eq!(safe_file_name("..."), "asset");
        assert_eq!(safe_file_name("report. "), "report");
        assert_eq!(safe_file_name(".hidden"), "hidden");
        let long = format!("{}.pdf", "é".repeat(300));
        let safe = safe_file_name(&long);
        assert!(safe.len() <= MAX_NAME_BYTES && safe.ends_with(".pdf"));
        assert_eq!(guess_media_type("logo.PNG"), "image/png");
        assert_eq!(guess_media_type("blob"), "application/octet-stream");
    }

    #[test]
    fn identical_content_is_found_for_deduplication() {
        let mut doc = crate::archive::create_document("Dedup").unwrap();
        crate::archive::add_attachment(
            &mut doc,
            "a.png".into(),
            "image/png".into(),
            b"same".to_vec(),
        );
        crate::archive::add_attachment(
            &mut doc,
            "b.png".into(),
            "image/png".into(),
            b"other".to_vec(),
        );
        let sum = archive_io::sha256_hex(b"same");
        let found = find_duplicate(&doc.attachments, &sum, 4).unwrap();
        assert_eq!(found.display_name, "a.png");
        assert!(find_duplicate(&doc.attachments, &sum, 5).is_none());
        assert!(find_duplicate(&doc.attachments, &archive_io::sha256_hex(b"new"), 3).is_none());
    }

    #[test]
    fn orphans_are_assets_no_config_string_mentions() {
        let config = serde_json::json!({
            "design": {"forms": [{"logo": "a1", "nested": [{"src": "asset://a2"}]}]},
            "name": "a3-not-really"
        });
        let ids = vec!["a1".into(), "a2".into(), "a4".into()];
        assert_eq!(orphan_ids(&config, &ids), vec!["a4".to_string()]);
    }
}
