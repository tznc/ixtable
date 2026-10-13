//! Golden-template Runtime flows (port of tests/integration/runtime-ux.test.tsx,
//! runtime-polish.test.tsx and runtime-list-open-race.test.tsx). The steps that run actions
//! (Start work, Post transfer, Print sheet) need the Automation runner and are not ported
//! here. Imports are explicit.

use super::tests::{nav, renderer, wait};
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;
use serde_json::json;

fn from_template<'a>(cx: &'a mut TestAppContext, id: &str) -> Harness<'a> {
    let mut h = Harness::start(cx);
    h.click(format!("template:{id}"));
    assert!(h.has_shell(), "the template did not open");
    h.switch_mode(ModeId::Run);
    wait(&mut h);
    h
}

fn open_page(h: &mut Harness, label: &str) {
    let id = nav(h, label);
    h.click(id);
    wait(h);
}

/// The index of the list row labelled "Open <name>".
fn row_of(h: &mut Harness, name: &str) -> usize {
    (0..25)
        .find(|n| h.try_label(format!("row:{n}")).as_deref() == Some(&format!("Open {name}")))
        .unwrap_or_else(|| panic!("no row for {name}"))
}

fn open_row(h: &mut Harness, name: &str) {
    let n = row_of(h, name);
    h.click(format!("row:{n}"));
    wait(h);
}

#[gpui_kit::test]
fn record_pages_have_one_back_link_and_yes_no_cells(cx: &mut TestAppContext) {
    let mut h = from_template(cx, "work-orders");
    open_page(&mut h, "Work orders");
    open_row(&mut h, "WO-1001");

    assert_eq!(h.label("back-crumb"), "Back to Work orders");
    assert!(h.exists("edit"));
    assert!(h.exists("delete"));

    let done: Vec<String> = (0..4)
        .map(|n| h.try_label(format!("related:tasks:cell:{n}:done")).unwrap_or_default())
        .collect();
    assert_eq!(done, ["Yes", "Yes", "No", "No"]);

    // Back returns to the list and the link goes away.
    h.click("back-crumb-button");
    wait(&mut h);
    assert!(h.exists("row:0"));
    assert!(!h.exists("back-crumb"));
    open_row(&mut h, "WO-1002");
    assert!(h.exists("back-crumb"));
}

#[gpui_kit::test]
fn runtime_lists_render_logical_booleans_as_yes_no(cx: &mut TestAppContext) {
    let mut h = from_template(cx, "inventory");
    open_page(&mut h, "Products");
    let _ = row_of(&mut h, "BOLT-M8");
    let cells: Vec<String> = (0..25)
        .filter_map(|n| h.try_label(format!("cell:{n}:active")))
        .collect();
    assert!(!cells.is_empty());
    assert!(cells.iter().all(|c| c == "Yes" || c == "No"), "{cells:?}");
    assert!(cells.iter().any(|c| c == "Yes"));
}

#[gpui_kit::test]
fn re_choosing_the_open_page_returns_to_its_list_and_relationship_labels_render(
    cx: &mut TestAppContext,
) {
    let mut h = from_template(cx, "inventory");
    open_page(&mut h, "Products");
    open_row(&mut h, "BOLT-M8");
    assert!(h.exists("image:product-photo"));
    assert!(h.label("image:product-photo:state").len() < 40);
    // The thresholds show location names, not ids.
    assert_eq!(h.label("related:reorder-thresholds:cell:0:location-id"), "Main warehouse");
    let cells: Vec<String> = (0..4)
        .filter_map(|n| h.try_label(format!("related:reorder-thresholds:cell:{n}:location-id")))
        .collect();
    assert!(cells.contains(&"Downtown store".to_string()), "{cells:?}");
    assert!(!cells.contains(&"1".to_string()));

    open_page(&mut h, "Products");
    assert!(h.exists("row:0"));
    assert!(!h.exists("back-crumb"));
    let page = renderer(&mut h);
    assert!(h.cx.read(|cx| page.read(cx).list().is_some()));
    drop(page);

    open_page(&mut h, "Reorder thresholds");
    let n = row_of(&mut h, "Hex nut M8");
    assert!(n < 25);
    let airport = (0..25)
        .filter(|n| h.try_label(format!("cell:{n}:location-id")).as_deref() == Some("Airport store"))
        .count();
    assert_eq!(airport, 1);
}

#[gpui_kit::test]
fn opens_the_clicked_record_when_its_schema_reloads_after_a_database_change(
    cx: &mut TestAppContext,
) {
    let mut h = from_template(cx, "work-orders");
    open_page(&mut h, "Work orders");
    open_row(&mut h, "WO-1001");
    assert!(h.exists("record-title"));

    // A database change drops the cached schema while the user goes back and opens another.
    h.database_changed();
    h.click("back-crumb-button");
    open_row(&mut h, "WO-1002");
    wait(&mut h);
    assert!(!h.exists("status:error"));
    let page = renderer(&mut h);
    let record = h.cx.read(|cx| page.read(cx).record()).expect("a record view");
    let number = h.cx.read(|cx| record.read(cx).record().get("number").cloned());
    assert_eq!(number, Some(json!("WO-1002")));
}

#[gpui_kit::test]
fn replaces_the_legacy_main_ids_with_uuid_v7_on_open(cx: &mut TestAppContext) {
    let h = Harness::new_document(cx);
    let is_v7 = |id: &str| {
        id.len() == 36 && id.as_bytes()[14] == b'7' && matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b')
    };
    let mut config = h.config();
    assert!(is_v7(&config.design.forms[0].id));
    config.design.forms[0].id = "main".into();
    config.design.navigation[0].id = "main".into();
    config.design.navigation[0].target_id = Some("main".into());
    config.design.start_page = Some("main".into());
    config.roles = serde_json::from_value(json!([{
        "id": "r", "name": "Clerk",
        "permissions": {
            "navigation": ["main"], "actions": [],
            "objects": [{"kind": "form", "id": "main", "read": true}],
        },
    }]))
    .expect("roles");
    crate::update_document_config(h.label.clone(), config).expect("update config");
    let path = h.temp_path("legacy-main.ixt");
    crate::save_document_as(h.label.clone(), path.to_string_lossy().into()).expect("save");
    crate::open_document(h.label.clone(), path.to_string_lossy().into()).expect("open");

    let opened = h.config();
    let form = opened.design.forms[0].id.clone();
    let item = &opened.design.navigation[0];
    assert!(is_v7(&form));
    assert!(is_v7(&item.id));
    assert_eq!(item.target_id.as_deref(), Some(form.as_str()));
    assert_eq!(opened.design.start_page.as_deref(), Some(item.id.as_str()));
    let permissions = &opened.roles[0].permissions;
    assert_eq!(permissions.navigation, [item.id.clone()]);
    assert_eq!(permissions.objects[0].id, form);
    let yaml = crate::read_document_config_yaml(h.label.clone()).expect("yaml");
    assert!(!yaml.lines().any(|l| l.trim_end().ends_with("id: main")), "{yaml}");
}
