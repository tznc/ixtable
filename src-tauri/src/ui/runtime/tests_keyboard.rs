//! Port of the keyboard-only case of tests/integration/accessibility.test.tsx: a record is
//! created in a Runtime form by typing, Tab and Enter, without pressing the Create button.
//! (The axe audit of every screen has no GPUI counterpart.) Imports are explicit.

use crate::ui::golden::journey::{eventually, open_page, scalar, start_from_template, wait};
use gpui_kit::TestAppContext;
use serde_json::json;

#[gpui_kit::test]
fn creates_a_record_in_the_runtime_with_the_keyboard_only(cx: &mut TestAppContext) {
    let mut h = start_from_template(cx, "inventory");
    let label = h.label.clone();
    open_page(&mut h, "Locations");
    h.click("new-record");
    wait(&mut h);
    // Focus reaches the first field, then Tab moves on to the next input.
    h.click("field:code");
    h.input("WH9");
    h.press("tab");
    h.input("Overflow yard");
    // Enter in an input saves the record.
    h.press("enter");
    eventually(&mut h, "the new location", |_| {
        scalar(&label, "SELECT count(*) FROM locations WHERE code = 'WH9'") == json!(1)
    });
    assert_eq!(scalar(&label, "SELECT name FROM locations WHERE code = 'WH9'"), json!("Overflow yard"));
    assert!(h.try_label("field:code:error").is_none());
}
