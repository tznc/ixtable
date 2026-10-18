//! Archive upgrade fixtures (PRD §26.5). `tests/fixtures/archives/format-<N>/` holds
//! one `.ixt` per golden app, written by the build that introduced format N, plus a
//! `manifest.json` of what each must contain. Every later build must keep opening
//! them. Never rewrite an existing fixture directory: when the format changes, add
//! `format-<N+1>/` with `node scripts/ci/write-archive-fixtures.mjs`. Each manifest
//! entry records the sha256 of its file; CI (`scripts/ci/check-archive-fixtures.mjs`)
//! also rejects any change to a fixture file that exists on the base branch.
//! `format-1/` is derived from `format-2/` by `write_format_1_archive_fixtures`.
use super::{count_rows, fresh_manager};
use crate::archive_io::{self, FORMAT_VERSION, MIN_FORMAT_VERSION};
use crate::manager::DocumentManager;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const APPS: &[(&str, &[&str])] = &[
    (
        "crm",
        &[
            "deal_stages",
            "companies",
            "contacts",
            "deals",
            "activities",
        ],
    ),
    (
        "inventory",
        &[
            "suppliers",
            "locations",
            "products",
            "stock_movements",
            "reorder_thresholds",
            "transfers",
        ],
    ),
    (
        "work-orders",
        &[
            "statuses",
            "priorities",
            "assignees",
            "assets",
            "work_orders",
            "tasks",
            "work_order_log",
        ],
    ),
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct AssetEntry {
    id: String,
    checksum: String,
    size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct FixtureEntry {
    file: String,
    format_version: i64,
    document_id: String,
    forms: Vec<String>,
    queries: Vec<String>,
    reports: Vec<String>,
    dashboards: Vec<String>,
    rows: BTreeMap<String, i64>,
    assets: Vec<AssetEntry>,
    /// sha256 of the committed `.ixt` file; guards against rewriting a released fixture.
    #[serde(default)]
    sha256: String,
}

pub(super) fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/archives")
}

/// What the open document in `window` holds, in manifest form.
fn describe(
    m: &DocumentManager,
    window: &str,
    file: &str,
    format_version: i64,
    tables: &[&str],
) -> FixtureEntry {
    let c = m.config(window).unwrap();
    let mut assets: Vec<AssetEntry> = m
        .asset_list(window)
        .unwrap()
        .into_iter()
        .map(|a| AssetEntry {
            id: a.id,
            checksum: a.checksum,
            size: a.size,
        })
        .collect();
    assets.sort_by(|a, b| a.id.cmp(&b.id));
    FixtureEntry {
        file: file.into(),
        format_version,
        document_id: m.state(window).unwrap().document_id,
        forms: c.design.forms.iter().map(|f| f.id.clone()).collect(),
        queries: c.saved_queries.iter().map(|q| q.id.clone()).collect(),
        reports: c.reports.iter().map(|r| r.id.clone()).collect(),
        // Released fixtures list their dashboards; pages made for forms and reports on open are extra.
        dashboards: c.pages.iter().filter(|p| !crate::pages::upgrade::is_wrapper(p)).map(|d| d.id.clone()).collect(),
        rows: tables
            .iter()
            .map(|t| (t.to_string(), count_rows(m, window, t)))
            .collect(),
        assets,
        sha256: String::new(),
    }
}

fn file_sha256(path: &Path) -> String {
    archive_io::sha256_hex(&fs::read(path).unwrap())
}

fn fixture_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(root())
        .expect("tests/fixtures/archives exists")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("manifest.json").is_file())
        .collect();
    dirs.sort();
    dirs
}

fn read_manifest(dir: &Path) -> Vec<FixtureEntry> {
    serde_json::from_slice(&fs::read(dir.join("manifest.json")).unwrap()).unwrap()
}

fn write_manifest(dir: &Path, manifest: &[FixtureEntry]) {
    fs::write(
        dir.join("manifest.json"),
        format!("{}\n", serde_json::to_string_pretty(manifest).unwrap()),
    )
    .unwrap();
}

