//! Generated in-memory forms: resolved by id, cached per table schema, never in the design.

use super::{is_designed_form, resolve_form, table_forms};
use crate::archive::DocumentConfig;
use crate::forms::generate::GenerateOptions;
use crate::forms::testkit::{col, pk, table};
use crate::forms::schema::new_form;

fn items(extra: &str) -> crate::data::TableSchema {
    table("registry_items", vec![pk(col("id", "INTEGER"), 1), col(extra, "TEXT")], vec![])
}

#[test]
fn generates_a_list_and_a_detail_form_once_per_schema() {
    let first = table_forms(&items("name"), &GenerateOptions::default());
    let again = table_forms(&items("name"), &GenerateOptions::default());
    assert_eq!(first.list.id, again.list.id);
    assert_eq!(first.detail.id, again.detail.id);
    let changed = table_forms(&items("title"), &GenerateOptions::default());
    assert_ne!(changed.list.id, first.list.id);
}

#[test]
fn resolves_designed_forms_first_then_generated_ones() {
    let generated = table_forms(&items("label"), &GenerateOptions::default());
    let mut config = DocumentConfig::default();
    assert_eq!(resolve_form(&config, &generated.list.id).map(|f| f.id), Some(generated.list.id.clone()));
    assert!(!is_designed_form(&config, &generated.list));
    let designed = new_form("Designed", None);
    config.design.forms.push(designed.clone());
    assert_eq!(resolve_form(&config, &designed.id).map(|f| f.name), Some("Designed".into()));
    assert!(is_designed_form(&config, &designed));
    assert!(resolve_form(&config, "missing").is_none());
}
