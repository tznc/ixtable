//! Chaos tests for manual save and autosave (exploratory). Each test asserts the
//! behaviour a user should get; a failure is a finding.
use crate::data::{DataValue, NamedValue};
use crate::manager::{AppError, SessionState};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn m() -> &'static crate::manager::DocumentManager {
    crate::manager().unwrap()
}

fn label(tag: &str) -> String {
    format!("chaos-{tag}-{}", uuid::Uuid::new_v4())
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ixtable-chaos-{tag}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&d).unwrap();
    d
}

fn table(w: &str, name: &str) {
    let spec = json!({
        "name": name,
        "columns": [
            {"name": "id", "declaredType": "INTEGER", "nullable": false, "primaryKeyPosition": 1,
             "unique": false, "defaultExpression": null, "generatedExpression": null},
            {"name": "body", "declaredType": "TEXT", "nullable": true, "primaryKeyPosition": 0,
             "unique": false, "defaultExpression": null, "generatedExpression": null},
        ],
        "foreignKeys": [], "checks": [], "withoutRowid": false,
    });
    crate::create_database_table(w.into(), serde_json::from_value(spec).unwrap()).unwrap();
}

fn insert(w: &str, t: &str, body: &str) -> Result<(), AppError> {
    crate::records::insert(
        w,
        t,
        vec![NamedValue {
            column: "body".into(),
            value: DataValue::Text(body.into()),
        }],
    )
    .map(|_| ())
    .map_err(|e| e.error)
}

/// A saved document with table `items`; the session stays open.
fn saved(tag: &str) -> (String, PathBuf, PathBuf) {
    let w = label(tag);
    let d = dir(tag);
    m().new_session(&w).unwrap();
    table(&w, "items");
    insert(&w, "items", "seed").unwrap();
    let path = d.join("doc.ixt");
    m().save(&w, Some(path.clone())).unwrap();
    (w, d, path)
}

fn rename(w: &str, name: &str) -> SessionState {
    let mut c = m().config(w).unwrap();
    c.name = name.into();
    m().update_config(w, c).unwrap()
}

/// Rows of `t` and the config name in the archive at `path` (opened in a fresh window).
fn archived(path: &Path, t: &str) -> (i64, String) {
    crate::archive_io::verify(path).unwrap();
    let w = label("verify");
    m().open(&w, path).unwrap();
    let rows = super::count_rows(m(), &w, t);
    let name = m().config(&w).unwrap().name;
    m().close(&w, true).unwrap();
    (rows, name)
}

fn temps(d: &Path) -> Vec<String> {
    fs::read_dir(d)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect()
}

#[cfg(unix)]
fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p).unwrap().permissions().mode() & 0o7777
}

// ---- File-system chaos ----------------------------------------------------------

#[cfg(unix)]
#[test]
fn chaos_save_keeps_the_files_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let (w, _d, path) = saved("perm");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    rename(&w, "private");
    m().save(&w, None).unwrap();
    assert_eq!(mode(&path), 0o600, "save widened a private file's permissions");
}

#[cfg(unix)]
#[test]
fn chaos_save_through_a_symlink_updates_the_target() {
    let (w, d, real) = saved("symlink");
    m().close(&w, true).unwrap();
    let link = d.join("link.ixt");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let w = label("symlink2");
    m().open(&w, &link).unwrap();
    rename(&w, "via link");
    m().save(&w, None).unwrap();
    assert!(
        fs::symlink_metadata(&link).unwrap().file_type().is_symlink(),
        "save replaced the symlink with a regular file"
    );
    assert_eq!(archived(&real, "items").1, "via link");
}

