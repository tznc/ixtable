//! ixtable's design system (`docs/decisions/design-system.md`).
//!
//! gpui-bootstrap supplies the primitives (buttons, alerts, badges, modals,
//! form controls) and one token [`Theme`]. This crate re-themes it for
//! ixtable and adds the app's own components. Each one does one job, so a
//! screen never has to choose between two ways of saying the same thing.
//!
//! ```ignore
//! ixtable_design::init(cx);
//! // `PageHeader`, `ActionButton`, `Callout`, `Field`, `DataTable` and so on.
//! use ixtable_design::prelude::*;
//! ```

pub mod action;
pub mod app_shell;
pub mod callout;
pub mod data_table;
pub mod dialog;
pub mod empty_state;
pub mod field;
pub mod key_value;
pub mod page_header;
pub mod record_form;
pub mod ribbon;
pub mod section;
pub mod sidebar;
pub mod status_badge;
pub mod tabs;
pub mod theme;
pub mod toast;
pub mod typography;

#[cfg(test)]
mod contrast_audit;

pub use typography::{Tone, Type};

pub use gpui_bootstrap::{ActiveTheme, ColorMode, Platform, Theme, Variant};
pub use theme::{DESKTOP, change_mode, init, native};

/// Every component, the type scale and the theme.
pub mod prelude {
    pub use crate::action::{ActionButton, ButtonLook, Emphasis};
    pub use crate::app_shell::AppShell;
    pub use crate::callout::Callout;
    pub use crate::data_table::{Align, Column, DataTable};
    pub use crate::dialog::{Dialog, DialogSize};
    pub use crate::empty_state::EmptyState;
    pub use crate::field::Field;
    pub use crate::key_value::KeyValue;
    pub use crate::page_header::PageHeader;
    pub use crate::record_form::RecordForm;
    pub use crate::ribbon::{Ribbon, RibbonCommand, RibbonGroup};
    pub use crate::section::Section;
    pub use crate::sidebar::{Sidebar, SidebarGroup, SidebarItem};
    pub use crate::status_badge::StatusBadge;
    pub use crate::tabs::{TabStyle, Tabs};
    pub use crate::toast::Toast;
    pub use crate::typography::{Tone, Type, TypeScale as _};
    pub use gpui_bootstrap::prelude::*;
}
