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
pub mod fields;
pub mod generate;
pub mod generate_app;
pub mod legacy_ids;
pub mod lookups;
pub mod navigation;
pub mod nesting;
pub mod operations;
pub mod pushdown;
pub mod rbac;
pub mod registry;
pub mod row_writes;
pub mod schema;
pub mod state;
pub mod trigger_warnings;
pub mod values;

#[cfg(test)]
pub mod testkit;
