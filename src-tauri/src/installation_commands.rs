//! Tauri commands for opening, updating, and resetting runtime installations.
use crate::bundle::{self, io_err, SignedBundle};
use crate::installation::{
    self, apply_bundle, check_signer, installations_root, open_session, plan, read_installed,
    runtime_session, BundleSummary, InstalledBundle, ResetPreview,
};
use crate::installation_checks::{pending_migrations, PendingMigration};
use crate::manager::{AppError, DocumentManager, SessionState};
use std::{fs, path::Path};
use uuid::Uuid;

/// Largest bundle the Runtime will read (cloud limit 500 MB plus headroom);
/// bundles are verified in memory, so the size is checked before reading.
pub const MAX_BUNDLE_BYTES: u64 = 600 * 1024 * 1024;

fn check_bundle_size(len: u64) -> Result<(), AppError> {
    if len > MAX_BUNDLE_BYTES {
        return Err(AppError::new(
            "BUNDLE_TOO_LARGE",
            format!(
                "This bundle is {} MB; the Runtime opens bundles up to {} MB.",
                len / (1024 * 1024),
                MAX_BUNDLE_BYTES / (1024 * 1024)
            ),
        ));
    }
    Ok(())
}

/// Copies the chosen file into a private temporary location, then verifies it there.
#[cfg(test)]
fn receive(root: &Path, path: &str) -> Result<SignedBundle, AppError> {
    receive_checked(root, path, None).map(|(signed, _)| signed)
}

/// `receive`, also returning the file's sha256. With `expected_sha256` (from
/// `inspect_runtime_bundle`), a file that changed since inspection is refused.
fn receive_checked(
    root: &Path,
    path: &str,
    expected_sha256: Option<&str>,
) -> Result<(SignedBundle, String), AppError> {
    let missing = |e: std::io::Error| AppError::new("MISSING_FILE", format!("{path}: {e}"));
    check_bundle_size(fs::metadata(path).map_err(missing)?.len())?;
    let incoming = root.join(".incoming");
    fs::create_dir_all(&incoming).map_err(io_err)?;
    let tmp = bundle::ScratchFile(incoming.join(format!("{}.ixtr", Uuid::new_v4())));
    fs::copy(path, &tmp.0).map_err(missing)?;
    // The source may have grown between the check and the copy.
    check_bundle_size(fs::metadata(&tmp.0).map_err(io_err)?.len())?;
    let bytes = fs::read(&tmp.0).map_err(io_err)?;
    drop(tmp);
    let sha256 = bundle::sha256_hex(&bytes);
    if expected_sha256.is_some_and(|e| !e.eq_ignore_ascii_case(&sha256)) {
        return Err(AppError::new(
            "BUNDLE_CHANGED",
            "The bundle file changed after it was inspected. Open it again.",
        ));
    }
    Ok((bundle::verify(&bytes)?, sha256))
}

/// Verifies signature, signer pin, and Runtime compatibility, then decrypts.
fn admit(
    root: &Path,
    path: &str,
    password: Option<&str>,
    expected_sha256: Option<&str>,
) -> Result<(SignedBundle, Vec<u8>), AppError> {
    let (signed, _) = receive_checked(root, path, expected_sha256)?;
    check_signer(root, &signed)?;
    bundle::check_runtime_compat(&signed.header, bundle::APP_VERSION)?;
    let archive = signed.archive_bytes(password)?;
    Ok((signed, archive))
}

/// Closes the window's runtime session so its files can be swapped; returns its dir.
fn release(
    m: &DocumentManager,
    window: &str,
    bundle_id: &str,
) -> Result<Option<std::path::PathBuf>, AppError> {
    let Ok(state) = m.state(window) else {
        return Ok(None);
    };
    match runtime_session(&state.session_id) {
        Some(rt) if rt.bundle_id == bundle_id => {
            m.close(window, true)?;
            Ok(Some(rt.dir))
        }
        _ => Ok(None),
    }
}

fn install_and_open(
    window: &str,
    signed: &SignedBundle,
    archive: &[u8],
) -> Result<SessionState, AppError> {
    let m = crate::manager()?;
    let root = installations_root();
    let reopen = release(m, window, &signed.header.bundle_id)?;
    match apply_bundle(&root, signed, archive) {
        Ok((dir, _)) => open_session(m, window, &dir),
        Err(e) => {
            crate::logging::warn("bundle", &format!("{}: {}", e.code, e.message));
            if let Some(dir) = reopen {
                open_session(m, window, &dir)?;
            }
            Err(e)
        }
    }
}

/// Reads a bundle's verified header and what opening it would do (no decryption).
pub fn inspect_runtime_bundle(
    window_label: String,
    path: String,
) -> Result<BundleSummary, AppError> {
    let _ = window_label;
    inspect(&installations_root(), &path)
}

