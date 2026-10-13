//! Port of tests/unit/export-menu.test.tsx: the menu hosted on its own, with a scripted runner.
//! Imports are explicit.

use super::{ExportMenu, ExportRunner};
use crate::export::{ExportFormat, ExportSummary};
use crate::manager::AppError;
use crate::ui::data::testkit::{choose, menu_labels};
use crate::ui::testing::Harness;
use gpui_kit::{AppContext as _, Entity, TestAppContext};
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Calls = Arc<Mutex<Vec<(ExportFormat, String)>>>;

// Hosts the menu in a window of its own (the harness' window is switched to it).
fn host<'a>(
    cx: &'a mut TestAppContext,
    name: &str,
    runner: ExportRunner,
) -> (Harness<'a>, Entity<ExportMenu>) {
    let mut h = Harness::start(cx);
    let name = name.to_string();
    let (window, menu) = h.cx.update(|cx| {
        gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |_, cx| cx.new(|_| ExportMenu::new(&name, runner)))
            .expect("open the menu window")
    });
    h.window = window;
    h.settle();
    (h, menu)
}

fn wait(h: &mut Harness) {
    for _ in 0..40 {
        h.advance(Duration::from_millis(20));
        std::thread::sleep(Duration::from_millis(5));
        h.settle();
    }
}

fn recording(calls: &Calls, rows: u64) -> ExportRunner {
    let calls = calls.clone();
    Arc::new(move |format, path| {
        calls.lock().unwrap().push((format, path.clone()));
        Ok(ExportSummary { rows, path })
    })
}

#[gpui_kit::test]
fn lists_the_three_formats_and_closes_on_escape(cx: &mut TestAppContext) {
    let calls: Calls = Default::default();
    let (mut h, _menu) = host(cx, "orders", recording(&calls, 1));
    h.click("Export");
    assert_eq!(menu_labels(&mut h), ["CSV", "Excel (.xlsx)", "JSON"]);
    h.press("escape");
    assert!(!h.exists("popup-menu"));
}

#[gpui_kit::test]
fn does_nothing_when_the_save_dialog_is_cancelled(cx: &mut TestAppContext) {
    let calls: Calls = Default::default();
    let (mut h, menu) = host(cx, "orders", recording(&calls, 1));
    h.dialogs.queue_save(None);
    choose(&mut h, "Export", "CSV");
    wait(&mut h);
    assert_eq!(h.dialogs.count("save"), 1);
    assert!(calls.lock().unwrap().is_empty());
    assert!(!h.exists("export-status") && !h.exists("export-error"));
    assert!(!h.cx.read(|cx| menu.read(cx).busy));
}

#[gpui_kit::test]
fn exports_to_the_chosen_path_with_the_extension_ensured_then_reports_the_rows(cx: &mut TestAppContext) {
    let calls: Calls = Default::default();
    let (mut h, menu) = host(cx, "Sales/Q1", recording(&calls, 1234));
    let path = h.temp_path("out");
    h.dialogs.queue_save(Some(path.clone()));
    choose(&mut h, "Export", "Excel (.xlsx)");
    wait(&mut h);
    let want = format!("{}.xlsx", path.to_string_lossy());
    assert_eq!(*calls.lock().unwrap(), [(ExportFormat::Xlsx, want)]);
    assert_eq!(h.label("export-status"), "Exported 1,234 rows");
    // The button is usable again (the test executor cannot hold a runner half way to show it busy).
    assert!(!h.cx.read(|cx| menu.read(cx).busy));
}

#[gpui_kit::test]
fn shows_the_error_from_a_failed_export(cx: &mut TestAppContext) {
    let runner: ExportRunner = Arc::new(|_, _| Err(AppError::new("IO_ERROR", "disk full")));
    let (mut h, _menu) = host(cx, "orders", runner);
    let path = h.temp_path("out.json");
    h.dialogs.queue_save(Some(path));
    choose(&mut h, "Export", "JSON");
    wait(&mut h);
    assert!(h.label("export-error").contains("disk full"));
    assert!(!h.exists("export-status"));
}
