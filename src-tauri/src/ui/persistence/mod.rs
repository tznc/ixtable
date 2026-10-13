//! Persistence views: assets, archive size, checkpoints, logs and crash recovery.
//! Ports of src/persistence/*.

pub mod archive_size;
pub mod asset_preview;
pub mod assets_tab;
pub mod checkpoints;
pub mod format;
pub mod logs_tab;
pub mod recovery_list;
pub mod text_sections;
#[cfg(test)]
mod tests;

use gpui_kit::{ElementId, SharedString};

/// An element id built from text (`export-asset:logo.png`).
pub fn eid(text: String) -> ElementId {
    SharedString::from(text).into()
}