#[cfg(unix)]
#[test]
fn chaos_save_refuses_a_read_only_file() {
    use std::os::unix::fs::PermissionsExt;
    let (w, _d, path) = saved("rofile");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
    let before = fs::read(&path).unwrap();
    rename(&w, "overwrite read-only");
    let result = m().save(&w, None);
    let after = fs::read(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(result.is_err(), "saved over a read-only file; mode now {:o}", mode(&path));
    assert_eq!(before, after);
}

#[cfg(unix)]
#[test]
fn chaos_save_into_a_read_only_folder_fails_cleanly() {
    use std::os::unix::fs::PermissionsExt;
    let (w, d, path) = saved("rodir");
    let before = fs::read(&path).unwrap();
    fs::set_permissions(&d, fs::Permissions::from_mode(0o555)).unwrap();
    rename(&w, "blocked");
    let result = m().save(&w, None);
    fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
    let e = result.expect_err("save into a read-only folder succeeded (running as root?)");
    eprintln!("read-only folder error: {e}");
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(temps(&d).is_empty(), "{:?}", temps(&d));
    let s = m().state(&w).unwrap();
    assert!(s.dirty);
    assert!(
        !e.message.contains("os error"),
        "raw OS error shown to the user: {e}"
    );
}

/// Set `IXTABLE_CHAOS_TINY_DIR` to a small (few MB) mounted filesystem.
#[test]
fn chaos_save_on_a_full_disk_keeps_the_last_archive_and_the_dirty_flag() {
    let Ok(tiny) = std::env::var("IXTABLE_CHAOS_TINY_DIR") else {
        eprintln!("skipped: IXTABLE_CHAOS_TINY_DIR unset");
        return;
    };
    let d = PathBuf::from(tiny).join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&d).unwrap();
    let w = label("full");
    m().new_session(&w).unwrap();
    table(&w, "items");
    let path = d.join("doc.ixt");
    m().save(&w, Some(path.clone())).unwrap();
    let before = fs::read(&path).unwrap();
    // Incompressible rows bigger than the free space.
    let db = m().database_path(&w).unwrap();
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("CREATE TABLE fill(b BLOB); WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i < 40) INSERT INTO fill SELECT randomblob(131072) FROM n;")
        .unwrap();
    m().mark_data_dirty(&w).unwrap();
    let e = m().autosave(&w).expect_err("autosave on a full disk succeeded");
    eprintln!("full-disk error: {e}");
    assert_eq!(fs::read(&path).unwrap(), before, "last archive changed");
    assert!(temps(&d).is_empty(), "temp left behind: {:?}", temps(&d));
    let s = m().state(&w).unwrap();
    assert!(s.dirty && s.last_error.is_some());
    assert!(
        e.message.to_lowercase().contains("disk") || e.message.to_lowercase().contains("space"),
        "message does not say the disk is full: {e}"
    );
    m().close(&w, true).unwrap();
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn chaos_manual_save_after_the_file_was_deleted() {
    let (w, d, path) = saved("deleted");
    fs::remove_file(&path).unwrap();
    rename(&w, "after delete");
    let result = m().save(&w, None);
    eprintln!("deleted-file save: {:?}", result.as_ref().map(|s| s.dirty));
    match result {
        Ok(_) => assert_eq!(archived(&path, "items").1, "after delete"),
        Err(e) => {
            assert!(m().state(&w).unwrap().dirty);
            panic!("Ctrl+S cannot recreate a deleted file: {e}");
        }
    }
    let _ = d;
}

#[test]
fn chaos_file_replaced_externally_is_a_conflict_and_never_overwritten() {
    let (w, d, path) = saved("replaced");
    let other = label("other");
    m().new_session(&other).unwrap();
    m().save(&other, Some(d.join("other.ixt"))).unwrap();
    m().close(&other, true).unwrap();
    fs::copy(d.join("other.ixt"), &path).unwrap();
    let foreign = fs::read(&path).unwrap();
    rename(&w, "mine");
    let auto = m().autosave(&w).unwrap_err();
    assert_eq!(auto.code, "EXTERNAL_CONFLICT");
    assert!(m().state(&w).unwrap().conflict);
    let e = m().save(&w, None).unwrap_err();
    assert_eq!(e.code, "EXTERNAL_CONFLICT");
    assert_eq!(fs::read(&path).unwrap(), foreign);
    assert!(m().state(&w).unwrap().dirty);
}

/// A clean document whose file changed: Save marks a conflict, then Close must still work.
#[test]
fn chaos_clean_document_with_a_conflict_can_still_be_closed() {
    let (w, d, path) = saved("stuck");
    let other = label("other");
    m().new_session(&other).unwrap();
    m().save(&other, Some(path.clone())).unwrap();
    m().close(&other, true).unwrap();
    let _ = m().save(&w, None);
    let s = m().state(&w).unwrap();
    eprintln!("dirty={} conflict={}", s.dirty, s.conflict);
    // What the UI does when the document is not dirty: close without force.
    if !s.dirty {
        m().close(&w, false)
            .expect("a clean but conflicted document cannot be closed without discarding");
    }
    let _ = d;
}

// ---- Concurrency chaos ---------------------------------------------------------

#[test]
fn chaos_writes_ddl_config_edits_and_saves_race_without_losing_anything() {
    let (w, d, path) = saved("race");
    let stop = Arc::new(AtomicBool::new(false));
    let inserted = Arc::new(AtomicUsize::new(1));
    let errors = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let mut handles = vec![];
    for t in 0..3 {
        let (w, inserted, errors) = (w.clone(), inserted.clone(), errors.clone());
        handles.push(std::thread::spawn(move || {
            for i in 0..150 {
                match insert(&w, "items", &format!("{t}-{i}")) {
                    Ok(()) => {
                        inserted.fetch_add(1, Ordering::SeqCst);
                        let _ = m().mark_data_dirty(&w);
                    }
                    Err(e) => errors.lock().unwrap().push(format!("insert: {e}")),
                }
            }
        }));
    }
    {
        let (w, errors) = (w.clone(), errors.clone());
        handles.push(std::thread::spawn(move || {
            for i in 0..15 {
                table(&w, &format!("ddl_{i}"));
                if let Err(e) = insert(&w, &format!("ddl_{i}"), "x") {
                    errors.lock().unwrap().push(format!("ddl insert: {e}"));
                }
            }
        }));
    }
    {
        let w = w.clone();
        handles.push(std::thread::spawn(move || {
            for i in 0..200 {
                rename(&w, &format!("name-{i}"));
            }
        }));
    }
    let mut savers = vec![];
    for kind in 0..2 {
        let (w, stop, errors) = (w.clone(), stop.clone(), errors.clone());
        savers.push(std::thread::spawn(move || {
            let mut n = 0;
            while !stop.load(Ordering::SeqCst) {
                let r = if kind == 0 { m().save(&w, None) } else { m().autosave(&w) };
                if let Err(e) = r {
                    errors.lock().unwrap().push(format!("save: {e}"));
                }
                n += 1;
            }
            n
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    stop.store(true, Ordering::SeqCst);
    let saves: usize = savers.into_iter().map(|h| h.join().unwrap()).sum();
    let errors = errors.lock().unwrap().clone();
    assert!(errors.is_empty(), "{} errors during the race, first: {:?}", errors.len(), errors.first());
    m().save(&w, None).unwrap();
    assert!(!m().state(&w).unwrap().dirty);
    let (rows, name) = archived(&path, "items");
    assert_eq!(rows as usize, inserted.load(Ordering::SeqCst), "after {saves} saves");
    assert_eq!(name, "name-199");
    for i in 0..15 {
        assert_eq!(archived(&path, &format!("ddl_{i}")).0, 1);
    }
    assert!(temps(&d).is_empty());
}

/// A save that reports the document clean must have every committed write in the archive.
#[test]
fn chaos_a_clean_flag_after_a_racing_save_means_the_archive_has_the_write() {
    let (w, _d, path) = saved("dirtyflag");
    let mut expected = 1;
    for i in 0..40 {
        let w2 = w.clone();
        let saver = std::thread::spawn(move || m().save(&w2, None));
        std::thread::sleep(Duration::from_micros((i % 8) * 400));
        insert(&w, "items", "race").unwrap();
        m().mark_data_dirty(&w).unwrap();
        expected += 1;
        saver.join().unwrap().unwrap();
        if !m().state(&w).unwrap().dirty {
            assert_eq!(archived(&path, "items").0, expected, "iteration {i}: clean but stale");
        }
    }
}

#[test]
fn chaos_action_queries_during_saves() {
    let (w, _d, path) = saved("action");
    for i in 0..20 {
        insert(&w, "items", &format!("a{i}")).unwrap();
    }
    m().mark_data_dirty(&w).unwrap();
    let mut c = m().config(&w).unwrap();
    c.saved_queries.push(crate::archive::SavedQuery {
        id: "q-up".into(),
        name: "Touch".into(),
        sql: "UPDATE items SET body = body || '!'".into(),
        action: Some(crate::archive::ActionSpec {
            kind: crate::archive::ActionKind::Update,
            table: "items".into(),
        }),
        ..Default::default()
    });
    m().update_config(&w, c).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let saver = {
        let (w, stop) = (w.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut errs = vec![];
            while !stop.load(Ordering::SeqCst) {
                if let Err(e) = m().save(&w, None) {
                    errs.push(e.to_string());
                }
            }
            errs
        })
    };
    let mut run_errs = vec![];
    for _ in 0..10 {
        if let Err(e) = crate::records::run_action_query(&w, "q-up", vec![], false, 0) {
            run_errs.push(e.error.to_string());
        }
    }
    stop.store(true, Ordering::SeqCst);
    let save_errs = saver.join().unwrap();
    assert!(run_errs.is_empty(), "{run_errs:?}");
    assert!(save_errs.is_empty(), "{save_errs:?}");
    m().save(&w, None).unwrap();
    assert!(!m().state(&w).unwrap().dirty, "clean after final save");
    let v = label("v");
    m().open(&v, &path).unwrap();
    let r = m()
        .read_query(&v, "SELECT count(*) FROM items WHERE body LIKE '%!!!!!!!!!!'")
        .unwrap();
    m().close(&v, true).unwrap();
    assert_eq!(r.rows[0][0], DataValue::Integer(21));
}

#[test]
fn chaos_rapid_repeated_saves() {
    let (w, d, path) = saved("rapid");
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let w = w.clone();
            std::thread::spawn(move || {
                (0..15)
                    .map(|j| {
                        if j % 3 == 0 {
                            rename(&w, &format!("r{i}-{j}"));
                        }
                        m().save(&w, None).map(|_| ())
                    })
                    .filter_map(Result::err)
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let errs: Vec<_> = threads.into_iter().flat_map(|t| t.join().unwrap()).collect();
    assert!(errs.is_empty(), "{errs:?}");
    m().save(&w, None).unwrap();
    assert!(!m().state(&w).unwrap().dirty);
    let name = m().config(&w).unwrap().name;
    assert_eq!(archived(&path, "items").1, name);
    assert!(temps(&d).is_empty());
}

/// Two windows write the same path: errors must name the conflict, not leak raw I/O.
#[test]
fn chaos_two_windows_saving_one_path() {
    let (a, d, path) = saved("twowin");
    let b = label("twowin-b");
    m().new_session(&b).unwrap();
    table(&b, "items");
    let stop = Arc::new(AtomicBool::new(false));
    let run = |w: String, as_path: bool, stop: Arc<AtomicBool>, path: PathBuf| {
        std::thread::spawn(move || {
            let mut codes = vec![];
            while !stop.load(Ordering::SeqCst) {
                rename(&w, "x");
                let r = if as_path { m().save(&w, Some(path.clone())) } else { m().save(&w, None) };
                if let Err(e) = r {
                    codes.push(format!("{e}"));
                }
            }
            codes
        })
    };
    let ta = run(a.clone(), false, stop.clone(), path.clone());
    let tb = run(b.clone(), true, stop.clone(), path.clone());
    std::thread::sleep(Duration::from_secs(3));
    stop.store(true, Ordering::SeqCst);
    let mut errs = ta.join().unwrap();
    errs.extend(tb.join().unwrap());
    crate::archive_io::verify(&path).unwrap();
    let unexpected: Vec<_> = errs.iter().filter(|e| !e.starts_with("EXTERNAL_CONFLICT")).collect();
    assert!(unexpected.is_empty(), "{} unexpected: {:?}", unexpected.len(), unexpected.first());
    let _ = (d, m().close(&a, true), m().close(&b, true));
}

/// Save As onto the file another window has open: that window must see a conflict.
#[test]
fn chaos_save_as_over_a_file_open_in_another_window() {
    let (a, d, path) = saved("saveas-open");
    let b = label("saveas-b");
    m().new_session(&b).unwrap();
    m().save(&b, Some(path.clone())).unwrap();
    rename(&a, "a edits");
    assert_eq!(m().autosave(&a).unwrap_err().code, "EXTERNAL_CONFLICT");
    assert!(m().save(&a, None).is_err());
    m().save(&a, Some(d.join("a-copy.ixt"))).unwrap();
    assert_eq!(archived(&d.join("a-copy.ixt"), "items").1, "a edits");
}

/// A large data.db: record writes that land while a save packs it must not fail.
/// Set `IXTABLE_CHAOS_HEAVY=<MB>`.
#[test]
fn chaos_record_writes_during_a_large_save() {
    let Some(mb) = std::env::var("IXTABLE_CHAOS_HEAVY").ok().and_then(|v| v.parse::<u64>().ok())
    else {
        eprintln!("skipped: IXTABLE_CHAOS_HEAVY unset");
        return;
    };
    let (w, _d, path) = saved("heavy");
    let db = m().database_path(&w).unwrap();
    let n = mb * 8;
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch(&format!("CREATE TABLE fill(b BLOB); WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i < {n}) INSERT INTO fill SELECT randomblob(131072) FROM n;"))
        .unwrap();
    m().mark_data_dirty(&w).unwrap();
    let w2 = w.clone();
    let started = Instant::now();
    let saver = std::thread::spawn(move || m().autosave(&w2).map(|_| ()));
    let mut errs = vec![];
    let mut writes = 0;
    while !saver.is_finished() {
        let t = Instant::now();
        match insert(&w, "items", "during") {
            Ok(()) => writes += 1,
            Err(e) => errs.push(format!("{e} after {} ms", t.elapsed().as_millis())),
        }
        let _ = m().mark_data_dirty(&w);
        std::thread::sleep(Duration::from_millis(100));
    }
    let save = saver.join().unwrap();
    eprintln!(
        "{mb} MB save took {} ms; {writes} writes ok, {} failed",
        started.elapsed().as_millis(),
        errs.len()
    );
    save.unwrap();
    assert!(errs.is_empty(), "{errs:?}");
    m().save(&w, None).unwrap();
    assert_eq!(archived(&path, "items").0 as usize, 1 + writes);
    m().close(&w, true).unwrap();
}
