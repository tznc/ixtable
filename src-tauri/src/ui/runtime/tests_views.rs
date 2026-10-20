//! Form view flows (port of tests/integration/form-views.test.tsx): continuous forms, split
//! forms with the record navigation bar, popups opened by actions. The React tests wrote the
//! design to the session and reopened the document; these seed it through the ConfigStore.
//! Imports are explicit: `gpui_kit::*` would shadow `#[test]`.

use super::tests::{
    at, grid, renderer, seed, seed_tasks, task_form, title_field, uid, wait, with_runner,
};
use crate::data::DataValue;
use crate::design::FormMode;
use crate::forms::actions::{ActionResult, NavigationTarget, NoticeTone};
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;
use serde_json::{Map, Value, json};

fn titles(h: &Harness) -> Vec<String> {
    h.read_page("tasks")
        .rows
        .iter()
        .map(|row| match &row[1] {
            DataValue::Text(text) => text.clone(),
            other => format!("{other:?}"),
        })
        .collect()
}

fn open_tasks(h: &mut Harness) {
    h.switch_mode(ModeId::Run);
    wait(h);
}

#[gpui_kit::test]
fn edits_adds_and_deletes_records_in_place_on_a_continuous_form(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_tasks(&mut h, &["Draft plan", "Review budget"]);
    task_form(&mut h, json!({"modes": ["continuous", "create", "edit"]}));
    open_tasks(&mut h);
    assert!(h.exists("crow:1"));
    assert!(h.exists("crow:2"));
    assert!(h.exists("crow:new"));
    // Labels appear once as column headers, not on every row.
    assert!(h.exists("continuous-header"));
    assert!(!h.exists("record-1:field:title:label"));
    assert!(!h.exists("record-new:field:title:label"));

    // Edit in place: Save appears once the row changes.
    assert!(!h.exists("crow:1:save"));
    h.fill("record-1:field:title", "Final plan");
    assert!(h.exists("crow:1:save"));
    h.click("crow:1:save");
    wait(&mut h);
    assert_eq!(h.label("notice"), "Changes saved.");
    assert_eq!(titles(&h), ["Final plan", "Review budget"]);

    // Add through the new-record row.
    h.fill("record-new:field:title", "Ship release");
    h.click("crow:new:save");
    wait(&mut h);
    assert_eq!(h.label("notice"), "Record created.");
    assert!(h.exists("crow:3"));
    assert_eq!(titles(&h), ["Final plan", "Review budget", "Ship release"]);

    h.dialogs.queue_confirm(true);
    h.click("crow:3:delete");
    wait(&mut h);
    assert_eq!(h.label("notice"), "Record deleted.");
    assert_eq!(titles(&h), ["Final plan", "Review budget"]);
}

#[gpui_kit::test]
fn a_continuous_row_validates_required_fields_before_saving(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_tasks(&mut h, &["Draft plan"]);
    task_form(&mut h, json!({"modes": ["continuous", "create", "edit"]}));
    open_tasks(&mut h);
    h.type_into("record-new:field:title", "x");
    h.fill("record-new:field:title", "");
    h.click("crow:new:save");
    wait(&mut h);
    assert_eq!(titles(&h), ["Draft plan"]);
    assert!(h.exists("record-new:field:title:error"));
}

