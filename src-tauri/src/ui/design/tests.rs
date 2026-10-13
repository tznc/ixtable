//! Design studio flows (port of `tests/integration/design-studio.test.tsx`, and the designer
//! parts of `guided-crud`, `expressions` and `expression-problems`). Imports are explicit:
//! `gpui_kit::*` would shadow `#[test]`.
//!
//! The studio is driven through the window like a user would; what a test reads back is the
//! stored definition (`h.config()`), the labels of the studio's elements and, for the preview,
//! the Runtime renderer's entities.

use super::test_support::{choose, control, form, open_designer, studio, wait_for_columns, wait_until};
use crate::data::DataValue;
use crate::design::ControlKind;
use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;

/// A new document in a window tall enough to show the whole properties panel (the harness
/// only clicks what is visible).
pub fn tall_document(cx: &mut TestAppContext) -> Harness<'_> {
    let mut h = Harness::start_sized(cx, (1500.0, 4200.0));
    h.click("new-document");
    assert!(h.has_shell(), "New document did not open the shell");
    h
}

#[gpui_kit::test]
fn edits_binds_undoes_and_previews_a_design_with_live_data(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    h.create_table("products", &[("id", "INTEGER", 1), ("name", "TEXT", 0)]);
    h.insert_row(
        "products",
        &[("id", DataValue::Integer(1)), ("name", DataValue::Text("Live product".into()))],
    );
    open_designer(&mut h);

    choose(&mut h, "data-table", "products");
    h.click("add-text");
    wait_for_columns(&mut h, "products");
    h.fill("prop-label", "Product name");
    choose(&mut h, "bound-column", "name");
    let text = control(&h, ControlKind::Text).expect("the text control");
    assert_eq!(text.binding.as_ref().map(|b| b.column.as_str()), Some("name"));
    // The control is a labelled item on the canvas.
    assert_eq!(h.label(text.id.clone()), "Product name");

    h.click("undo");
    assert_eq!(control(&h, ControlKind::Text).expect("control").binding, None);
    h.click("redo");
    assert_eq!(
        control(&h, ControlKind::Text).expect("control").binding.as_ref().map(|b| b.column.as_str()),
        Some("name")
    );

    h.click("tab-preview");
    assert_eq!(h.label("design-preview"), "Main form preview");
    wait_until(&mut h, "the live row", |h| h.try_label("cell:0:name").as_deref() == Some("Live product"));
    assert_eq!(h.label("col:name"), "Product name");

    choose(&mut h, "preview-mode", "Detail");
    let studio = studio(&mut h);
    wait_until(&mut h, "the live record", |h| {
        let preview = h.cx.read(|cx| studio.read(cx).preview()).expect("the preview");
        let Some(renderer) = h.cx.read(|cx| preview.read(cx).renderer()) else {
            return false;
        };
        h.cx.read(|cx| {
            renderer
                .read(cx)
                .record()
                .is_some_and(|record| record.read(cx).record().get("name").and_then(|v| v.as_str()) == Some("Live product"))
        })
    });
    assert_eq!(h.label("design-preview"), "Main form preview");
    assert_eq!(form(&h).modes.len(), 4);
}
