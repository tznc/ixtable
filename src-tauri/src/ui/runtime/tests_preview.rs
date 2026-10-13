//! Role preview failures and cleanup (port of tests/unit/role-preview.test.tsx). The React hooks
//! become the `Runtime` entity: a refused preview falls back to developer access with an error
//! notice, and dropping the entity (leaving Run mode) clears what Rust still enforces.
//! Imports are explicit.

use super::Runtime;
use crate::data::{DataValue, NamedValue};
use crate::forms::actions::NoticeTone;
use crate::ui::testing::Harness;
use gpui_kit::{AppContext as _, TestAppContext};

fn insert(label: &str) -> Result<(), String> {
    let name = NamedValue { column: "name".into(), value: DataValue::Text("x".into()) };
    crate::recordstore::commands::insert_row(label.into(), "things".into(), vec![name], None)
        .map(|_| ())
        .map_err(|e| e.code)
}

#[gpui_kit::test]
fn shows_a_refused_role_preview_and_falls_back_to_developer_access(cx: &mut TestAppContext) {
    // No session answers to this window label, so Rust refuses the preview.
    let runtime = cx.new(|_| Runtime::new("no-such-window"));
    runtime.update(cx, |runtime, cx| runtime.set_role_id(Some("clerk".into()), cx));
    let (message, tone, role) = cx.read(|cx| {
        let runtime = runtime.read(cx);
        let notice = runtime.notice.clone().expect("a notice");
        (notice.message, notice.tone, runtime.role_id())
    });
    assert!(message.starts_with("Could not preview this role: "), "{message}");
    assert_eq!(tone, NoticeTone::Error);
    assert_eq!(role, None);
}

#[gpui_kit::test]
fn clears_a_leftover_rust_preview_when_run_mode_is_left(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.create_table("things", &[("id", "INTEGER", 1), ("name", "TEXT", 0)]);
    h.database_changed();
    let label = h.label.clone();
    assert_eq!(insert(&label), Ok(()));

    let runtime = h.cx.update(|cx| cx.new(|_| Runtime::new(&label)));
    h.cx.update(|cx| runtime.update(cx, |runtime, cx| runtime.set_role_id(Some("ghost".into()), cx)));
    // An unknown role is denied everything, and Rust enforces it.
    assert_eq!(insert(&label), Err("FORBIDDEN".into()));
    assert!(h.cx.read(|cx| runtime.read(cx).notice.is_none()));

    drop(runtime);
    h.settle();
    assert_eq!(insert(&label), Ok(()), "leaving Run mode restores full access");
}
