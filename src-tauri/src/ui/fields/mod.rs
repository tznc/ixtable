//! Field settings: input masks, rich text, attachments and multi-select values
//! (port of `src/fields/`). The pure helpers live here; the editors that use them
//! are in `settings_editor` and `inputs`.

pub mod datasheet;
pub mod settings_editor;

#[cfg(test)]
mod tests;
