// Attachment field storage tests (included from record_attachments.rs).
use super::*;
use crate::recordstore::conformance::{each_store, per_store, Harness, Store};

#[test]
fn hex_and_checksum_match_known_values() {
    assert_eq!(hex(&[0, 15, 16, 255]), "000f10ff");
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn referenced_ids_reads_only_file_arrays() {
    let value = r#"[{"id":"a","name":"x.pdf"},{"name":"no id"},{"id":"b"}]"#;
    assert_eq!(referenced_ids(value), vec!["a", "b"]);
    assert!(referenced_ids("not json").is_empty());
    assert!(referenced_ids(r#"{"id":"a"}"#).is_empty());
}

fn stores_reads_and_finds_unused_files(store: Store) {
    each_store(store, |h: &mut Harness| {
        let bytes: Vec<u8> = (0..=255u8).cycle().take(70_000).collect();
        let a = store_file(h.store.as_mut(), "docs", "files", "../a/report.pdf", "application/pdf", &bytes)
            .unwrap();
        let b = store_file(h.store.as_mut(), "docs", "files", "b.txt", "text/plain", b"hello").unwrap();
        assert_eq!(a.name, "report.pdf");
        assert_eq!(a.size, 70_000);
        h.reader.refresh().unwrap();

        let (table, column, content) = read_file(&h.reader, &a.id).unwrap().unwrap();
        assert_eq!((table.as_str(), column.as_str()), ("docs", "files"));
        assert_eq!(content.file, a);
        assert_eq!(STANDARD.decode(content.content_base64).unwrap(), bytes);
        let missing = uuid::Uuid::now_v7().to_string();
        assert!(read_file(&h.reader, &missing).unwrap().is_none());
        assert!(read_file(&h.reader, "x' OR '1'='1").is_err());

        h.store
            .execute_internal("CREATE TABLE docs(id INTEGER PRIMARY KEY, files TEXT)", &[])
            .unwrap();
        let refs = serde_json::to_string(&[&a]).unwrap();
        h.store
            .execute_internal("INSERT INTO docs(id, files) VALUES (1, ?)", &[refs])
            .unwrap();
        h.reader.refresh().unwrap();
        let fields = [("docs".to_string(), "files".to_string())];
        let later = chrono::Utc::now() + chrono::Duration::hours(2);
        // Inside the grace period nothing is unused; after it, only the unreferenced file.
        assert!(unused_files(&h.reader, &fields, chrono::Utc::now()).unwrap().is_empty());
        assert_eq!(unused_files(&h.reader, &fields, later).unwrap(), vec![b.id.clone()]);
    });
}

fn a_store_without_attachments_reads_nothing(store: Store) {
    each_store(store, |h: &mut Harness| {
        h.reader.refresh().unwrap();
        let id = uuid::Uuid::now_v7().to_string();
        assert!(read_file(&h.reader, &id).unwrap().is_none());
        assert!(unused_files(&h.reader, &[], chrono::Utc::now()).unwrap().is_empty());
    });
}

per_store!(
    stores_reads_and_finds_unused_files,
    a_store_without_attachments_reads_nothing,
);

#[test]
fn sqlite_store_keeps_the_exact_bytes() {
    let dir = std::env::temp_dir().join(format!("ixtable-attach-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("data.db");
    rusqlite::Connection::open(&path).unwrap();
    let mut store = crate::recordstore::sqlite::SqliteRecordStore::new(path.clone());
    let bytes = [0u8, 1, 127, 128, 254, 255];
    let file = store_file(&mut store, "t", "c", "a.bin", "application/octet-stream", &bytes).unwrap();
    let c = rusqlite::Connection::open(&path).unwrap();
    let (content, size): (Vec<u8>, i64) = c
        .query_row(
            "SELECT content, size FROM _ixtable_attachments WHERE id = ?",
            [&file.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(content, bytes);
    assert_eq!(size, 6);
    drop(c);
    let _ = std::fs::remove_dir_all(dir);
}
