use super::*;
use crate::archive::ArchiveDocument;
use crate::bundle::{archive_bytes, build_bundle, verify, BundleMeta};
use ed25519_dalek::SigningKey;
use rand_core::OsRng;
use rusqlite::Connection;

fn temp(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ixtable-{label}-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn seeded_data(rows: &[&str]) -> Vec<u8> {
    let dir = temp("seed");
    let path = dir.join("data.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE Customers(id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
        .unwrap();
    for name in rows {
        conn.execute("INSERT INTO Customers(name) VALUES(?1)", [name])
            .unwrap();
    }
    drop(conn);
    let bytes = fs::read(&path).unwrap();
    fs::remove_dir_all(dir).unwrap();
    bytes
}

struct Fixture {
    root: PathBuf,
    key: SigningKey,
    doc: ArchiveDocument,
}

impl Fixture {
    fn new() -> Self {
        let mut doc = archive::create_document("CRM").unwrap();
        doc.data = seeded_data(&["Ada", "Grace"]);
        Self {
            root: temp("installations"),
            key: SigningKey::generate(&mut OsRng),
            doc,
        }
    }
    fn bundle_with(&self, version: &str, key: &SigningKey) -> (SignedBundle, Vec<u8>) {
        let archive = archive_bytes(&self.doc, &self.root.join(".tmp")).unwrap();
        let meta = BundleMeta {
            bundle_id: self.doc.metadata.document_id.clone(),
            name: "CRM".into(),
            version: version.into(),
            ..Default::default()
        };
        let signed = verify(&build_bundle(&archive, &meta, None, key).unwrap()).unwrap();
        let bytes = signed.archive_bytes(None).unwrap();
        (signed, bytes)
    }
    fn bundle(&self, version: &str) -> (SignedBundle, Vec<u8>) {
        self.bundle_with(version, &self.key)
    }
    fn apply(&self, version: &str) -> Result<(PathBuf, Action), AppError> {
        let (signed, archive) = self.bundle(version);
        apply_bundle(&self.root, &signed, &archive)
    }
    fn dir(&self) -> PathBuf {
        self.root.join(&self.doc.metadata.document_id)
    }
    fn names(&self) -> Vec<String> {
        let conn = Connection::open(self.dir().join("data.db")).unwrap();
        let mut stmt = conn
            .prepare("SELECT name FROM Customers ORDER BY id")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }
    fn insert(&self, name: &str) {
        Connection::open(self.dir().join("data.db"))
            .unwrap()
            .execute("INSERT INTO Customers(name) VALUES(?1)", [name])
            .unwrap();
    }
    fn installed_version(&self) -> String {
        read_installed(&self.dir()).unwrap().version
    }
    fn add_migration(&mut self, id: &str, up: &str) {
        let migration = serde_json::from_value(serde_json::json!({
            "id": id, "name": id, "order": self.doc.config.migrations.len() + 1,
            "targetStore": "sqlite", "up": up, "down": null, "reversible": false, "dependsOn": []
        }))
        .unwrap();
        self.doc.config.migrations.push(migration);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn first_open_initializes_data_and_pins_signer() {
    let f = Fixture::new();
    let (dir, action) = f.apply("1.0.0").unwrap();
    assert_eq!(action, Action::Install);
    assert!(dir.join("active").join(ARCHIVE).is_file());
    assert_eq!(f.names(), ["Ada", "Grace"]);
    let info = read_installed(&dir).unwrap();
    assert_eq!(info.version, "1.0.0");
    assert_eq!(
        info.signer_public_key,
        f.bundle("1.0.0").0.header.signer_public_key
    );
    // Re-opening the same version keeps installation records untouched.
    f.insert("Linus");
    assert_eq!(f.apply("1.0.0").unwrap().1, Action::Open);
    assert_eq!(f.names(), ["Ada", "Grace", "Linus"]);
}

#[test]
fn update_preserves_installation_records_and_keeps_checkpoint() {
    let f = Fixture::new();
    f.apply("1.0.0").unwrap();
    f.insert("Runtime record");
    let (_, action) = f.apply("1.1.0").unwrap();
    assert_eq!(action, Action::Update);
    assert_eq!(f.names(), ["Ada", "Grace", "Runtime record"]);
    assert_eq!(f.installed_version(), "1.1.0");
    let previous = read_installed(&f.dir().join("previous")).unwrap();
    assert_eq!(previous.version, "1.0.0");
    assert_eq!(
        read_installed(&f.dir())
            .unwrap()
            .previous_version
            .as_deref(),
        Some("1.0.0")
    );
}

#[test]
fn older_version_is_refused() {
    let f = Fixture::new();
    f.apply("2.0.0").unwrap();
    let err = f.apply("1.9.0").unwrap_err();
    assert_eq!(err.code, "BUNDLE_DOWNGRADE");
    assert!(err.message == "Installed: 2.0.0. This file: 1.9.0.", "{}", err.message);
    assert_eq!(f.installed_version(), "2.0.0");
}

#[test]
fn different_signer_is_rejected() {
    let f = Fixture::new();
    f.apply("1.0.0").unwrap();
    let other = SigningKey::generate(&mut OsRng);
    let (signed, archive) = f.bundle_with("1.1.0", &other);
    let err = apply_bundle(&f.root, &signed, &archive).unwrap_err();
    assert_eq!(err.code, "BUNDLE_SIGNER_MISMATCH");
    assert_eq!(f.installed_version(), "1.0.0");
}

#[test]
fn migrations_apply_to_installation_data_on_update() {
    let mut f = Fixture::new();
    f.apply("1.0.0").unwrap();
    f.insert("Kept");
    f.add_migration("m-email", "ALTER TABLE Customers ADD COLUMN email TEXT");
    f.apply("1.1.0").unwrap();
    assert_eq!(f.names(), ["Ada", "Grace", "Kept"]);
    let conn = Connection::open(f.dir().join("data.db")).unwrap();
    let applied: i64 = conn
        .query_row(
            "SELECT count(*) FROM pragma_table_info('Customers') WHERE name='email'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(applied, 1);
    // The installation record names what the update ran (post-update notice).
    assert_eq!(
        read_installed(&f.dir()).unwrap().applied_migrations,
        ["m-email"]
    );
}

#[test]
fn applied_migrations_record_only_the_latest_apply() {
    let mut f = Fixture::new();
    f.add_migration("m-email", "ALTER TABLE Customers ADD COLUMN email TEXT");
    f.apply("1.0.0").unwrap();
    let fresh = read_installed(&f.dir()).unwrap();
    assert_eq!(fresh.last_action.as_deref(), Some("install"));
    assert!(fresh.applied_migrations.is_empty());
    f.add_migration("m-phone", "ALTER TABLE Customers ADD COLUMN phone TEXT");
    f.apply("1.1.0").unwrap();
    let updated = read_installed(&f.dir()).unwrap();
    assert_eq!(updated.last_action.as_deref(), Some("update"));
    assert_eq!(updated.applied_migrations, ["m-phone"]);
    assert_eq!(f.apply("1.1.0").unwrap().1, Action::Open);
    let reopened = read_installed(&f.dir()).unwrap();
    assert_eq!(reopened.last_action.as_deref(), Some("open"));
    assert!(reopened.applied_migrations.is_empty());
    assert_eq!(reopened.version, "1.1.0");
}

#[test]
fn pending_migrations_preview_what_an_update_will_run_without_writing() {
    use crate::installation_checks::pending_migrations;
    let mut f = Fixture::new();
    f.apply("1.0.0").unwrap();
    f.add_migration("m-email", "ALTER TABLE Customers ADD COLUMN email TEXT");
    let db = f.dir().join("data.db");
    let before = fs::read(&db).unwrap();
    let pending = pending_migrations(&db, &f.doc.config).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "m-email");
    assert!(pending[0].sql.contains("ADD COLUMN email"));
    // Read-only: the installation data is byte-identical after the preview.
    assert_eq!(fs::read(&db).unwrap(), before);
    f.apply("1.1.0").unwrap();
    assert!(pending_migrations(&db, &f.doc.config).unwrap().is_empty());
    // A missing database means every supported migration is pending.
    assert_eq!(
        pending_migrations(&f.root.join("none.db"), &f.doc.config)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn failing_migration_reverts_and_old_version_still_opens() {
    let mut f = Fixture::new();
    f.apply("1.0.0").unwrap();
    f.insert("Kept");
    f.add_migration("m-bad", "ALTER TABLE Missing ADD COLUMN x TEXT");
    let err = f.apply("1.1.0").unwrap_err();
    assert_eq!(err.code, "UPDATE_FAILED");
    assert!(
        err.message.contains("1.0.0 is still active"),
        "{}",
        err.message
    );
    assert_eq!(f.installed_version(), "1.0.0");
    assert_eq!(f.names(), ["Ada", "Grace", "Kept"]);
    let active = archive::read_archive(&f.dir().join("active").join(ARCHIVE)).unwrap();
    assert!(active.config.migrations.is_empty());
    assert!(fs::read_dir(f.dir()).unwrap().all(|e| !e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".staging")));
}

#[test]
fn health_check_failure_blocks_update() {
    let mut f = Fixture::new();
    f.apply("1.0.0").unwrap();
    f.doc.config.design.forms[0].source = Some(crate::design::FormSource {
        table: Some("Invoices".into()),
        ..Default::default()
    });
    let err = f.apply("1.1.0").unwrap_err();
    assert_eq!(err.code, "UPDATE_FAILED");
    assert!(err.message.contains("Invoices"));
    assert_eq!(f.installed_version(), "1.0.0");
}

#[test]
fn restore_previous_recovers_checkpoint() {
    let f = Fixture::new();
    f.apply("1.0.0").unwrap();
    checkpoint(&f.dir()).unwrap();
    f.insert("After checkpoint");
    restore_previous(&f.dir()).unwrap();
    assert_eq!(f.names(), ["Ada", "Grace"]);
}

#[test]
fn reset_replaces_records_after_checkpoint() {
    let f = Fixture::new();
    f.apply("1.0.0").unwrap();
    f.insert("Local");
    let preview = reset_preview(&f.dir(), &f.root.join(".tmp")).unwrap();
    assert_eq!(preview.current[0].rows, 3);
    assert_eq!(preview.bundled[0].rows, 2);
    assert!(preview.message.contains("3 record(s)"));
    reset_data(&f.dir()).unwrap();
    assert_eq!(f.names(), ["Ada", "Grace"]);
    let conn = Connection::open(f.dir().join("previous").join("data.db")).unwrap();
    let kept: i64 = conn
        .query_row("SELECT count(*) FROM Customers", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, 3);
}

#[test]
fn unsafe_bundle_id_is_rejected() {
    assert!(installation_dir(Path::new("/tmp"), "../escape").is_err());
    assert!(installation_dir(Path::new("/tmp"), "").is_err());
    assert!(installation_dir(Path::new("/tmp"), "0b7c-uuid_1").is_ok());
}

thread_local! {
    pub(super) static FAIL_ACTIVATION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[test]
fn windows_reserved_bundle_ids_are_rejected() {
    for id in [
        "CON", "con", "Prn", "aux", "NUL", "COM1", "com9", "LPT1", "lpt9",
    ] {
        assert!(installation_dir(Path::new("/tmp"), id).is_err(), "{id}");
    }
    for id in ["CON.txt", "nul.", "abc.", "abc "] {
        assert!(installation_dir(Path::new("/tmp"), id).is_err(), "{id}");
        assert!(is_windows_reserved(id) || id.ends_with(['.', ' ']), "{id}");
    }
    assert!(is_windows_reserved("lpt3.log"));
    for id in ["CONSOLE", "COM0", "COM10", "LPT", "aux_data", "nullable"] {
        assert!(installation_dir(Path::new("/tmp"), id).is_ok(), "{id}");
    }
}

#[test]
fn corrupt_installation_record_fails_closed_and_keeps_files() {
    let f = Fixture::new();
    f.apply("1.0.0").unwrap();
    f.insert("Precious");
    fs::write(f.dir().join(BUNDLE_JSON), b"{ not json").unwrap();
    // Even a bundle from a different signer must not be treated as a fresh install.
    let other = SigningKey::generate(&mut OsRng);
    let (signed, archive) = f.bundle_with("1.1.0", &other);
    let err = apply_bundle(&f.root, &signed, &archive).unwrap_err();
    assert_eq!(err.code, "INSTALLATION_CORRUPT");
    assert_eq!(
        check_signer(&f.root, &signed).unwrap_err().code,
        "INSTALLATION_CORRUPT"
    );
    assert_eq!(f.names(), ["Ada", "Grace", "Precious"]);
    // An unreadable record (here: a directory) is corrupt too, not "not installed".
    fs::remove_file(f.dir().join(BUNDLE_JSON)).unwrap();
    fs::create_dir(f.dir().join(BUNDLE_JSON)).unwrap();
    assert_eq!(
        f.apply("1.0.0").unwrap_err().code,
        "INSTALLATION_CORRUPT"
    );
    assert_eq!(f.names(), ["Ada", "Grace", "Precious"]);
}

#[test]
fn installation_without_record_is_moved_aside_not_deleted() {
    let f = Fixture::new();
    f.apply("1.0.0").unwrap();
    f.insert("Precious");
    fs::remove_file(f.dir().join(BUNDLE_JSON)).unwrap();
    assert_eq!(f.apply("1.0.0").unwrap().1, Action::Install);
    assert_eq!(f.names(), ["Ada", "Grace"]);
    let id = &f.doc.metadata.document_id;
    let aside: Vec<PathBuf> = fs::read_dir(&f.root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&format!("{id}.corrupt-"))
        })
        .collect();
    assert_eq!(aside.len(), 1);
    let kept: i64 = Connection::open(aside[0].join("data.db"))
        .unwrap()
        .query_row("SELECT count(*) FROM Customers", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, 3);
}

#[test]
fn failed_activation_restores_the_retired_version() {
    let f = Fixture::new();
    f.apply("1.0.0").unwrap();
    f.insert("Kept");
    FAIL_ACTIVATION.with(|x| x.set(true));
    let err = f.apply("1.1.0").unwrap_err();
    FAIL_ACTIVATION.with(|x| x.set(false));
    assert_eq!(err.code, "UPDATE_FAILED");
    assert!(err.message.contains("Activation failed"), "{}", err.message);
    assert_eq!(f.installed_version(), "1.0.0");
    assert_eq!(f.names(), ["Ada", "Grace", "Kept"]);
    assert!(f.dir().join("active").join(ARCHIVE).is_file());
    assert!(fs::read_dir(f.dir()).unwrap().all(|e| {
        let name = e.unwrap().file_name().to_string_lossy().into_owned();
        !name.starts_with(".retired") && !name.starts_with(".staging")
    }));
    // The restored installation still updates normally.
    assert_eq!(f.apply("1.1.0").unwrap().1, Action::Update);
}
