//! Port of `tests/integration/golden/work-orders.test.tsx`: nested navigation, queries, the
//! printable sheet, the operations dashboard, the work-order lifecycle with sync and async
//! triggers, roles, and the v1 -> v2 bundle update.
//!
//! Through the backend instead of a screen that is not part of this port: the sheet and
//! dashboard (see `tests_crm.rs`), and in the migration journey the YAML upgrade
//! (`apply_document_config_yaml`) and applying the pending migration (`apply_migrations`).

use super::journey::*;
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use gpui_kit::{ScrollDelta, TestAppContext, point, px, test::TestWindowExt as _};
use serde_json::{Map, json};

const APP: &str = "work-orders";

fn trigger_id(label: &str) -> String {
    let config = crate::read_document_config(label.into()).expect("config");
    config.triggers.iter().find(|t| t.name == "Log new work orders in the background").expect("trigger").id.clone()
}

/// Waits until the async trigger's job has run once and succeeded.
fn job_done(h: &mut Harness) {
    let (label, id) = (h.label.clone(), trigger_id(&h.label));
    eventually(h, "the background job to succeed", |_| {
        let jobs: Vec<_> = crate::jobs::list_jobs(label.clone(), None).expect("jobs").into_iter().filter(|j| j.trigger_id == id).collect();
        jobs.len() == 1 && jobs[0].status == "succeeded" && jobs[0].attempts == 1
    });
}

#[gpui_kit::test]
fn golden_work_orders_nested_navigation_queries_printable_sheet_and_operations_dashboard(cx: &mut TestAppContext) {
    with_journey(APP, "reads", |journey| {
        let mut h = journey.step("Create Work orders from the start screen", move |_| start_from_template(cx, "work-orders"));
        let label = h.label.clone();
        journey.step("Validate definitions", |j| j.check("no error issues", validation_errors(&label), Vec::<String>::new()));
        journey.step("Nested navigation groups open reference tables", |j| {
            let config = h.config();
            let setup = config.design.navigation.iter().find(|n| n.label == "Setup").expect("Setup group");
            let reference = setup.children.iter().find(|n| n.label == "Reference data").expect("Reference data nested in Setup");
            assert!(reference.children.iter().any(|n| n.label == "Statuses"));
            let statuses = nav(&mut h, "Statuses");
            assert!(h.exists(statuses));
            open_page(&mut h, "Priorities");
            let _ = row_of(&mut h, "low");
            j.check("nesting depth", setup.children.iter().any(|n| n.id == reference.id), true);
        });
        journey.step("Grouped and filtered queries", |j| {
            j.check(
                "by status",
                saved_query(&label, "Work orders by status", &[]),
                json!([["Open", 3], ["In progress", 1], ["Done", 1], ["Cancelled", 0]]),
            );
            let open = saved_query(&label, "Open work orders", &[]);
            j.check(
                "open orders by priority",
                open.iter().map(|r| json!([r[0], r[3], r[6]])).collect::<Vec<_>>(),
                json!([["WO-1004", "Urgent", 0], ["WO-1001", "High", 50], ["WO-1002", "Normal", 0], ["WO-1005", "Low", 0]]),
            );
            j.check(
                "priority parameter",
                saved_query(&label, "Open work orders", &[("priority", json!("High"))]).iter().map(|r| r[0].clone()).collect::<Vec<_>>(),
                json!(["WO-1001"]),
            );
        });
        journey.step("Printable work-order sheet: page 1 of N, headers, footers, totals", |j| {
            let (studio, run) = host_report(&mut h, "Work order sheet", None);
            assert_eq!(h.label("report-params"), "Work order sheet parameters");
            assert_eq!(h.cx.read(|cx| run.read(cx).prompt_text("number", cx)).as_deref(), Some(""));
            h.click("params-run");
            let pages = report_pages(&mut h, &run);
            let first = &pages[0];
            let count = |text: &str| first.iter().filter(|t| t.as_str() == text).count();
            assert!(count("Work order sheet") >= 1, "{first:?}");
            assert_eq!(count("WO-1001 · Replace pump seal"), 1);
            assert_eq!(count("Total hours: 3.5"), 2);
            assert_eq!(count(&format!("Page 1 of {}", pages.len())), 1);
            assert_eq!(count("Technician signature: ____________"), 1);
            j.check(
                "report total on the last page",
                pages.last().unwrap().iter().any(|t| t == "All work orders: 7 hours"),
                true,
            );
            leave_report(&mut h, studio);
        });
        journey.step("Operations dashboard with priority filter", |j| {
            let mut dash = dashboard(&h, "Operations");
            assert_eq!(dash.kpi("Open work orders"), "4");
            assert_eq!(dash.kpi("Urgent"), "1");
            assert!(dash.options("Priority").iter().any(|o| o == "High"));
            dash.filter("Priority", "High");
            assert_eq!(dash.kpi("Open work orders"), "1");
            j.check("urgent within High", dash.kpi("Urgent"), "0");
        });
    });
}

