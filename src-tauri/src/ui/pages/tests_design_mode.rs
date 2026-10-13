//! The Design mode: pages hold forms and reports, which open in their designers from the
//! page and come back to it. Imports are explicit: `gpui_kit::*` would shadow `#[test]`.

use super::test_support::{dashboard, design_mode, new_dashboard, open_dashboards};
use super::Focus;
use crate::design::NavKind;
use crate::pages::ComponentKind;
use crate::ui::reports::test_support::id;
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;

fn tall_document(cx: &mut TestAppContext) -> Harness<'_> {
    let mut h = Harness::start_sized(cx, (1500.0, 2400.0));
    h.click("new-document");
    assert!(h.has_shell(), "New document did not open the shell");
    h
}

fn focus(h: &mut Harness) -> Focus {
    let design = design_mode(h);
    h.cx.read(|cx| design.read(cx).focus)
}

#[gpui_kit::test]
fn the_mode_bar_has_one_design_mode_for_pages(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    assert!(!ModeId::ALL.iter().any(|m| matches!(m.key(), "reports" | "dashboards")));
    assert_eq!(ModeId::from_key("dashboards"), ModeId::Design);
    assert_eq!(ModeId::from_key("reports"), ModeId::Design);
    // A new document's main form is on a page of its own, which navigation opens.
    let config = h.config();
    assert_eq!(config.pages.len(), 1);
    assert_eq!(config.pages[0].components[0].form_id.as_deref(), Some(config.design.forms[0].id.as_str()));
    assert_eq!(config.design.navigation[0].kind, NavKind::Page);
    open_dashboards(&mut h);
    assert_eq!(h.label("page-item-1"), "Main form");
    assert_eq!(h.label("mode-title"), "Main form");
}

#[gpui_kit::test]
fn edits_a_pages_form_in_the_form_designer_and_comes_back(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_dashboards(&mut h);
    let component = h.config().pages[0].components[0].id.clone();
    h.click(id(&component));
    assert_eq!(h.label("edit-form"), "Edit form");
    h.click("edit-form");
    assert_eq!(focus(&mut h), Focus::Form);
    assert_eq!(h.label("design-studio"), "Form builder");
    assert_eq!(h.label("design-crumb-title"), "Form: Main form");
    h.click("add-text");
    assert_eq!(h.config().design.forms[0].controls.len(), 1);
    h.click("back-to-page");
    assert_eq!(focus(&mut h), Focus::Page);
    assert_eq!(h.label("mode-title"), "Main form");
}

#[gpui_kit::test]
fn a_new_form_component_creates_its_form(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    new_dashboard(&mut h);
    h.click("add-kpi");
    h.click("add-form");
    assert_eq!(h.label("edit-form"), "New form");
    h.click("edit-form");
    let config = h.config();
    let form = config.design.forms.iter().find(|f| f.name == "Page 1 form").expect("the new form");
    let page = dashboard(&h);
    let block = page.components.iter().find(|c| c.kind == ComponentKind::Form).expect("form component");
    assert_eq!(block.form_id.as_deref(), Some(form.id.as_str()));
    assert_eq!(focus(&mut h), Focus::Form);
    assert_eq!(h.label("design-crumb-title"), "Form: Page 1 form");
    // The form is on Page 1, so it gets no page of its own.
    assert_eq!(config.pages.len(), 2);
    h.click("back-to-page");
    assert_eq!(h.label("mode-title"), "Page 1");
}

#[gpui_kit::test]
fn a_report_component_opens_the_report_designer(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    new_dashboard(&mut h);
    h.click("add-report");
    h.click("edit-report");
    let report = h.config().reports.into_iter().next().expect("a report");
    assert_eq!(report.name, "Page 1 report");
    assert_eq!(focus(&mut h), Focus::Report);
    assert_eq!(h.label("design-crumb-title"), "Report: Page 1 report");
    assert!(h.try_label("report-name").is_some());
    h.click("back-to-page");
    assert_eq!(focus(&mut h), Focus::Page);
    let block = &dashboard(&h).components[0];
    assert_eq!(block.report_id.as_deref(), Some(report.id.as_str()));
}

#[gpui_kit::test]
fn new_forms_and_reports_get_a_page_and_deleting_them_removes_it(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_dashboards(&mut h);
    h.click("new-report");
    assert_eq!(focus(&mut h), Focus::Report);
    let report = h.config().reports[0].clone();
    assert!(h.config().pages.iter().any(|p| p.name == report.name && p.components[0].report_id.as_ref() == Some(&report.id)));
    h.click("delete-report");
    assert!(h.config().reports.is_empty());
    assert_eq!(h.config().pages.len(), 1);
    assert_eq!(focus(&mut h), Focus::Page);

    h.click("new-form");
    assert_eq!(focus(&mut h), Focus::Form);
    assert_eq!(h.config().pages.len(), 2);
    h.click("delete-form");
    assert_eq!(h.config().pages.len(), 1);
    assert_eq!(focus(&mut h), Focus::Page);
}

#[gpui_kit::test]
fn a_form_on_no_page_is_listed_and_opens(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_dashboards(&mut h);
    let form = h.config().design.forms[0].id.clone();
    let component = h.config().pages[0].components[0].id.clone();
    h.click(id(&component));
    h.press("delete");
    assert!(h.config().pages[0].components.is_empty());
    let loose = format!("loose-form:{form}");
    assert_eq!(h.label(id(&loose)), "Main form");
    h.click(id(&loose));
    assert_eq!(focus(&mut h), Focus::Form);
}

#[gpui_kit::test]
fn the_navigation_editor_is_in_the_sidebar(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_dashboards(&mut h);
    h.click("design-navigation");
    assert_eq!(focus(&mut h), Focus::Navigation);
    assert_eq!(h.label("mode-title"), "Navigation");
    assert!(h.try_label("design-navigation").is_some());
    h.click("page-item-1");
    assert_eq!(focus(&mut h), Focus::Page);
}

#[gpui_kit::test]
fn the_runtime_treats_the_blank_main_form_page_as_an_empty_app(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    h.switch_mode(ModeId::Run);
    // The page only shows the blank main form, so the Runtime offers to build the app.
    assert!(h.try_label("rt-empty").is_some());
}
