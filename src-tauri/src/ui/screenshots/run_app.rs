//! Run App window captures (`run-app-*`): Studio's "Run App" split button and its role
//! menu, and the app running in its own window as the owner and as a role.

use super::runtime::seed_sales;
use crate::roles::{ObjectPermission, Permissions, Role};
use crate::ui::data::testkit::menu_labels;
use crate::ui::runtime::tests::{generate, nav, run_as, seed, wait};
use crate::ui::runtime::window::OWNER;
use crate::ui::screenshot::context;
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn run_app() {
    let mut cx = context();
    let mut h = Harness::start(&mut cx);
    h.click("new-document");
    seed_sales(&mut h);
    generate(&mut h);
    wait(&mut h);
    let customers = nav(&mut h, "Customers");
    seed(&mut h, move |config| {
        let id = customers.trim_start_matches("nav:").to_string();
        let mut sales = Role { id: "sales".into(), name: "Sales rep".into(), permissions: Permissions::default() };
        sales.permissions.navigation = vec![id];
        // Read on the Customers forms, so the page opens too.
        sales.permissions.objects = config
            .design
            .forms
            .iter()
            .filter(|f| f.name.starts_with("Customers"))
            .map(|f| ObjectPermission { kind: "form".into(), id: f.id.clone(), read: true, ..Default::default() })
            .collect();
        config.roles = vec![sales, Role { id: "auditor".into(), name: "Auditor".into(), permissions: Permissions::default() }];
    });
    let run = h.run_app_window().expect("Run App is open");
    assert_ne!(h.label("mode-title"), "Run App", "the window is titled with the app name");
    assert_eq!(h.label("run-as"), OWNER);
    h.capture(
        "run-app-01-owner-window",
        &[
            "A separate window titled 'Run App' shows the app's navigation (Customers, Orders, Order items).",
            "The header has a 'Run as' select reading 'Owner (default)'.",
        ],
    );

    h.switch_mode(ModeId::Design);
    h.click_in("run-app", "popup");
    let labels = menu_labels(&mut h);
    assert!(labels.ends_with(&[OWNER.to_string(), "Sales rep".into(), "Auditor".into()]), "{labels:?}");
    h.capture(
        "run-app-02-studio-role-menu",
        &[
            "Studio's sidebar ends its mode list with a primary 'Run App' split button; 'Runtime' is no longer a mode.",
            "The button's menu, headed 'Run as', lists 'Owner (default)', 'Sales rep' and 'Auditor', with the owner checked.",
        ],
    );
    h.press("escape");

    h.focus_window(run);
    run_as(&mut h, "Sales rep");
    wait(&mut h);
    assert_eq!(h.label("run-as"), "Sales rep");
    h.capture(
        "run-app-03-sales-rep",
        &[
            "The Run App window runs as 'Sales rep': its navigation shows only Customers, with the customers list open.",
            "The 'Run as' select reads 'Sales rep'.",
        ],
    );
}
