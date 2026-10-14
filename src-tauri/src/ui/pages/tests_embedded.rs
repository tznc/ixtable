//! Forms embedded in dashboards (the dashboard cases of
//! `tests/integration/data-binding-guards.test.tsx` and `data-binding.test.tsx`). The
//! embedded forms are `FormRenderer`s, so their element ids repeat across widgets; tests look
//! them up inside the widget (`widget:<title>`). Imports are explicit.

use super::test_support::{
    at, bin_query, define, field, insert_part, open_dashboards, open_in_runtime, seed_parts, uid, value_in,
};
use crate::ui::reports::test_support::{choose, id, wait_until};
use crate::ui::runtime::tests::{wait, with_real_runner};
use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;
use serde_json::json;

fn tall_document(cx: &mut TestAppContext) -> Harness<'_> {
    let mut h = Harness::start_sized(cx, (1500.0, 2400.0));
    h.click("new-document");
    assert!(h.has_shell(), "New document did not open the shell");
    h
}

/// Opens the page added last (after the new document's main form page) on its View tab.
fn open_view(h: &mut Harness) {
    open_dashboards(h);
    let last = h.config().pages.len();
    h.click(id(&format!("page-item-{last}")));
    h.click("tab-view");
}

fn name_in(h: &mut Harness, widget: &str) -> Option<String> {
    value_in(h, widget, "field:part-name")
}

#[gpui_kit::test]
fn pins_a_first_row_embedded_edit_form_while_it_has_unsaved_changes(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    seed_parts(&mut h);
    let form = uid();
    define(
        &mut h,
        vec![json!({
            "id": form, "name": "Part", "source": {"kind": "table", "table": "parts"},
            "modes": ["detail", "edit"],
            "controls": [field("Part name", "name", "text", 1), field("Quantity", "qty", "number", 2)],
        })],
        vec![json!({
            "id": uid(), "name": "Stock",
            "components": [{"id": uid(), "kind": "form", "title": "Edit", "formId": form, "mode": "edit", "placement": at(1, 1, 6)}],
        })],
    );
    open_view(&mut h);
    wait_until(&mut h, "the first part", |h| name_in(h, "widget:Edit").as_deref() == Some("Part 01"));
    h.fill("field:quantity", "11");

    // A write elsewhere does not move the form off its record while it has unsaved edits.
    let label = h.label.clone();
    crate::recordstore::commands::execute_write_batch(label.clone(), vec![insert_part(0, "A")], None).expect("insert");
    h.records_changed();
    wait(&mut h);
    assert_eq!(name_in(&mut h, "widget:Edit").as_deref(), Some("Part 01"));
    assert_eq!(value_in(&mut h, "widget:Edit", "field:quantity").as_deref(), Some("11"));

    // Once saved, the form looks for the first row again and finds the new one.
    h.click_in("widget:Edit", "save");
    wait_until(&mut h, "the new first part", |h| name_in(h, "widget:Edit").as_deref() == Some("Part 00"));

    let name = crate::data::NamedValue { column: "name".into(), value: crate::data::DataValue::Text("Part 00".into()) };
    crate::recordstore::commands::delete_row(label.clone(), "parts".into(), vec![crate::data::DataValue::Integer(0)], Some(vec![name]), None)
        .expect("delete");
    h.records_changed();
    wait_until(&mut h, "the deleted message", |h| h.exists("embed-gone"));
    assert_eq!(h.label("embed-gone"), "The record shown here was deleted.");
    h.click("embed-show-first");
    wait_until(&mut h, "the first part again", |h| name_in(h, "widget:Edit").as_deref() == Some("Part 01"));
    let rows = crate::execute_read_query(label, "SELECT qty FROM parts WHERE id = 1".into()).expect("query");
    assert_eq!(crate::ui::testing::shown(&rows.rows[0][0]), "11");
}

fn fill_in(h: &mut Harness, scope: &str, id: &str, text: &str) {
    h.click_in(scope.to_string(), id.to_string());
    h.select_all();
    h.press("backspace");
    if !text.is_empty() {
        h.input(text);
    }
}

fn filter_dashboard(name: &str, filters: serde_json::Value, components: serde_json::Value) -> serde_json::Value {
    json!({"id": uid(), "name": name, "filters": filters, "components": components})
}

fn bin_filter(default: Option<&str>) -> serde_json::Value {
    let mut filter = json!({"id": uid(), "name": "Bin", "param": "bin", "control": "select", "options": ["A", "B"]});
    if let Some(default) = default {
        filter["default"] = json!(default);
    }
    filter
}

