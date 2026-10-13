//! Forms generated in memory (table pages, related lists without a form), never persisted
//! (port of `src/runtime/registry.ts`).

use super::generate::{GenerateOptions, generate_crud_forms};
use crate::archive::DocumentConfig;
use crate::data::TableSchema;
use crate::design::Form;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

#[derive(Default)]
struct Registry {
    ephemeral: HashMap<String, Form>,
    by_table: HashMap<String, TableForms>,
}

/// Generated list and detail forms of one table.
#[derive(Clone, Debug)]
pub struct TableForms {
    pub list: Form,
    pub detail: Form,
    signature: String,
}

fn registry() -> MutexGuard<'static, Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Looks a form up in the design, then among generated in-memory forms.
pub fn resolve_form(config: &DocumentConfig, form_id: &str) -> Option<Form> {
    config
        .design
        .forms
        .iter()
        .find(|f| f.id == form_id)
        .cloned()
        .or_else(|| registry().ephemeral.get(form_id).cloned())
}

/// True for forms defined in the document (the permission subject is the form, not its table).
pub fn is_designed_form(config: &DocumentConfig, form: &Form) -> bool {
    config.design.forms.iter().any(|f| f.id == form.id)
}

/// Generated list + detail forms for a table, built with the same generator as "Generate
/// form from table" and cached per table schema.
pub fn table_forms(schema: &TableSchema, options: &GenerateOptions) -> TableForms {
    let mut children: Vec<&str> = options.children.iter().map(|c| c.name.as_str()).collect();
    children.sort_unstable();
    let mut targets: Vec<&str> = options.targets.keys().map(String::as_str).collect();
    targets.sort_unstable();
    let signature = serde_json::json!([schema, children, targets, options.child_forms.len()]).to_string();
    let mut registry = registry();
    if let Some(hit) = registry.by_table.get(&schema.name).filter(|h| h.signature == signature) {
        return hit.clone();
    }
    let generated = generate_crud_forms(schema, options);
    registry.ephemeral.insert(generated.list.id.clone(), generated.list.clone());
    registry.ephemeral.insert(generated.detail.id.clone(), generated.detail.clone());
    let entry = TableForms { list: generated.list, detail: generated.detail, signature };
    registry.by_table.insert(schema.name.clone(), entry.clone());
    entry
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
