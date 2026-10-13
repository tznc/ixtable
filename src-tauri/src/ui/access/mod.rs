//! Microsoft Access import (docs/decisions/access-import.md): the start-screen wizard
//! and the Settings > Access migration report.

pub mod host;
pub mod migration;
pub mod migration_tab;
pub mod summary;
pub mod wizard;
#[cfg(test)]
pub(crate) mod tests;
