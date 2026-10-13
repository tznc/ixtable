//! Design mode: the form builder (port of `src/design/`). The pure logic (operations,
//! schema, generation, nesting, constraints, track input) lives in `crate::forms`; this
//! module is the views.
//!
//! - `DesignStudio` (`studio.rs`) is the mode's view: form list and palette, canvas,
//!   navigation editor or preview, and the properties panel. The panes are `impl` blocks in
//!   sibling files: `form_list`, `canvas`, `control_props`, `kind_props`, `form_props`,
//!   `layout_settings`, `styles`, `navigation`, `asset_picker`.
//! - `DesignPreview` (`preview.rs`) renders the Runtime's `FormRenderer` detached.
//! - `CreateFormsOffer` and `GenerateAppButton` generate forms and navigation (Data mode
//!   shows the offer after a table is created).
//! - `inputs.rs` holds the text-input caches; `ctl.rs` the labeled controls built on them.
//!
//! Test ids are documented in each file. Elements are found by id; the visible label of a
//! field is its accessibility label.

/// An edit handler for the control `$cid`: `|c, v|` mutates control `c` with the new value `v`
/// (owned: text, `Option` text or a bool).
macro_rules! edit_with {
    ($cid:expr, $label:expr, |$c:ident, $v:ident| $body:expr) => {{
        let cid: String = $cid.clone();
        move |this: &mut $crate::ui::design::studio::DesignStudio,
              $v,
              cx: &mut gpui_kit::Context<$crate::ui::design::studio::DesignStudio>| {
            this.edit_control(&cid, $label, move |$c| $body, cx)
        }
    }};
}

/// Like `edit_with!` for a choice, whose value arrives as `&str` (`$v` is an owned `String`).
macro_rules! edit_pick {
    ($cid:expr, $label:expr, |$c:ident, $v:ident| $body:expr) => {{
        let cid: String = $cid.clone();
        move |this: &mut $crate::ui::design::studio::DesignStudio,
              picked: &str,
              cx: &mut gpui_kit::Context<$crate::ui::design::studio::DesignStudio>| {
            let $v = picked.to_string();
            this.edit_control(&cid, $label, move |$c| $body, cx)
        }
    }};
}

mod asset_picker;
mod canvas;
mod control_props;
mod create_offer;
mod ctl;
mod form_list;
mod form_props;
mod generate;
mod inputs;
mod kind_props;
mod layout_settings;
mod navigation;
mod preview;
mod studio;
mod styles;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_flows;
#[cfg(test)]
mod tests_forms;
#[cfg(test)]
mod tests_panels;
#[cfg(test)]
pub(crate) mod test_support;

pub use create_offer::{CreateFormsOffer, OfferEvent};
pub use generate::{GenerateAppButton, GenerateEvent, apply_generated_app, generate_app};
pub use preview::DesignPreview;
pub use studio::{DesignStudio, View};
