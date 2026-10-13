//! Data export UI (port of `src/export/`): file names, the format menu and the
//! table export used by Data mode. Query mode reuses `menu::ExportMenu` with its own
//! runner.

pub mod menu;
pub mod names;

#[cfg(test)]
mod tests;