/// The "Bin parts" query form (list and detail) over `bin_query`; returns (form id, JSON).
fn bin_parts_form(h: &mut Harness, controls: Vec<serde_json::Value>) -> String {
    let (query, form) = (uid(), uid());
    let saved = bin_query(&query);
    super::test_support::edit_store_query(h, saved);
    define(
        h,
        vec![json!({
            "id": form, "name": "Bin parts",
            "source": {"kind": "query", "queryId": query, "params": {"bin": "params.bin"}},
            "modes": ["list", "detail"], "listColumns": ["id", "name"], "pageSize": 25, "controls": controls,
        })],
        vec![],
    );
    form
}

#[gpui_kit::test]
fn offers_record_id_only_for_table_sourced_embedded_forms(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    seed_parts(&mut h);
    let query = uid();
    super::test_support::edit_store_query(&mut h, bin_query(&query));
    let (part, bin) = (uid(), uid());
    define(
        &mut h,
        vec![
            json!({"id": part, "name": "Part", "source": {"kind": "table", "table": "parts"}, "modes": ["detail"],
                   "controls": [field("Part name", "name", "text", 1)]}),
            json!({"id": bin, "name": "Bin parts", "source": {"kind": "query", "queryId": query, "params": {"bin": "'A'"}},
                   "modes": ["detail"], "controls": [field("Part name", "name", "text", 1)]}),
        ],
        vec![],
    );
    super::test_support::new_dashboard(&mut h);
    h.click("add-form");
    choose(&mut h, "prop-form", "Part");
    h.fill("prop-record-id", "params.id");
    assert_eq!(super::test_support::component(&h, "Form 1").record_id.as_deref(), Some("params.id"));
    choose(&mut h, "prop-form", "Bin parts");
    assert!(!h.exists("prop-record-id"));
    assert_eq!(super::test_support::component(&h, "Form 1").record_id, None);
}

#[gpui_kit::test]
fn keeps_dashboard_embedded_forms_inside_the_forms_row_filter(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    seed_parts(&mut h);
    let form = uid();
    let part_filter = json!({"id": uid(), "name": "Part id", "param": "part", "control": "number", "default": 3});
    define(
        &mut h,
        vec![json!({
            "id": form, "name": "Part", "source": {"kind": "table", "table": "parts"}, "filter": "record.bin = 'B'",
            "modes": ["detail", "edit"],
            "controls": [field("Part name", "name", "text", 1), field("Bin", "bin", "text", 2)],
        })],
        vec![filter_dashboard(
            "Bin B",
            json!([part_filter]),
            json!([
                {"id": uid(), "kind": "form", "title": "Pinned", "formId": form, "mode": "edit", "recordId": "params.part", "placement": at(1, 1, 6)},
                {"id": uid(), "kind": "form", "title": "First", "formId": form, "mode": "detail", "placement": at(7, 1, 6)},
            ]),
        )],
    );
    open_view(&mut h);
    wait_until(&mut h, "the row filter message", |h| h.exists_in("widget:Pinned", "embed-outside"));
    assert_eq!(h.label_in("widget:Pinned", "embed-outside"), "This record is outside the form's filter.");
    assert!(!h.exists_in("widget:Pinned", "field:part-name"));
    wait_until(&mut h, "the first filtered part", |h| name_in(h, "widget:First").as_deref() == Some("Part 41"));

    fill_in(&mut h, "dash-view", "filter:Part id", "45");
    super::test_support::advance_by(&mut h, 300);
    wait_until(&mut h, "part 45", |h| name_in(h, "widget:Pinned").as_deref() == Some("Part 45"));
    fill_in(&mut h, "widget:Pinned", "field:bin", "A");
    h.click_in("widget:Pinned", "save");
    wait_until(&mut h, "the row filter message again", |h| h.exists_in("widget:Pinned", "embed-outside"));
    assert_eq!(name_in(&mut h, "widget:First").as_deref(), Some("Part 41"));
}

#[gpui_kit::test]
fn says_when_a_form_is_missing_cannot_open_in_a_mode_or_has_no_record(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    h.create_table("parts", &[("id", "INTEGER", 1), ("name", "TEXT", 0), ("bin", "TEXT", 0), ("qty", "INTEGER", 0)]);
    let form = uid();
    define(
        &mut h,
        vec![json!({"id": form, "name": "Part", "source": {"kind": "table", "table": "parts"}, "modes": ["list", "detail"],
                    "controls": [field("Part name", "name", "text", 1)]})],
        vec![filter_dashboard(
            "Forms",
            json!([]),
            json!([
                {"id": uid(), "kind": "form", "title": "Gone", "formId": uid(), "mode": "list", "placement": at(1, 1, 4)},
                {"id": uid(), "kind": "form", "title": "Edit", "formId": form, "mode": "edit", "placement": at(5, 1, 4)},
                {"id": uid(), "kind": "form", "title": "Empty", "formId": form, "mode": "detail", "placement": at(9, 1, 4)},
                {"id": uid(), "kind": "form", "title": "Nothing", "mode": "list", "placement": at(1, 3, 4)},
            ]),
        )],
    );
    open_view(&mut h);
    wait_until(&mut h, "the empty message", |h| h.exists_in("widget:Empty", "embed-empty"));
    assert_eq!(h.label_in("widget:Gone", "embed-missing"), "This form does not exist.");
    assert_eq!(h.label_in("widget:Edit", "embed-unsupported"), "Part cannot open in edit mode.");
    assert_eq!(h.label_in("widget:Empty", "embed-empty"), "No record to show.");
    assert_eq!(h.label_in("widget:Nothing", "gate:Nothing"), "Choose a form.");
}

