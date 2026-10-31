//! Chaos findings in the form designer (`chaos-NN-*`). Exploratory captures of the bugs that
//! the probes in `ui::design::tests_chaos` found.

use crate::design::ControlKind;
use crate::ui::design::test_support::{control, form, id, open_designer, studio};
use crate::ui::screenshot::context;
use crate::ui::testing::Harness;

const WINDOW: (f32, f32) = (1500.0, 1200.0);

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn chaos_orphaned_tab_control() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, WINDOW);
    h.click("new-document");
    open_designer(&mut h);
    h.click("add-tabs");
    let tabs = control(&h, ControlKind::Tabs).expect("tabs");
    h.click(format!("tab-page:{}:{}", tabs.id, tabs.tabs[1].id));
    let s = studio(&mut h);
    let tabs_id = tabs.id.clone();
    h.cx.update(|cx| s.update(cx, |s, cx| s.select(Some(tabs_id), cx)));
    h.settle();
    h.click("remove-tab-2");
    h.click("add-label");
    let label = control(&h, ControlKind::Label).expect("the label exists in the definition");
    let tab = label.parent.as_ref().and_then(|p| p.tab.clone()).expect("a tab parent");
    assert!(!control(&h, ControlKind::Tabs).expect("tabs").tabs.iter().any(|t| t.id == tab), "the label sits on a removed tab page");
    assert!(h.exists(id(&label.id)), "drawn on the stale (removed) page");
    h.capture(
        "chaos-01a-control-on-removed-tab",
        &[
            "The tab group shows only \"Tab 1\", not highlighted, yet a \"New text\" card is drawn below it (the removed page).",
        ],
    );
    let tabs = control(&h, ControlKind::Tabs).expect("tabs");
    h.click(format!("tab-page:{}:{}", tabs.id, tabs.tabs[0].id));
    assert!(!h.exists(id(&label.id)), "once Tab 1 is chosen the label is on no page");
    assert!(control(&h, ControlKind::Label).is_some(), "but it is still in the definition");
    h.capture(
        "chaos-01b-control-unreachable",
        &["With Tab 1 active the label card is gone from every canvas, although the form still stores it."],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn chaos_undo_removes_several_adds() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, WINDOW);
    h.click("new-document");
    open_designer(&mut h);
    for _ in 0..3 {
        h.click("add-text");
    }
    assert_eq!(form(&h).controls.len(), 3);
    h.capture("chaos-02-before-undo", &["The canvas shows three \"New text field\" cards."]);
    h.click("undo");
    assert_eq!(form(&h).controls.len(), 0);
    h.capture("chaos-03-after-one-undo", &["After one Undo the form is empty (\"This form is empty\"), not two cards."]);
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn chaos_duplicate_form_names() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, WINDOW);
    h.click("new-document");
    open_designer(&mut h);
    h.click("new-form");
    h.click("new-form");
    let second = h.config().design.forms[1].id.clone();
    crate::ui::design::test_support::open_form(&mut h, &second);
    h.settle();
    h.click("delete-form");
    h.click("new-form");
    let names: Vec<String> = h.config().design.forms.iter().map(|f| f.name.clone()).collect();
    assert_eq!(names, ["Main form", "Form 3", "Form 3"]);
    h.capture("chaos-04-duplicate-form-names", &["Two forms are both named \"Form 3\" (the page list and form title show the clash)."]);
}
