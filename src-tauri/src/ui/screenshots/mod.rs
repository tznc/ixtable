//! Screenshot specs, one per critical flow (see `ui::screenshot` and the
//! `/app-qa` skill). Captures are named `<flow>-NN-<what>`. Every capture
//! comes after assertions that prove the UI reached the intended state.
//!
//! Run them with `cargo screenshot [filter]` from `src-tauri/`.

mod app_shell;
mod automation;
mod cloud;
mod pages;
mod query;
mod datasheet;
mod design;
mod export;
mod extras;
mod import;
mod fields;
mod schema;
mod shop;
mod reports;
mod ribbon;
mod run_app;
mod runtime;
mod access_import;
mod migrations;
mod persistence;
mod release;
mod settings;
mod templates;
