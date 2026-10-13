//! Common imports for UI modules: `use crate::ui::prelude::*;`.

pub use crate::ui::{task, widgets};
pub use crate::ui::widgets::StartAligned as _;
pub use gpui_kit::assets::IconName;
pub use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
pub use gpui_kit::prelude::FluentBuilder as _;
pub use gpui_kit::*;
