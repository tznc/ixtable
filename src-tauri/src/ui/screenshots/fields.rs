//! Field formats in the table designer (`fields-01`). The Runtime entry capture
//! (`fields-02`) belongs to the Runtime spec.

use crate::ui::data::testkit::{data_mode, make_table};
use crate::ui::screenshot::context;
use crate::ui::testing::{Harness, UiContext as _};
use serde_json::json;

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn fields() {
    let mut cx = context();
    // Tall enough to show the Field settings section below the fold.
    let mut h = Harness::start_sized(&mut cx, (1280.0, 1900.0));
    h.click("new-document");
    let text = |name: &str| json!({ "name": name, "declaredType": "TEXT" });
    make_table(
        &h,
        "contacts",
        json!([
            { "name": "id", "declaredType": "INTEGER", "primaryKeyPosition": 1 },
            text("name"), text("phone"), text("notes"), text("interests"), text("documents"),
        ]),
        json!([]),
    );
    h.database_changed();
    h.click("table:contacts");
    h.click("design-table");
    h.append("phone input mask", "(000) 000-0000");
    h.pick("notes format", "Rich text");
    h.pick("interests format", "Multiple choices");
    let mode = data_mode(&mut h);
    let designer = h.cx.read(|cx| mode.read(cx).designer().cloned()).expect("designer");
    let editor = h.cx.read(|cx| designer.read(cx).field_settings().clone());
    h.update(|window, cx| {
        editor.update(cx, |e, cx| e.set_choices("interests", "Pricing\nSupport\nTraining", window, cx))
    });
    h.pick("documents format", "Attachments");
    h.settle();
    let fields = h.config().entities.iter().find(|e| e.table == "contacts").map_or(0, |e| e.fields.len());
    assert_eq!(fields, 4);
    h.capture(
        "fields-01-table-designer",
        &[
            "The Field settings section lists the text columns with a Format picker each.",
            "phone shows the input mask (000) 000-0000 and its preview (___) ___-____.",
            "notes is Rich text, interests is Multiple choices with three choices, documents is Attachments.",
        ],
    );
}
