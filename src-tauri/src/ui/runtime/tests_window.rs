//! The Run App window: Studio's "Run App" split button opens the app in a window of its
//! own and runs it as the owner or a role. Imports are explicit.

use super::tests::{run_as, run_body, seed, wait};
use super::window::{self, OWNER};
use crate::roles::Role;
use crate::ui::data::testkit::{choose_open, menu_labels};
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;

fn clerk() -> Role {
    Role { id: "clerk".into(), name: "Clerk".into(), permissions: Default::default() }
}

fn read_notes(h: &Harness) -> &'static str {
    match crate::read_table_page(h.label.clone(), "notes".into(), 0, 10, vec![], vec![], None) {
        Ok(_) => "OK",
        Err(e) if e.code == "FORBIDDEN" => "FORBIDDEN",
        Err(_) => "ERROR",
    }
}

fn role_in_force(h: &mut Harness) -> Option<String> {
    let body = run_body(h);
    h.cx.read(|cx| body.read(cx).runtime().read(cx).role_id())
}

#[gpui_kit::test]
fn run_app_opens_its_own_window_and_studio_keeps_its_mode(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.switch_mode(ModeId::Design);
    assert!(!h.exists("mode-run"), "Run App is not a Studio mode");
    assert!(h.run_app_window().is_none());

    h.click("run-app-open");
    let run = h.run_app_window().expect("the Run App window opened");
    assert_ne!(run, h.main);
    assert_eq!(h.mode(), ModeId::Design, "Studio stays in its mode");
    assert_eq!(h.label("mode-title"), "Form builder");

    h.focus_window(run);
    assert_eq!(h.label("mode-title"), "Run App");
    assert_eq!(h.label("run-as"), OWNER);
    assert_eq!(role_in_force(&mut h), None);

    // Clicking Run App again brings the same window back.
    h.focus_window(h.main);
    h.click("run-app-open");
    assert_eq!(h.run_app_window(), Some(run));
    assert_eq!(h.config().active_mode, "design");
}

#[gpui_kit::test]
fn the_run_app_menu_runs_the_app_as_a_role(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.create_table("notes", &[("id", "INTEGER", 1), ("body", "TEXT", 0)]);
    seed(&mut h, |c| c.roles = vec![clerk()]);

    h.click_in("run-app", "popup");
    let labels = menu_labels(&mut h);
    assert!(labels.ends_with(&[OWNER.to_string(), "Clerk".to_string()]), "{labels:?}");
    choose_open(&mut h, "Clerk");
    let run = h.run_app_window().expect("the Run App window opened");
    h.focus_window(run);
    assert_eq!(h.label("run-as"), "Clerk");
    assert_eq!(role_in_force(&mut h).as_deref(), Some("clerk"));
    assert_eq!(read_notes(&h), "FORBIDDEN", "Rust enforces the role");

    // Studio in front keeps full access; the role applies again in the Run App window.
    h.focus_window(h.main);
    assert_eq!(read_notes(&h), "OK");
    h.focus_window(run);
    assert_eq!(read_notes(&h), "FORBIDDEN");

    // The window's own picker switches back to the owner.
    run_as(&mut h, OWNER);
    wait(&mut h);
    assert_eq!(role_in_force(&mut h), None);
    assert_eq!(read_notes(&h), "OK");

    // The Studio menu switches the open window to a role.
    h.focus_window(h.main);
    h.click_in("run-app", "popup");
    choose_open(&mut h, "Clerk");
    assert_eq!(h.run_app_window(), Some(run), "the same window");
    h.focus_window(run);
    assert_eq!(role_in_force(&mut h).as_deref(), Some("clerk"));
}

#[gpui_kit::test]
fn closing_the_document_closes_the_run_app_window(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.create_table("notes", &[("id", "INTEGER", 1), ("body", "TEXT", 0)]);
    seed(&mut h, |c| c.roles = vec![clerk()]);
    h.click_in("run-app", "popup");
    choose_open(&mut h, "Clerk");
    let run = h.run_app_window().expect("the Run App window opened");
    h.focus_window(run);
    assert_eq!(read_notes(&h), "FORBIDDEN");

    h.focus_window(h.main);
    h.dialogs.queue_confirm(true);
    h.click("close-project");
    wait(&mut h);
    assert!(!h.has_shell());
    assert!(h.run_app_window().is_none(), "the window closed with the document");
    let label = h.label.clone();
    assert!(h.cx.read(|cx| window::find(&label, cx)).is_none());
}

#[test]
fn a_stored_run_mode_maps_to_the_data_studio_mode() {
    assert_eq!(ModeId::from_key("run"), ModeId::Data);
    assert_eq!(ModeId::from_key("design"), ModeId::Design);
}
