//! Helpers shared by the Design studio tests and its screenshot spec. They are generic over
//! the harness's context, so they drive both the test and the headless-renderer apps.

use super::DesignStudio;
use crate::design::{Control, ControlKind, Form};
use crate::ui::shell::ModeId;
use crate::ui::testing::{Harness, UiContext};
use gpui_kit::Entity;

pub use crate::ui::reports::test_support::{choose, edit_store, id, wait_until};

/// Opens Design mode.
pub fn open_designer<C: UiContext>(h: &mut Harness<'_, C>) {
    h.switch_mode(ModeId::Design);
    assert_eq!(h.mode(), ModeId::Design);
    assert_eq!(h.label("design-studio"), "Form builder");
}

pub fn studio<C: UiContext>(h: &mut Harness<'_, C>) -> Entity<DesignStudio> {
    h.read_shell(|s, _| s.mode_view())
        .expect("a mode is showing")
        .downcast::<DesignStudio>()
        .ok()
        .expect("Design mode is showing")
}

/// The first form of the document.
pub fn form<C: UiContext>(h: &Harness<'_, C>) -> Form {
    h.config().design.forms.into_iter().next().expect("a form")
}

/// The first control of `kind` on the first form.
pub fn control<C: UiContext>(h: &Harness<'_, C>, kind: ControlKind) -> Option<Control> {
    form(h).controls.into_iter().find(|c| c.kind == kind)
}

/// Waits until the studio has read the columns of `table`.
pub fn wait_for_columns<C: UiContext>(h: &mut Harness<'_, C>, table: &str) {
    let studio = studio(h);
    let table = table.to_string();
    wait_until(h, "the table columns", move |h| h.cx.read(|cx| studio.read(cx).has_columns(&table)));
}

/// The labels of the options the choice `button` offers (the menu is closed again).
pub fn options_of<C: UiContext>(h: &mut Harness<'_, C>, button: &str) -> Vec<String> {
    h.click(id(button));
    let mut seen = vec![];
    for index in 0..64usize {
        let label = h.update(|w, _| {
            use gpui_kit::test::TestWindowExt as _;
            w.within("popup-menu").try_find(index).map(|item| item.label().unwrap_or_default().to_string())
        });
        match label {
            Some(label) => seen.push(label),
            None => break,
        }
    }
    h.press("escape");
    seen
}
