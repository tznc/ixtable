//! Runtime flows (port of tests/integration/guided-crud.test.tsx and runtime-ux.test.tsx).
//! Forms come from the generate logic (the React tests built them in the Design studio).
//! Imports are explicit: `gpui_kit::*` would shadow `#[test]`.

use super::{FormRenderer, Runtime, RunBody, RunMode};
use crate::data::DataValue;
use crate::forms::actions::{ActionContext, ActionResult, ActionRunner, set_action_runner};
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use gpui_kit::{Entity, TestAppContext};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

pub(super) fn customers(h: &mut Harness) {
    h.create_table("customers", &[("id", "INTEGER", 1), ("name", "TEXT", 0)]);
}

/// The id of the navigation button labelled `label`.
pub(super) fn nav(h: &mut Harness, label: &str) -> String {
    let config = h.config();
    let item = config
        .design
        .navigation
        .iter()
        .find(|n| n.label == label)
        .unwrap_or_else(|| panic!("no navigation item {label}"));
    format!("nav:{}", item.id)
}

pub(super) fn generate(h: &mut Harness) {
    h.switch_mode(ModeId::Run);
    h.click("generate-app");
}

pub(super) fn uid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Applies a definition edit through the document's ConfigStore.
pub(super) fn seed(h: &mut Harness, mutate: impl FnOnce(&mut crate::archive::DocumentConfig) + 'static) {
    let store = h.read_shell(|s, _| s.store.clone());
    h.cx.update(|cx| {
        store
            .update(cx, |s, cx| s.update("Seed design", mutate, cx))
            .expect("seed the design");
    });
    h.settle();
}

pub(super) fn grid() -> serde_json::Value {
    serde_json::json!({
        "columns": (0..12).map(|_| serde_json::json!({"kind": "fr", "value": 1})).collect::<Vec<_>>(),
        "rows": [], "columnGap": 16, "rowGap": 16, "padding": 0,
        "justifyItems": "stretch", "alignItems": "stretch", "namedRegions": [], "breakpoints": [],
    })
}

pub(super) fn at(row: u32) -> serde_json::Value {
    serde_json::json!({"column": 1, "row": row, "columnSpan": 6, "rowSpan": 1})
}

pub(super) fn title_field() -> serde_json::Value {
    serde_json::json!({
        "id": uid(), "kind": "text", "label": "Title", "binding": {"column": "title"},
        "validation": {"required": true}, "placement": at(1),
    })
}

pub(super) fn seed_tasks(h: &mut Harness, titles: &[&str]) {
    h.create_table("tasks", &[("id", "INTEGER", 1), ("title", "TEXT", 0)]);
    for (index, title) in titles.iter().enumerate() {
        h.insert_row(
            "tasks",
            &[
                ("id", DataValue::Integer(index as i64 + 1)),
                ("title", DataValue::Text(title.to_string())),
            ],
        );
    }
}

/// Replaces the first form with a "Task" form over `tasks` (patched by `patch`) and points
/// the first navigation item ("Tasks") at it.
pub(super) fn task_form(h: &mut Harness, patch: serde_json::Value) {
    seed(h, move |config| {
        let id = config.design.forms[0].id.clone();
        let mut form = serde_json::json!({
            "id": id, "name": "Task", "source": {"kind": "table", "table": "tasks"},
            "layout": grid(), "listColumns": ["title"], "rules": [], "pageSize": 25,
            "controls": [title_field()], "modes": ["list", "detail", "create", "edit"],
        });
        for (key, value) in patch.as_object().expect("patch object") {
            form[key] = value.clone();
        }
        config.design.forms[0] = serde_json::from_value(form).expect("task form");
        config.design.navigation[0].label = "Tasks".into();
        config.design.navigation[0].target_id = Some(id);
        config.design.navigation[0].mode = None;
    });
}

/// The Run mode body of the open document.
pub(super) fn run_body(h: &mut Harness) -> Entity<RunBody> {
    let view = h.read_shell(|s, _| s.mode_view()).expect("a mode view");
    let mode: Entity<RunMode> = view.downcast().expect("RunMode");
    h.cx.read(|cx| mode.read(cx).body().cloned()).expect("the Run body is rendered")
}

/// The form renderer of the page shown.
pub(super) fn renderer(h: &mut Harness) -> Entity<FormRenderer> {
    let body = run_body(h);
    let view = h.cx.read(|cx| body.read(cx).page_view_entity()).expect("a page");
    view.downcast().expect("a FormRenderer page")
}

pub(super) fn runtime(h: &mut Harness) -> Entity<Runtime> {
    let body = run_body(h);
    h.cx.read(|cx| body.read(cx).runtime().clone())
}

type RunnerFn = Box<dyn Fn(&str, &ActionContext) -> ActionResult + Send + Sync>;

struct FnRunner(RunnerFn);

impl ActionRunner for FnRunner {
    fn run_action(&self, action_id: &str, ctx: &ActionContext) -> ActionResult {
        (self.0)(action_id, ctx)
    }
}

/// Keeps the process-wide action runner to one test at a time; clears it on drop.
pub(super) struct RunnerGuard(#[allow(dead_code)] MutexGuard<'static, ()>);

impl Drop for RunnerGuard {
    fn drop(&mut self) {
        set_action_runner(None);
    }
}

/// Installs a test-double action runner (the Automation port provides the real one).
pub(super) fn with_runner(
    run: impl Fn(&str, &ActionContext) -> ActionResult + Send + Sync + 'static,
) -> RunnerGuard {
    static LOCK: Mutex<()> = Mutex::new(());
    let guard = RunnerGuard(LOCK.lock().unwrap_or_else(|e| e.into_inner()));
    set_action_runner(Some(Arc::new(FnRunner(Box::new(run)))));
    guard
}

/// Lets background work finish: the test clock advances in steps, with real time between
/// them for the threads that run actions.
pub(super) fn wait(h: &mut Harness) {
    for _ in 0..16 {
        h.advance(Duration::from_millis(20));
        std::thread::sleep(Duration::from_millis(5));
        h.settle();
    }
}

#[gpui_kit::test]
fn offers_to_generate_the_app_from_a_runtime_with_no_pages(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    customers(&mut h);
    h.switch_mode(ModeId::Run);
    assert!(h.exists("rt-empty"));
    h.click("generate-app");
    wait(&mut h);
    let id = nav(&mut h, "Customers");
    assert!(h.exists(id));
    assert!(h.exists("new-record"));
    assert!(!h.exists("rt-empty"));
}

#[gpui_kit::test]
fn creates_a_record_through_the_generated_form(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    customers(&mut h);
    generate(&mut h);
    wait(&mut h);
    h.click("new-record");
    wait(&mut h);
    h.fill("field:name", "Acme");
    h.click("create");
    wait(&mut h);
    let page = h.read_page("customers");
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0][1], DataValue::Text("Acme".into()));
}