/// Opens `path` in `window` and checks it against `expected`, asset bytes included.
fn check_open(m: &DocumentManager, window: &str, path: &Path, expected: &FixtureEntry, what: &str) {
    m.open(window, path)
        .unwrap_or_else(|e| panic!("{what}: open: {e}"));
    let tables: Vec<&str> = expected.rows.keys().map(String::as_str).collect();
    let header = archive_io::read_header(path).unwrap();
    let mut actual = describe(m, window, &expected.file, header.format_version, &tables);
    actual.sha256 = expected.sha256.clone();
    assert_eq!(&actual, expected, "{what}");
    for a in &expected.assets {
        let bytes = fs::read(m.asset_path(window, &a.id).unwrap()).unwrap();
        assert_eq!(
            archive_io::sha256_hex(&bytes),
            a.checksum,
            "{what}: asset {}",
            a.id
        );
    }
}

/// Every committed fixture opens with this build and holds what its manifest says,
/// then survives a save (an upgrade, for older formats) and a reopen.
#[test]
fn committed_archive_fixtures_open_save_and_reopen() {
    let dirs = fixture_dirs();
    assert!(!dirs.is_empty(), "no archive fixtures are committed");
    let (base, m) = fresh_manager("fixtures");
    for dir in dirs {
        let manifest = read_manifest(&dir);
        assert_eq!(manifest.len(), APPS.len(), "{}", dir.display());
        for expected in manifest {
            let what = format!("{}/{}", dir.display(), expected.file);
            let fixture = dir.join(&expected.file);
            assert!(
                (MIN_FORMAT_VERSION..=FORMAT_VERSION).contains(&expected.format_version),
                "{what}: format {} is no longer supported",
                expected.format_version
            );
            archive_io::verify(&fixture).unwrap_or_else(|e| panic!("{what}: verify: {e}"));
            // Work on a copy: saving must never touch the committed fixture.
            let copy = base.join(&expected.file);
            fs::copy(&fixture, &copy).unwrap();
            let window = format!("fixture-{}", expected.file);
            check_open(&m, &window, &copy, &expected, &what);
            m.mark_data_dirty(&window).unwrap();
            m.save(&window, None)
                .unwrap_or_else(|e| panic!("{what}: save: {e}"));
            archive_io::verify(&copy).unwrap();
            let upgraded = FixtureEntry {
                format_version: FORMAT_VERSION,
                ..expected
            };
            check_open(&m, &window, &copy, &upgraded, &format!("{what} (reopened)"));
            m.close(&window, true).unwrap();
            fs::remove_file(&copy).unwrap();
        }
    }
    let _ = fs::remove_dir_all(base);
}

/// Writes `format-<FORMAT_VERSION>/` from the golden templates. Refuses to overwrite.
#[test]
#[ignore = "writes committed fixtures: node scripts/ci/write-archive-fixtures.mjs"]
fn write_archive_fixtures() {
    let dir = root().join(format!("format-{FORMAT_VERSION}"));
    assert!(
        !dir.exists(),
        "{} exists; fixtures of a released format are never rewritten",
        dir.display()
    );
    let (base, m) = fresh_manager("write-fixtures");
    fs::create_dir_all(&dir).unwrap();
    let mut manifest = vec![];
    for (app, tables) in APPS {
        let window = format!("write-{app}");
        crate::templates::create(&m, &window, app).unwrap();
        let file = format!("{app}.ixt");
        m.save(&window, Some(dir.join(&file))).unwrap();
        manifest.push(describe(&m, &window, &file, FORMAT_VERSION, tables));
        m.close(&window, true).unwrap();
    }
    assert!(
        manifest.iter().all(|e| e.rows.values().all(|n| *n > 0)),
        "every fixture table is seeded"
    );
    assert!(
        !manifest[1].assets.is_empty(),
        "the inventory fixture carries an asset"
    );
    for e in &mut manifest {
        e.sha256 = file_sha256(&dir.join(&e.file));
    }
    write_manifest(&dir, &manifest);
    let _ = fs::remove_dir_all(base);
}