fn inspect(root: &Path, path: &str) -> Result<BundleSummary, AppError> {
    let (signed, sha256) = receive_checked(root, path, None)?;
    let installed = check_signer(root, &signed)?;
    bundle::check_runtime_compat(&signed.header, bundle::APP_VERSION)?;
    let action = plan(&signed, installed.as_ref())?;
    // Best effort: a preview failure never blocks opening; the UI shows why.
    let (pending_migrations, migrations_unavailable) = match action {
        installation::Action::Update
            if !signed.header.flags.encrypted =>
        {
            match signed
                .archive_bytes(None)
                .and_then(|archive| pending_for(root, &signed, &archive))
            {
                Ok(list) => (Some(list), None),
                Err(e) => (None, Some(e.message)),
            }
        }
        _ => (None, None),
    };
    let h = &signed.header;
    Ok(BundleSummary {
        bundle_id: h.bundle_id.clone(),
        name: h.name.clone(),
        version: h.version.clone(),
        release_notes: h.release_notes.clone(),
        min_runtime_version: h.min_runtime_version.clone(),
        encrypted: h.flags.encrypted,
        signer_fingerprint: signed.signer_fingerprint(),
        installed_version: installed.map(|i| i.version),
        action: format!("{action:?}").to_lowercase(),
        pending_migrations,
        migrations_unavailable,
        sha256,
    })
}

/// Migrations that applying `archive` would run on the installed records.
fn pending_for(
    root: &Path,
    signed: &SignedBundle,
    archive: &[u8],
) -> Result<Vec<PendingMigration>, AppError> {
    let doc = bundle::read_archive_bytes(archive, &root.join(".tmp"))?;
    let dir = installation::installation_dir(root, &signed.header.bundle_id)?;
    pending_migrations(&dir.join("data.db"), &doc.config)
        .map_err(|e| AppError::new("INSTALLATION_CORRUPT", e))
}

/// Pending migrations for a (possibly password-protected) update, shown with
/// the release notes before `update_runtime_installation` applies it.
pub fn preview_runtime_update(
    window_label: String,
    path: String,
    password: Option<String>,
) -> Result<Vec<PendingMigration>, AppError> {
    let _ = window_label;
    let root = installations_root();
    let (signed, archive) = admit(&root, &path, password.as_deref(), None)?;
    pending_for(&root, &signed, &archive)
}

/// Verifies and opens a runtime bundle: installs it on first open, updates the
/// installation when the bundle is newer, and opens a runtime-only session.
pub fn open_runtime_bundle(
    window_label: String,
    path: String,
    password: Option<String>,
    expected_sha256: Option<String>,
) -> Result<SessionState, AppError> {
    let (signed, archive) = admit(
        &installations_root(),
        &path,
        password.as_deref(),
        expected_sha256.as_deref(),
    )?;
    install_and_open(&window_label, &signed, &archive)
}

/// Manual update of an installed bundle ("Check for update…"). The version must be newer;
/// installation records are kept and migrated.
/// `expected_sha256` is the hash `inspect_runtime_bundle` returned for the file.
pub fn update_runtime_installation(
    window_label: String,
    path: String,
    password: Option<String>,
    expected_sha256: String,
) -> Result<SessionState, AppError> {
    let root = installations_root();
    let (signed, archive) = admit(&root, &path, password.as_deref(), Some(&expected_sha256))?;
    let m = crate::manager()?;
    if let Some(rt) = m
        .state(&window_label)
        .ok()
        .and_then(|s| runtime_session(&s.session_id))
    {
        if rt.bundle_id != signed.header.bundle_id {
            return Err(AppError::new(
                "BUNDLE_MISMATCH",
                format!(
                    "This file is a different application ({}).",
                    signed.header.name
                ),
            ));
        }
    }
    let installed = check_signer(&root, &signed)?.ok_or_else(|| {
        AppError::new(
            "BUNDLE_NOT_INSTALLED",
            "Open the bundle once before updating it",
        )
    })?;
    if plan(&signed, Some(&installed))? == installation::Action::Open {
        return Err(AppError::new(
            "BUNDLE_SAME_VERSION",
            format!("Version {} is already installed.", installed.version),
        ));
    }
    install_and_open(&window_label, &signed, &archive)
}

fn current_runtime(window: &str) -> Result<installation::RuntimeSession, AppError> {
    let state = crate::manager()?.state(window)?;
    runtime_session(&state.session_id)
        .ok_or_else(|| AppError::new("NOT_RUNTIME", "No runtime bundle is open in this window"))
}

/// Installed-bundle details for the runtime window header.
pub fn runtime_installation_info(window_label: String) -> Result<InstalledBundle, AppError> {
    let rt = current_runtime(&window_label)?;
    read_installed(&rt.dir).ok_or_else(|| AppError::new("NOT_RUNTIME", "Installation is missing"))
}

/// Impact preview for `reset_runtime_installation_data`.
pub fn preview_installation_reset(window_label: String) -> Result<ResetPreview, AppError> {
    let rt = current_runtime(&window_label)?;
    installation::reset_preview(&rt.dir, &installations_root().join(".tmp"))
}