#[gpui_kit::test]
fn shows_the_chosen_row_of_a_split_form_and_steps_with_the_navigation_bar(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_tasks(&mut h, &["Alpha", "Bravo", "Charlie"]);
    task_form(
        &mut h,
        json!({"modes": ["split", "detail", "create", "edit"], "navigationBar": true}),
    );
    open_tasks(&mut h);
    assert_eq!(h.label("split-hint"), "Choose a record to see it here.");
    h.click("row:1");
    wait(&mut h);
    let page = renderer(&mut h);
    let split = h.cx.read(|cx| page.read(cx).split()).expect("a split view");
    assert_eq!(h.label("nav-position"), "Record 2 of 3");
    let record = h.cx.read(|cx| split.read(cx).record().cloned()).expect("the detail pane");
    assert_eq!(h.cx.read(|cx| record.read(cx).record().get("title").cloned()), Some(json!("Bravo")));

    h.click("nav-next");
    wait(&mut h);
    assert_eq!(h.label("nav-position"), "Record 3 of 3");
    let bar = h.cx.read(|cx| split.read(cx).nav_bar()).expect("a navigation bar");
    assert!(h.cx.read(|cx| !bar.read(cx).can_next() && !bar.read(cx).can_last()));
    let record = h.cx.read(|cx| split.read(cx).record().cloned()).expect("the detail pane");
    assert_eq!(h.cx.read(|cx| record.read(cx).record().get("title").cloned()), Some(json!("Charlie")));

    // Editing in the detail pane refreshes the datasheet above it.
    h.click("edit");
    wait(&mut h);
    h.fill("field:title", "Charlie two");
    let bar = h.cx.read(|cx| split.read(cx).nav_bar()).expect("a navigation bar");
    assert!(h.cx.read(|cx| !bar.read(cx).can_first()));
    h.click("save");
    wait(&mut h);
    assert!(h.exists("row:2"));
    assert_eq!(h.label("row:2"), "Open Charlie two");

    h.click("nav-first");
    wait(&mut h);
    assert_eq!(h.label("nav-position"), "Record 1 of 3");
}

#[gpui_kit::test]
fn the_navigation_bar_follows_the_lists_sort_order(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_tasks(&mut h, &["Alpha", "Bravo", "Charlie"]);
    task_form(
        &mut h,
        json!({"modes": ["list", "detail", "create", "edit"], "navigationBar": true}),
    );
    open_tasks(&mut h);
    assert_eq!(h.label("row:0"), "Open Alpha");
    h.click("sort:title");
    wait(&mut h);
    h.click("sort:title");
    wait(&mut h);
    assert_eq!(h.label("row:0"), "Open Charlie");
    h.click("row:0");
    wait(&mut h);
    assert_eq!(h.label("nav-position"), "Record 1 of 3");
    h.click("nav-next");
    wait(&mut h);
    let page = renderer(&mut h);
    let record = h.cx.read(|cx| page.read(cx).record()).expect("a record view");
    assert_eq!(h.cx.read(|cx| record.read(cx).record().get("title").cloned()), Some(json!("Bravo")));
    h.click("nav-new");
    wait(&mut h);
    let page = renderer(&mut h);
    assert_eq!(h.cx.read(|cx| page.read(cx).mode()), FormMode::Create);
}

/// A Task form (detail, create, edit) with a "Use this task" button, a "Planner" launcher
/// form with an "Add a task" button, and the two actions (run by the test runner double).
fn popup_design(h: &mut Harness, add: String, pick: String) -> String {
    let button = |label: &str, action: &str, row: u32| {
        json!({
            "id": uid(), "kind": "button", "label": label, "validation": {"required": false},
            "placement": at(row), "actionId": action,
        })
    };
    task_form(
        h,
        json!({
            "modes": ["detail", "create", "edit"],
            "controls": [title_field(), button("Use this task", &pick, 2)],
        }),
    );
    let task = h.config().design.forms[0].id.clone();
    let launcher = uid();
    let planner = json!({
        "id": launcher, "name": "Planner", "source": null, "modes": ["create"], "layout": grid(),
        "listColumns": [], "pageSize": 25, "rules": [], "controls": [button("Add a task", &add, 1)],
    });
    seed(h, move |config| {
        config.design.forms.push(serde_json::from_value(planner).expect("planner form"));
        config.design.navigation[0].kind = crate::design::NavKind::Form;
        config.design.navigation[0].target_id = Some(launcher);
        config.design.navigation[0].mode = Some(FormMode::Create);
        for (id, name) in [(add, "Add a task"), (pick, "Use this task")] {
            config.actions.push(
                serde_json::from_value(json!({"id": id, "name": name, "onError": "stop", "steps": []}))
                    .expect("action"),
            );
        }
    });
    task
}

fn popup(form: &str, mode: &str, record_id: Option<Value>) -> NavigationTarget {
    NavigationTarget {
        kind: "form".into(),
        id: form.into(),
        mode: Some(mode.into()),
        record_id,
        params: Map::new(),
    }
}

