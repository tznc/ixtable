//! Form and Runtime logic with no UI: design helpers, generators, validation and
//! conditions, record data access and caches, RBAC, and navigation. The GPUI views
//! (`ui::runtime`, `ui::grid`) are built on these. Ports of `src/design/*` (non-UI) and
//! `src/runtime/*` (non-UI).

pub mod actions;
pub mod conditions;
pub mod constraints;
pub mod cursor;
pub mod data;
pub mod events;
pub mod field_values;
pub mod fields;
pub mod generate;
pub mod generate_app;
pub mod legacy_ids;
pub mod lookups;
pub mod mask;
pub mod navigation;
pub mod nesting;
pub mod operations;
pub mod pushdown;
pub mod rbac;
pub mod registry;
pub mod richtext;
pub mod richtext_edit;
pub mod richtext_images;
pub mod richtext_markup;
pub mod richtext_to_markdown;
pub mod row_writes;
pub mod schema;
pub mod state;
pub mod track_input;
pub mod trigger_warnings;
pub mod values;

#[cfg(test)]
pub mod testkit;
#[cfg(test)]
mod race_tests;
