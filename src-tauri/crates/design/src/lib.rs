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
pub mod callout;
pub mod data_table;
pub mod empty_state;
pub mod field;
pub mod key_value;
pub mod page_header;
pub mod section;
pub mod status_badge;
pub mod tabs;
pub mod theme;
pub mod toast;
pub mod typography;

pub use typography::{Tone, Type};

pub use gpui_bootstrap::{ActiveTheme, ColorMode, Theme, Variant};
pub use theme::{change_mode, init};

/// Every component, the type scale and the theme.
pub mod prelude {
    pub use crate::action::{ActionButton, Emphasis};
    pub use crate::callout::Callout;
    pub use crate::data_table::{Align, Column, DataTable};
    pub use crate::empty_state::EmptyState;
    pub use crate::field::Field;
    pub use crate::key_value::KeyValue;
    pub use crate::page_header::PageHeader;
    pub use crate::section::Section;
    pub use crate::status_badge::StatusBadge;
    pub use crate::tabs::Tabs;
    pub use crate::toast::Toast;
    pub use crate::typography::{Tone, Type, TypeScale as _};
    pub use gpui_bootstrap::prelude::*;
}