#[gpui_kit::test]
fn opens_a_popup_form_from_an_action_and_uses_what_it_returns(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_tasks(&mut h, &["Alpha"]);
    let (add, pick) = (uid(), uid());
    let task = popup_design(&mut h, add.clone(), pick.clone());
    let form = task.clone();
    let _runner = with_runner(move |action, ctx| {
        if action == add {
            let created = ctx.host.open_popup(&popup(&form, "create", None)).unwrap_or(Value::Null);
            ctx.host.notify(&format!("Added {}", created["title"].as_str().unwrap_or("")), NoticeTone::Info);
            let picked = ctx
                .host
                .open_popup(&popup(&form, "detail", Some(created["id"].clone())))
                .unwrap_or(Value::Null);
            ctx.host.notify(&format!("Picked {}", picked.as_str().unwrap_or("")), NoticeTone::Info);
        } else if action == pick {
            let title = ctx.record.as_ref().and_then(|r| r.get("title")).and_then(Value::as_str).unwrap_or("");
            ctx.host.close_form(json!(title.to_uppercase()));
        }
        ActionResult::success()
    });
    open_tasks(&mut h);
    h.click("button:add-a-task");
    wait(&mut h);
    assert!(h.exists("popup:1"));
    h.fill("field:title", "Write docs");
    h.click_in("popup:1", "create");
    wait(&mut h);

    // The saved record comes back; the next step opens it in a second popup.
    assert!(!h.exists("popup:1"));
    assert!(h.exists("popup:2"));
    h.click("button:use-this-task");
    wait(&mut h);
    assert!(!h.exists("popup:2"));
    assert_eq!(h.label("notice"), "Picked WRITE DOCS");
    assert_eq!(titles(&h), ["Alpha", "Write docs"]);
}

#[gpui_kit::test]
fn a_dismissed_popup_returns_null_and_keeps_the_opener_usable(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_tasks(&mut h, &["Alpha"]);
    let (add, pick) = (uid(), uid());
    let task = popup_design(&mut h, add.clone(), pick);
    let _runner = with_runner(move |action, ctx| {
        if action == add {
            let created = ctx.host.open_popup(&popup(&task, "create", None)).unwrap_or(Value::Null);
            let text = if created.is_null() { "Nothing added" } else { "Added" };
            ctx.host.notify(text, NoticeTone::Info);
        }
        ActionResult::success()
    });
    open_tasks(&mut h);
    h.click("button:add-a-task");
    wait(&mut h);
    assert!(h.exists("popup:1"));
    // A create popup has Cancel and no second Close button.
    assert!(!h.exists("popup-close:1"));
    h.click("cancel");
    wait(&mut h);
    assert_eq!(h.label("notice"), "Nothing added");
    assert!(!h.exists("popup:1"));
    assert_eq!(titles(&h), ["Alpha"]);
}

#[gpui_kit::test]
fn saves_a_continuous_row_when_save_is_pressed_and_keeps_edits_in_other_rows(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_tasks(&mut h, &["Draft plan", "Review budget"]);
    task_form(&mut h, json!({"modes": ["continuous", "create", "edit"]}));
    open_tasks(&mut h);
    h.fill("record-2:field:title", "Approve budget");
    h.fill("record-new:field:title", "Hire help");
    h.click("crow:2:save");
    wait(&mut h);
    assert_eq!(h.label("notice"), "Changes saved.");
    assert_eq!(titles(&h), ["Draft plan", "Approve budget"]);
    assert!(!h.exists("crow:2:save"));
    // The new-record row keeps what was typed in it.
    assert!(h.exists("crow:new:save"));
    h.click("crow:new:save");
    wait(&mut h);
    assert_eq!(titles(&h), ["Draft plan", "Approve budget", "Hire help"]);
}

#[gpui_kit::test]
fn the_bar_reads_record_n_of_n_after_a_record_is_created(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_tasks(&mut h, &["Alpha", "Bravo", "Charlie"]);
    task_form(&mut h, json!({"modes": ["split", "detail", "create", "edit"], "navigationBar": true}));
    open_tasks(&mut h);
    h.click("row:0");
    wait(&mut h);
    h.click("nav-new");
    wait(&mut h);
    h.fill("field:title", "Delta");
    h.click("create");
    wait(&mut h);
    assert_eq!(h.label("nav-position"), "Record 4 of 4");
}