fn order(label: &str, number: &str) -> serde_json::Value {
    json!(sql(label, &format!("SELECT status_code, progress, started_on IS NOT NULL, completed_on IS NOT NULL FROM work_orders WHERE number = '{number}'")))
}

fn events(label: &str, number: &str) -> serde_json::Value {
    json!(sql(label, &format!("SELECT l.event FROM work_order_log l JOIN work_orders w ON w.id = l.work_order_id WHERE w.number = '{number}' ORDER BY l.id")))
}

const TASKS: &str = "related:tasks:form";

fn add_task(h: &mut Harness, step: u32, description: &str, hours: &str) {
    h.click("related:tasks:add");
    wait(h);
    fill_in(h, TASKS, "field:step", &step.to_string());
    fill_in(h, TASKS, "field:description", description);
    fill_in(h, TASKS, "field:hours", hours);
    h.click_in(TASKS, "create");
    wait(h);
}

fn mark_task_done(h: &mut Harness, row: usize) {
    h.click(format!("related:tasks:edit:{row}"));
    wait(h);
    h.click_in(TASKS, "field:done");
    h.click_in(TASKS, "save");
    wait(h);
    assert!(!exists_in(h, "rt-form", TASKS), "saving the task failed: {}", messages(h));
}

fn press(h: &mut Harness, button: &str) {
    h.click(format!("button:{button}"));
    wait(h);
}

