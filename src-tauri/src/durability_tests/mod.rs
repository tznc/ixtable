//! Release-blocking durability tests (PRD §26.5, §27.1): archive upgrade fixtures,
//! archives from a newer build, forced termination during autosave, and the 500 MB
//! boundary check (opt-in locally, always run in CI).
use crate::manager::DocumentManager;
use std::path::PathBuf;

mod fixtures;
mod heavy;
mod incremental;
mod kill;
mod chaos;
mod newer_build;

/// A fresh state directory and a manager over it.
pub(crate) fn fresh_manager(tag: &str) -> (PathBuf, DocumentManager) {
    let base = std::env::temp_dir().join(format!("ixtable-{tag}-{}", uuid::Uuid::new_v4()));
    let m = DocumentManager::new(base.join("data"), base.join("cache")).unwrap();
    (base, m)
}

pub(crate) fn count_rows(m: &DocumentManager, window: &str, table: &str) -> i64 {
    let result = m
        .read_query(window, &format!("SELECT count(*) FROM {table}"))
        .unwrap();
    match &result.rows[0][0] {
        crate::data::DataValue::Integer(n) => *n,
        other => panic!("{table}: expected an integer, got {other:?}"),
    }
}
