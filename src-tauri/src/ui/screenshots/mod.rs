//! Screenshot specs, one per critical flow (see `ui::screenshot` and the
//! `/app-qa` skill). Captures are named `<flow>-NN-<what>`. Every capture
//! comes after assertions that prove the UI reached the intended state.
//!
//! Run them with `cargo screenshot [filter]` from `src-tauri/`.

mod app_shell;
mod query;
mod datasheet;
mod export;
mod fields;
mod schema;
mod shop;
mod reports;