#[gpui_kit::test]
fn golden_work_orders_lifecycle_with_status_actions_sync_and_async_triggers_and_roles(cx: &mut TestAppContext) {
    // The action runner is process-wide: hold the real one so a parallel test's double cannot replace it.
    let _runner = crate::ui::runtime::tests::with_real_runner();
    with_journey(APP, "lifecycle", |journey| {
        let mut h = journey.step("Create Work orders from the start screen", move |_| start_from_template(cx, "work-orders"));
        let label = h.label.clone();

        journey.step("Create a work order; async trigger logs it in the background", |j| {
            show_list(&mut h, "Work orders");
            h.click("new-record");
            wait(&mut h);
            h.fill("field:number", "1006");
            h.fill("field:title", "Replace HVAC belt");
            choose_related(&mut h, "asset", "Rooftop HVAC unit");
            choose_related(&mut h, "priority", "High");
            h.click("create");
            wait(&mut h);
            assert_eq!(h.label("field:number:error"), "Use the form WO-1234.");
            h.fill("field:number", "WO-1006");
            h.click("create");
            wait(&mut h);
            assert!(h.exists("edit"), "opened as saved: {}", messages(&mut h));
            job_done(&mut h);
            j.check("new order", order(&label, "WO-1006"), json!([["open", 0, false, false]]));
            j.check("audit row written by the job", events(&label, "WO-1006"), json!([["created"]]));
            h.click("back-crumb-button");
            wait(&mut h);
            open_row(&mut h, "WO-1006");
        });

        journey.step("Start is refused until someone is assigned", |j| {
            press(&mut h, "start-work");
            expect_alert(&mut h, "Assign someone before starting this work order.");
            j.check("still open", order(&label, "WO-1006")[0][0].clone(), "open");
            h.click("edit");
            wait(&mut h);
            choose_related(&mut h, "assignee", "Tom Chen");
            h.click("save");
            wait(&mut h);
            assert!(h.exists("edit"));
        });

        journey.step("Start work: status transition and log in one transaction", |j| {
            press(&mut h, "start-work");
            eventually(&mut h, "the order to start", |_| order(&label, "WO-1006")[0][0] == "in_progress");
            j.check("log", events(&label, "WO-1006"), json!([["created"], ["started"]]));
        });

        journey.step("Tasks (one-level master/detail) drive progress via sync triggers", |j| {
            add_task(&mut h, 1, "Remove old belt", "1.5");
            assert!(!exists_in(&mut h, "rt-form", "related:tasks:form:related:tasks"));
            add_task(&mut h, 2, "Fit new belt", "2");
            assert_eq!(h.label("related:tasks:cell:1:description"), "Fit new belt");
            j.check("progress after adding tasks", order(&label, "WO-1006")[0][1].clone(), 0);
            mark_task_done(&mut h, 0);
            eventually(&mut h, "progress 50", |_| order(&label, "WO-1006")[0][1] == 50);
            j.check("progress after one of two tasks", order(&label, "WO-1006")[0][1].clone(), 50);
        });

        journey.step("Duplicate step number violates the composite unique key", |j| {
            add_task(&mut h, 2, "Duplicate step", "0");
            expect_alert(&mut h, "Unique constraint failed");
            h.click_in(TASKS, "cancel");
            wait(&mut h);
            j.check(
                "tasks",
                scalar(&label, "SELECT count(*) FROM tasks t JOIN work_orders w ON w.id = t.work_order_id WHERE w.number = 'WO-1006'"),
                2,
            );
        });

        journey.step("Complete is refused while a task is open", |j| {
            press(&mut h, "complete-work");
            expect_alert(&mut h, "Finish every task before completing: 1 still open.");
            j.check("still in progress", order(&label, "WO-1006")[0][0].clone(), "in_progress");
        });

        journey.step("Finish the last task and complete the work order", |j| {
            mark_task_done(&mut h, 1);
            eventually(&mut h, "progress 100", |_| order(&label, "WO-1006")[0][1] == 100);
            press(&mut h, "complete-work");
            eventually(&mut h, "the order to complete", |_| order(&label, "WO-1006")[0][0] == "done");
            j.check("final state", order(&label, "WO-1006"), json!([["done", 100, true, true]]));
            j.check("log", events(&label, "WO-1006"), json!([["created"], ["started"], ["completed"]]));
        });

        journey.step("Print sheet opens the report for this work order only", |j| {
            // The button navigates to the report page; the report is then hosted directly.
            press(&mut h, "print-sheet");
            let mut params = Map::new();
            params.insert("number".into(), json!("WO-1006"));
            let (studio, run) = host_report(&mut h, "Work order sheet", Some(params));
            let pages = report_pages(&mut h, &run);
            assert_eq!(pages.len(), 1);
            assert!(pages[0].iter().any(|t| t == "WO-1006 · Replace HVAC belt"), "{:?}", pages[0]);
            assert!(!pages[0].iter().any(|t| t.starts_with("WO-1001")));
            j.check("sheet total", pages[0].iter().any(|t| t == "All work orders: 3.5 hours"), true);
            leave_report(&mut h, studio);
        });

        journey.step("Technician role: no setup, no new or deleted work orders", |j| {
            crate::ui::runtime::tests::run_as(&mut h, "Technician");
            wait(&mut h);
            let setup = h.config().design.navigation.iter().find(|n| n.label == "Setup").expect("Setup").id.clone();
            assert!(!h.exists(format!("nav-group:{setup}")), "Setup is hidden");
            show_list(&mut h, "Work orders");
            open_row(&mut h, "WO-1001");
            assert!(h.exists("edit"));
            j.check("delete hidden", h.exists("delete"), false);
            h.click("back-crumb-button");
            wait(&mut h);
            j.check("no create", h.exists("new-record"), false);
        });
    });
}

fn open_release_tab(h: &mut Harness) {
    h.switch_mode(ModeId::App);
    h.click("tab-release");
}

fn export_bundle(h: &mut Harness, version: &str, path: &std::path::Path) {
    open_release_tab(h);
    h.fill("release-version", version);
    h.dialogs.queue_save(Some(path.to_path_buf()));
    h.update(|w, cx| w.scroll("settings-panel", ScrollDelta::Pixels(point(px(0.), px(-5000.))), cx));
    h.settle();
    h.click("export-bundle");
    assert_eq!(h.label("exported-bundle"), format!("Exported Work orders {version}"), "{:?}", h.try_label("release-error"));
}

fn close_to_start(h: &mut Harness) {
    h.dialogs.queue_confirm(true);
    h.click("close-project");
    assert!(h.exists("open-bundle"));
}