#[gpui_kit::test]
fn pages_a_parameterized_query_form_with_dashboard_filter_values_as_params(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    seed_parts(&mut h);
    let form = bin_parts_form(&mut h, vec![]);
    define(
        &mut h,
        vec![],
        vec![filter_dashboard(
            "Stock",
            json!([bin_filter(Some("A"))]),
            json!([{"id": uid(), "kind": "form", "title": "Parts", "formId": form, "mode": "list", "placement": at(1, 1, 12)}]),
        )],
    );
    open_view(&mut h);
    let status = |h: &mut Harness| h.try_label_in("widget:Parts", "pager-status");
    wait_until(&mut h, "page 1", |h| status(h).as_deref() == Some("1–25 of 40"));
    h.click_in("widget:Parts", "next-page");
    wait_until(&mut h, "page 2", |h| status(h).as_deref() == Some("26–40 of 40"));
    wait_until(&mut h, "row 40", |h| h.try_label_in("widget:Parts", "row:14").as_deref() == Some("Open 40"));
    h.click_in("widget:Parts", "sort:id");
    h.click_in("widget:Parts", "sort:id");
    wait_until(&mut h, "descending order", |h| h.try_label_in("widget:Parts", "row:0").as_deref() == Some("Open 40"));
    assert_eq!(status(&mut h).as_deref(), Some("1–25 of 40"));
    h.click_in("widget:Parts", "search-column");
    h.click_in("widget:Parts", "search-column:option:name");
    fill_in(&mut h, "widget:Parts", "search", "part 3");
    wait_until(&mut h, "ten matches", |h| status(h).as_deref() == Some("1–10 of 10"));

    choose(&mut h, "filter:Bin", "B");
    super::test_support::advance_by(&mut h, 300);
    // The list restarts with its default order and no search when the parameters change.
    wait_until(&mut h, "bin B", |h| status(h).as_deref() == Some("1–20 of 20"));
    wait_until(&mut h, "row 41 first", |h| h.try_label_in("widget:Parts", "row:0").as_deref() == Some("Open 41"));
}

#[gpui_kit::test]
fn edits_a_record_through_an_embedded_form_and_refreshes_the_kpi(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    seed_parts(&mut h);
    let (query, form) = (uid(), uid());
    super::test_support::edit_store_query(
        &mut h,
        crate::archive::SavedQuery {
            id: query.clone(),
            name: "Total quantity".into(),
            sql: "SELECT sum(qty) AS total FROM parts WHERE ($bin IS NULL OR bin = $bin)".into(),
            parameters: vec![crate::archive::QueryParameter {
                name: "bin".into(),
                logical_type: "text".into(),
                default_value: None,
                required: false,
            }],
            ..Default::default()
        },
    );
    let part_filter = json!({"id": uid(), "name": "Part id", "param": "part", "control": "number", "default": 3});
    define(
        &mut h,
        vec![json!({
            "id": form, "name": "Part", "source": {"kind": "table", "table": "parts"}, "modes": ["detail", "edit"],
            "controls": [field("Quantity", "qty", "number", 1)],
        })],
        vec![filter_dashboard(
            "Stock",
            json!([bin_filter(None), part_filter]),
            json!([
                {"id": uid(), "kind": "kpi", "title": "Units", "queryId": query, "valueField": "total", "placement": at(1, 1, 6)},
                {"id": uid(), "kind": "form", "title": "Edit part", "formId": form, "mode": "edit", "recordId": "params.part", "placement": at(7, 1, 6)},
            ]),
        )],
    );
    open_view(&mut h);
    wait_until(&mut h, "the total", |h| h.try_label(id("kpi-value:Units")).as_deref() == Some("60"));
    wait_until(&mut h, "the quantity", |h| value_in(h, "widget:Edit part", "field:quantity").as_deref() == Some("1"));
    h.fill("field:quantity", "11");
    h.click_in("widget:Edit part", "save");
    wait_until(&mut h, "the new total", |h| h.try_label(id("kpi-value:Units")).as_deref() == Some("70"));
    let rows = crate::execute_read_query(h.label.clone(), "SELECT qty FROM parts WHERE id = 3".into()).expect("query");
    assert_eq!(crate::ui::testing::shown(&rows.rows[0][0]), "11");
}

