//! Release and runtime bundles: export (Settings > Release), the open/update flow, the
//! runtime sidebar, database login and diagnostics. Ports of src/release/*.

pub mod bundle_error;
pub mod bundle_flow;
pub mod database_login;
pub mod gate;
pub mod release_tab;
pub mod runtime_bar;
pub mod runtime_diagnostics;
pub mod sidebar;
pub mod update_confirm;
#[cfg(test)]
mod tests;