#[gpui_kit::test]
fn golden_work_orders_v1_bundle_runtime_record_then_update_to_v2_keeps_the_record_and_adds_due_dates(cx: &mut TestAppContext) {
    with_journey(APP, "migration-v1-v2", |journey| {
        let mut h = journey.step("Create Work orders v1 and save it", move |_| {
            let mut h = start_from_template(cx, "work-orders");
            let ixt = h.temp_path("work-orders.ixt");
            h.dialogs.queue_save(Some(ixt));
            h.click("save-project");
            assert_eq!(h.label("document-status"), "Saved");
            h
        });
        let label = h.label.clone();
        let (v1, v2) = (h.temp_path("work-orders-1.0.0.ixtr"), h.temp_path("work-orders-2.0.0.ixtr"));
        journey.step("Export runtime bundle 1.0.0", |_| export_bundle(&mut h, "1.0.0", &v1));

        journey.step("Open the bundle and add a Runtime record", |j| {
            close_to_start(&mut h);
            h.dialogs.queue_open(Some(v1.clone()));
            h.click("open-bundle");
            assert_eq!(h.label("runtime-version"), "Version 1.0.0");
            wait(&mut h);
            show_list(&mut h, "Work orders");
            h.click("new-record");
            wait(&mut h);
            h.fill("field:number", "WO-2001");
            h.fill("field:title", "Runtime-only order");
            choose_related(&mut h, "asset", "Backup generator");
            h.click("create");
            wait(&mut h);
            assert!(h.exists("edit"), "opened as saved: {}", messages(&mut h));
            job_done(&mut h);
            j.check("runtime record", scalar(&label, "SELECT count(*) FROM work_orders"), 6);
        });

        journey.step("Upgrade the Studio document to v2 through the YAML and Migrations", |j| {
            close_to_start(&mut h);
            h.dialogs.queue_open(Some(h.temp_path("work-orders.ixt")));
            h.click("open-document");
            assert!(h.exists("mode-app"));
            let yaml = crate::templates::read_template_config("work-orders@2".into()).expect("v2 template");
            apply_yaml(&mut h, &yaml.replace("activeMode: run", "activeMode: app"));
            h.click("tab-migrations");
            assert_eq!(h.label("apply-migrations"), "Apply pending (1)");
            h.click("apply-migrations");
            wait(&mut h);
            assert!(h.label("run-status").starts_with("All migrations succeeded."), "{}", h.label("run-status"));
            j.check("Studio data has the new column", sql(&label, "SELECT number, due_on FROM work_orders WHERE number = 'WO-1001'"), json!([["WO-1001", null]]));
            j.check("v2 definitions validate", validation_errors(&label), Vec::<String>::new());
        });

        journey.step("Export runtime bundle 2.0.0", |_| export_bundle(&mut h, "2.0.0", &v2));

        journey.step("Update the installation to 2.0.0", |_| {
            close_to_start(&mut h);
            h.dialogs.queue_open(Some(v1.clone()));
            h.click("open-bundle");
            h.dialogs.queue_open(Some(v2.clone()));
            h.click("check-update");
            let title = h.label("update-confirm");
            assert!(title.starts_with("Update ") && title.ends_with(" to 2.0.0?"), "{title}");
            let note = ["no-migrations", "migrations-note", "migration:0"].map(|id| (id, h.try_label(id)));
            assert_eq!(h.try_label("migration:0").as_deref(), Some("002 Add work-order due dates"), "{note:?}");
            assert!(!h.exists("migration:1"));
            h.click("migration-sql-disclosure:0-toggle");
            assert!(h.label("migration-sql:0").to_lowercase().contains("alter table work_orders"));
            h.click("apply-update");
            assert!(h.notice().unwrap_or_default().contains("Updated to version 2.0.0. Your records were kept."), "{:?}", h.notice());
        });

        journey.step("The Runtime record is preserved and the new column is present", |j| {
            wait(&mut h);
            j.check(
                "record kept, due_on added",
                sql(&label, "SELECT number, title, due_on FROM work_orders WHERE number = 'WO-2001'"),
                json!([["WO-2001", "Runtime-only order", null]]),
            );
            let schema = crate::inspect_table(label.clone(), "work_orders".into(), None).expect("schema");
            j.check("due_on column", schema.columns.iter().any(|c| c.name == "due_on"), true);
            let status = crate::migrations::commands::migration_status(label.clone()).expect("status");
            j.check(
                "migration 002 applied once on the installation",
                status.iter().map(|m| (m.name.clone(), m.applied)).collect::<Vec<_>>(),
                json!([["001 Create work-order schema", true], ["002 Add work-order due dates", true], ["Seed sample data", true]]),
            );
            show_list(&mut h, "Work orders");
            open_row(&mut h, "WO-2001");
            assert!(h.exists("field:due-on:label"), "the Due on field shows");
        });
    });
}
