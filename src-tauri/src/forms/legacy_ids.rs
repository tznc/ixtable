//! Legacy `main` design ids (port of `src/design/legacyIds.ts`). The rewrite itself lives in
//! `design::upgrade` (Studio runs it when a document opens); these are the JSON-level entry
//! points the TypeScript module exposed.

use crate::design::upgrade::{LEGACY_DEFAULT_ID, legacy_ids, rekey_value};
use serde_json::Value;

/// True when the config still has the legacy `main` form or navigation id.
pub fn has_legacy_ids(config: &Value) -> bool {
    legacy_ids(config, String::new) != Default::default()
}

/// The config with the legacy ids replaced by ids from `new_id` and every reference followed;
/// None when there is nothing to rewrite.
pub fn upgrade_legacy_ids(config: &Value, new_id: impl FnMut() -> String) -> Option<Value> {
    let ids = legacy_ids(config, new_id);
    if ids == Default::default() {
        return None;
    }
    let mut next = config.clone();
    rekey_value(&mut next, &ids);
    Some(next)
}

/// The id the old default design gave its form and its navigation item.
pub const LEGACY_ID: &str = LEGACY_DEFAULT_ID;

#[cfg(test)]
#[path = "legacy_ids_tests.rs"]
mod tests;