#[gpui_kit::test]
fn keeps_query_forms_off_record_ids_and_restarts_paging_when_filter_params_change(cx: &mut TestAppContext) {
    let _guard = with_real_runner();
    let mut h = tall_document(cx);
    seed_parts(&mut h);
    let form = bin_parts_form(&mut h, vec![field("Part name", "name", "text", 1)]);
    let (action, home) = (uid(), uid());
    let open_by_id = serde_json::from_value(json!({
        "id": action, "name": "Open by id", "onError": "stop",
        "steps": [{"id": uid(), "kind": "openForm", "formId": form, "mode": "detail", "recordId": "3"}],
    }))
    .expect("action");
    super::test_support::edit_store(&mut h, move |c| c.actions = vec![open_by_id]);
    define(
        &mut h,
        vec![],
        vec![json!({
            "id": home, "name": "Home", "filters": [bin_filter(Some("A"))],
            "components": [
                {"id": uid(), "kind": "form", "title": "List", "formId": form, "mode": "list", "placement": at(1, 1, 6)},
                {"id": uid(), "kind": "form", "title": "First", "formId": form, "mode": "detail", "recordId": "params.bin", "placement": at(7, 1, 6)},
                {"id": uid(), "kind": "button", "label": "Open by id", "actionId": action, "placement": at(1, 3, 6)},
            ],
        })],
    );
    open_in_runtime(&mut h, &home);
    wait_until(&mut h, "the first part", |h| name_in(h, "widget:First").as_deref() == Some("Part 01"));
    let status = |h: &mut Harness| h.try_label_in("widget:List", "pager-status");
    wait_until(&mut h, "page 1", |h| status(h).as_deref() == Some("1–25 of 40"));
    h.click_in("widget:List", "next-page");
    wait_until(&mut h, "page 2", |h| status(h).as_deref() == Some("26–40 of 40"));
    choose(&mut h, "filter:Bin", "B");
    super::test_support::advance_by(&mut h, 300);
    wait_until(&mut h, "bin B", |h| status(h).as_deref() == Some("1–20 of 20"));
    assert!(!super::test_support::list_can_prev(&mut h, "List"));

    h.click("dash-button:Open by id");
    wait_until(&mut h, "the refusal", |h| h.exists("status:error"));
    assert_eq!(h.label("status:error"), "This form shows query rows, so it cannot open a record by id.");
}

#[gpui_kit::test]
fn opens_a_dashboard_from_an_action_with_params_that_reach_filters_query_forms_and_first_row_binding(
    cx: &mut TestAppContext,
) {
    let _guard = with_real_runner();
    let mut h = tall_document(cx);
    seed_parts(&mut h);
    let form = bin_parts_form(&mut h, vec![field("Part name", "name", "text", 1)]);
    let (home, bin_view, action) = (uid(), uid(), uid());
    let show = serde_json::from_value(json!({
        "id": action, "name": "Show bin B", "onError": "stop",
        "steps": [{"id": uid(), "kind": "openDashboard", "dashboardId": bin_view, "params": {"bin": "'B'"}}],
    }))
    .expect("action");
    super::test_support::edit_store(&mut h, move |c| c.actions = vec![show]);
    define(
        &mut h,
        vec![],
        vec![
            json!({"id": home, "name": "Home", "components": [
                {"id": uid(), "kind": "button", "label": "Show bin B", "actionId": action, "placement": at(1, 1, 6)}]}),
            json!({"id": bin_view, "name": "Bin view", "filters": [bin_filter(None)], "components": [
                {"id": uid(), "kind": "form", "title": "List", "formId": form, "mode": "list", "placement": at(1, 1, 6)},
                {"id": uid(), "kind": "form", "title": "First", "formId": form, "mode": "detail", "placement": at(7, 1, 6)}]}),
        ],
    );
    open_in_runtime(&mut h, &home);
    h.click("dash-button:Show bin B");
    wait_until(&mut h, "the Bin filter", |h| h.try_label(id("filter:Bin")).as_deref() == Some("B"));
    wait_until(&mut h, "bin B", |h| h.try_label_in("widget:List", "pager-status").as_deref() == Some("1–20 of 20"));
    wait_until(&mut h, "the first part of bin B", |h| name_in(h, "widget:First").as_deref() == Some("Part 41"));
}