/// Destructive: replaces installation records with the bundle's bootstrap data after a
/// recovery checkpoint. Requires `confirmed: true`.
pub fn reset_runtime_installation_data(
    window_label: String,
    confirmed: bool,
) -> Result<SessionState, AppError> {
    if !confirmed {
        return Err(AppError::new(
            "CONFIRMATION_REQUIRED",
            "Resetting installation data needs explicit confirmation",
        ));
    }
    crate::authz::require_unrestricted(&window_label, "reset installation data")?;
    let rt = current_runtime(&window_label)?;
    let m = crate::manager()?;
    m.close(&window_label, true)?;
    let result = installation::reset_data(&rt.dir);
    // Reapply the installation's role (a cloud install would otherwise deny all).
    let state = crate::cloud::install::open_with_role(m, &window_label, &rt.dir)?;
    result.map(|_| state)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_bundles_are_rejected_before_reading() {
        let root = std::env::temp_dir().join(format!("ixtable-recv-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let big = root.join("big.ixtr");
        // Sparse: no disk space or memory is used.
        fs::File::create(&big)
            .unwrap()
            .set_len(MAX_BUNDLE_BYTES + 1)
            .unwrap();
        let err = receive(&root, big.to_str().unwrap()).unwrap_err();
        assert_eq!(err.code, "BUNDLE_TOO_LARGE");
        assert!(err.message.contains("600 MB"), "{}", err.message);
        // Never copied into the incoming area.
        assert!(!root.join(".incoming").exists());
        let small = root.join("small.ixtr");
        fs::write(&small, b"not a bundle").unwrap();
        assert_eq!(
            receive(&root, small.to_str().unwrap()).unwrap_err().code,
            "BUNDLE_SIGNATURE"
        );
        assert_eq!(fs::read_dir(root.join(".incoming")).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    struct Setup {
        root: std::path::PathBuf,
        key: ed25519_dalek::SigningKey,
        doc: crate::archive::ArchiveDocument,
    }

    impl Setup {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("ixtable-inspect-{}", Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            Self {
                root,
                key: ed25519_dalek::SigningKey::generate(&mut rand_core::OsRng),
                doc: crate::archive::create_document("CRM").unwrap(),
            }
        }
        fn write(&self, version: &str, min_runtime: Option<&str>) -> String {
            let archive = bundle::archive_bytes(&self.doc, &self.root.join(".tmp")).unwrap();
            let meta = bundle::BundleMeta {
                bundle_id: self.doc.metadata.document_id.clone(),
                name: "CRM".into(),
                version: version.into(),
                min_runtime_version: min_runtime.map(Into::into),
                ..Default::default()
            };
            let bytes = bundle::build_bundle(&archive, &meta, None, &self.key).unwrap();
            let path = self.root.join(format!("{version}-{}.ixtr", Uuid::new_v4()));
            fs::write(&path, bytes).unwrap();
            path.to_str().unwrap().to_string()
        }
        fn install(&self, version: &str) {
            let signed = receive(&self.root, &self.write(version, None)).unwrap();
            let archive = signed.archive_bytes(None).unwrap();
            apply_bundle(&self.root, &signed, &archive).unwrap();
        }
    }

    impl Drop for Setup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn inspect_reports_incompatible_runtime_before_reading_the_archive() {
        let s = Setup::new();
        s.install("1.0.0");
        let err = inspect(&s.root, &s.write("2.0.0", Some("999.0.0"))).unwrap_err();
        assert_eq!(err.code, "BUNDLE_INCOMPATIBLE");
        assert!(err.message.contains("999.0.0"), "{}", err.message);
    }

    #[test]
    fn inspect_survives_an_unreadable_migration_preview() {
        let s = Setup::new();
        s.install("1.0.0");
        let dir = installation::installation_dir(&s.root, &s.doc.metadata.document_id).unwrap();
        fs::write(dir.join("data.db"), b"not a database").unwrap();
        let summary = inspect(&s.root, &s.write("2.0.0", None)).unwrap();
        assert_eq!(summary.action, "update");
        assert!(summary.pending_migrations.is_none());
        assert!(summary.migrations_unavailable.is_some());
        let ok = inspect(&s.root, &s.write("1.0.0", None)).unwrap();
        assert!(ok.migrations_unavailable.is_none());
    }

    #[test]
    fn apply_refuses_a_file_changed_after_inspection() {
        let s = Setup::new();
        let path = s.write("1.0.0", None);
        let summary = inspect(&s.root, &path).unwrap();
        assert_eq!(
            summary.sha256,
            bundle::sha256_hex(&fs::read(&path).unwrap())
        );
        assert!(admit(&s.root, &path, None, Some(&summary.sha256)).is_ok());
        fs::copy(s.write("1.0.1", None), &path).unwrap();
        let err = admit(&s.root, &path, None, Some(&summary.sha256)).unwrap_err();
        assert_eq!(err.code, "BUNDLE_CHANGED");
        assert!(err.message.contains("changed after it was inspected"));
    }
}
