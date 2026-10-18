//! Screens no other spec captures (`extras-*`): bundled file sources, the Roles tab, the
//! Runtime previewed as a role, and expression errors in the Problems tab.

use super::runtime::seed_sales;
use crate::ui::runtime::tests::{generate, nav, seed, wait};
use crate::ui::data::testkit::choose;
use crate::ui::screenshot::context;
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;

const WINDOW: (f32, f32) = (1280.0, 1000.0);
const TALL: (f32, f32) = (1280.0, 1500.0);

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn extras_file_sources() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, WINDOW);
    h.click("new-document");
    h.switch_mode(ModeId::App);
    h.click("tab-files");
    assert!(h.exists("sources-empty"));
    let path = h.temp_path("sales.csv");
    std::fs::write(&path, "region,amount\nnorth,10\nsouth,5\nnorth,2\n").unwrap();
    h.dialogs.queue_open(Some(path));
    h.click("add-file-source");
    assert_eq!(h.label("source-preview"), "3 rows \u{b7} region (text), amount (integer)");
    h.capture(
        "extras-01-file-source-preview",
        &[
            "The File sources tab shows the chosen sales.csv with a Source name of sales and a header checkbox.",
            "A preview line reads 3 rows with region (text) and amount (integer), beside Add and Cancel buttons.",
        ],
    );
    h.click("confirm-add-source");
    assert_eq!(h.label("source:sales"), "files.sales");
    assert_eq!(h.label("source-rows:sales"), "3");
    h.capture(
        "extras-02-file-source-added",
        &["The list shows files.sales with format CSV, columns region, amount, 3 rows and a Remove button."],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn extras_roles_and_problems() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, TALL);
    h.click("new-document");
    seed_sales(&mut h);
    h.switch_mode(ModeId::Run);
    generate(&mut h);
    wait(&mut h);

    let orders_nav = nav(&mut h, "Orders");
    h.switch_mode(ModeId::App);
    h.click("tab-roles");
    assert!(h.label("rbac-note").contains("not a database security boundary"));
    h.click("add-role");
    h.fill("role-name", "Clerk");
    let list = h.config().design.forms.iter().find(|f| f.name == "Orders list").map(|f| f.id.clone()).expect("Orders list");
    h.click(format!("perm:form:{list}:read"));
    let page = h.config().pages.iter().find(|d| d.name == "Orders list").map(|d| d.id.clone()).expect("Orders list page");
    h.click(format!("perm:page:{page}:read"));
    h.click("perm:table:orders:read");
    h.click(format!("nav-perm:{}", orders_nav.trim_start_matches("nav:")));
    assert_eq!(h.config().roles[0].name, "Clerk");
    assert_eq!(h.config().roles[0].permissions.navigation.len(), 1);
    h.capture(
        "extras-03-roles",
        &[
            "The Roles tab opens with the note that roles are not a database security boundary.",
            "The Clerk role has a permission matrix with read checked for the Orders list form, the Orders list page and the orders table, and Orders shown in navigation.",
        ],
    );

    h.switch_mode(ModeId::Run);
    wait(&mut h);
    choose(&mut h, "run-as", "Clerk");
    wait(&mut h);
    assert!(h.exists("row:0"));
    assert!(!h.exists("new-record"));
    h.capture(
        "extras-04-runtime-as-role",
        &[
            "The Runtime previews as the Clerk role, with the role selector showing Clerk.",
            "The Orders list is shown with its records, no New record button is offered, and navigation lists only Orders.",
        ],
    );

    h.switch_mode(ModeId::App);
    seed(&mut h, |config| {
        let form = config.design.forms.iter_mut().find(|f| f.name == "Orders").expect("Orders form");
        let amount = form.controls.iter_mut().find(|c| c.label == "Amount").expect("Amount control");
        amount.visible_when = Some("record.total >".into());
        amount.validation.expression = Some("nope(1)".into());
    });
    h.click("tab-problems");
    h.settle();
    assert!(h.exists(("problem-open", 0usize)));
    assert!(h.label(("problem-open", 0usize)).starts_with("Open form Orders"));
    h.capture(
        "extras-05-expression-problems",
        &[
            "The Problems tab lists the broken expressions of the Orders form with the control and field that hold each.",
            "Each expression error has an Open form Orders link and a message such as an unexpected end of expression.",
        ],
    );
}
