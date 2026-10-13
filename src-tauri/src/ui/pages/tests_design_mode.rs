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
    h.click("prop-delete");
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

#[gpui_kit::test]
fn declares_page_parameters_and_links_pages_with_values(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    // Page 1 takes a required region.
    new_dashboard(&mut h);
    h.click("add-page-param");
    h.fill("page-param-name:1", "region");
    h.click("page-param-required:1");
    let target = dashboard(&h);
    assert_eq!(target.parameters[0].name, "region");
    assert!(target.parameters[0].required);

    // Page 2 has a button that opens Page 1 with region 'East'.
    h.click("new-page");
    h.click("add-button");
    crate::ui::reports::test_support::choose(&mut h, "prop-target-page", "Page 1");
    h.fill("prop-page-param:region", "'East'");
    let source = dashboard(&h);
    let button = &source.components[0];
    assert_eq!(button.target_page_id.as_deref(), Some(target.id.as_str()));
    assert_eq!(button.page_params.get("region").map(String::as_str), Some("'East'"));

    // A navigation item to Page 1 takes a value too, and the links overview lists both.
    h.click("design-navigation");
    h.click("add-nav-item");
    let item = h.config().design.navigation.last().cloned().expect("nav item");
    crate::ui::reports::test_support::choose(&mut h, &format!("nav-target:{}", item.id), "Page 1");
    h.fill(id(&format!("nav-param:{}:region", item.id)), "'West'");
    let item = h.config().design.navigation.last().cloned().expect("nav item");
    assert_eq!(item.params.get("region").map(String::as_str), Some("'West'"));
    let links: Vec<String> = (1..=3).filter_map(|n| h.try_label(id(&format!("page-link:{n}")))).collect();
    assert!(links.contains(&"Button Action button 1 on Page 2 opens Page 1 (region = 'East')".to_string()) || links.iter().any(|l| l.contains("on Page 2 opens Page 1 (region = 'East')")), "{links:?}");
    assert!(links.iter().any(|l| l.ends_with("opens Page 1 (region = 'West')")), "{links:?}");
}

#[gpui_kit::test]
fn a_page_button_opens_its_target_page_in_the_runtime_with_the_values(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    crate::ui::reports::test_support::edit_store(&mut h, |c| {
        c.pages = serde_json::from_value(serde_json::json!([
            {"id": "detail", "name": "Detail", "parameters": [{"name": "region", "logicalType": "text"}],
             "components": [{"id": "t", "kind": "text", "title": "Note", "placement": {"column": 1, "row": 1, "columnSpan": 4},
                             "visibleWhen": "params.region == 'East'", "text": "East detail"}]},
            {"id": "home", "name": "Home", "components": [
                {"id": "b", "kind": "button", "title": "Go", "label": "Open East", "targetPageId": "detail",
                 "pageParams": {"region": "'East'"}, "placement": {"column": 1, "row": 1, "columnSpan": 4}}
            ]}
        ]))
        .unwrap();
        c.design.navigation = serde_json::from_value(serde_json::json!([
            {"id": "n", "label": "Home", "kind": "page", "targetId": "home"}
        ]))
        .unwrap();
        c.design.start_page = Some("n".into());
    });
    h.switch_mode(ModeId::Run);
    h.click(id("dash-button:Open East"));
    crate::ui::reports::test_support::wait_until(&mut h, "the detail page", |h| h.try_label(id("text:Note:0")).is_some());
    assert_eq!(h.label(id("text:Note:0")), "East detail");
}