/// Every committed fixture file matches the sha256 its manifest records, and every
/// file in a fixture directory is listed. A changed byte means a released fixture
/// was rewritten.
#[test]
fn committed_archive_fixtures_match_their_checksums() {
    let dirs = fixture_dirs();
    assert!(
        dirs.iter().any(|d| d.ends_with("format-1")),
        "format-1 fixtures exist"
    );
    assert!(
        dirs.iter()
            .any(|d| d.ends_with(format!("format-{FORMAT_VERSION}"))),
        "current-format fixtures exist"
    );
    for dir in dirs {
        let manifest = read_manifest(&dir);
        let mut listed: Vec<String> = manifest.iter().map(|e| e.file.clone()).collect();
        for e in &manifest {
            assert_eq!(
                e.sha256.len(),
                64,
                "{}/{}: no sha256",
                dir.display(),
                e.file
            );
            assert_eq!(
                file_sha256(&dir.join(&e.file)),
                e.sha256,
                "{}/{} changed; released fixtures are never rewritten",
                dir.display(),
                e.file
            );
        }
        listed.push("manifest.json".into());
        listed.sort();
        let mut present: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        present.sort();
        assert_eq!(present, listed, "{}", dir.display());
    }
}

/// Writes `format-1/` from the committed `format-2/` fixtures, byte for byte the way
/// ixtable 0.1 wrote format 1: no `application_id`, a rollback journal, no
/// `payload_chunks` table (each payload is one zstd blob in its row), and a
/// version-2 document config. Same documents, data, and assets as format 2, so both
/// directories share one manifest shape. Refuses to overwrite.
#[test]
#[ignore = "writes committed fixtures: node scripts/ci/write-archive-fixtures.mjs --format-1"]
fn write_format_1_archive_fixtures() {
    let dir = root().join("format-1");
    assert!(
        !dir.exists(),
        "{} exists; fixtures of a released format are never rewritten",
        dir.display()
    );
    let source = root().join("format-2");
    fs::create_dir_all(&dir).unwrap();
    let mut manifest = read_manifest(&source);
    for e in &mut manifest {
        let doc = archive_io::read_archive(&source.join(&e.file)).unwrap();
        let path = dir.join(&e.file);
        write_format_1(&path, &doc);
        e.format_version = 1;
        e.sha256 = file_sha256(&path);
    }
    write_manifest(&dir, &manifest);
}

fn write_format_1(path: &Path, doc: &crate::archive::ArchiveDocument) {
    use rusqlite::params;
    let zstd = |b: &[u8]| zstd::stream::encode_all(b, 3).unwrap();
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode=DELETE; PRAGMA foreign_keys=ON;
 CREATE TABLE archive_metadata(format_version INTEGER NOT NULL, document_id TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, application_version TEXT NOT NULL);
 CREATE TABLE data_payload(id INTEGER PRIMARY KEY CHECK(id=1), compression TEXT NOT NULL, checksum TEXT NOT NULL, uncompressed_size INTEGER NOT NULL, contents BLOB NOT NULL);
 CREATE TABLE document_config(id INTEGER PRIMARY KEY CHECK(id=1), version INTEGER NOT NULL, json TEXT NOT NULL);
 CREATE TABLE attachments(id TEXT PRIMARY KEY, display_name TEXT NOT NULL, media_type TEXT NOT NULL, checksum TEXT NOT NULL, uncompressed_size INTEGER NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, compression TEXT NOT NULL, contents BLOB NOT NULL);",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO archive_metadata VALUES(1,?1,?2,?3,'0.1.0')",
        params![
            doc.metadata.document_id,
            doc.metadata.created_at,
            doc.metadata.updated_at
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO data_payload VALUES(1,'zstd',?1,?2,?3)",
        params![
            archive_io::sha256_hex(&doc.data),
            doc.data.len() as i64,
            zstd(&doc.data)
        ],
    )
    .unwrap();
    let mut legacy = serde_json::to_value(&doc.config).unwrap();
    legacy["version"] = 2.into();
    conn.execute(
        "INSERT INTO document_config VALUES(1,2,?1)",
        [legacy.to_string()],
    )
    .unwrap();
    for a in &doc.attachments {
        conn.execute(
            "INSERT INTO attachments VALUES(?1,?2,?3,?4,?5,?6,?7,'zstd',?8)",
            params![
                a.id,
                a.display_name,
                a.media_type,
                archive_io::sha256_hex(&a.contents),
                a.contents.len() as i64,
                a.created_at,
                a.updated_at,
                zstd(&a.contents)
            ],
        )
        .unwrap();
    }
}
