//! Helpers shared by the reports tests and the reports screenshot spec. They are generic
//! over the harness's context, so they drive both the test and the headless-renderer apps.

use super::{ReportPreview, ReportsMode};
use crate::archive::{DocumentConfig, SavedQuery};
use crate::data::DataValue;
use crate::reports::{Band, Component, Report};
use crate::ui::shell::ModeId;
use crate::ui::testing::{Harness, UiContext};
use gpui_kit::{Entity, SharedString};
use std::time::Duration;

pub fn id(name: &str) -> SharedString {
    SharedString::from(name.to_string())
}

/// Opens the options menu of the choice `button` and picks the option labeled `option`.
pub fn choose<C: UiContext>(h: &mut Harness<'_, C>, button: &str, option: &str) {
    h.click(id(button));
    let mut seen = vec![];
    for index in 0..64usize {
        let label = h.update(|w, _| {
            use gpui_kit::test::TestWindowExt as _;
            w.within("popup-menu")
                .try_find(index)
                .map(|item| item.label().unwrap_or_default().to_string())
        });
        match label {
            Some(label) if label == option => {
                h.click_in("popup-menu", index);
                return;
            }
            Some(label) => seen.push(label),
            None => break,
        }
    }
    panic!("{button} has no option {option:?}; options are {seen:?}");
}

/// Polls (with real sleeps: loads run on worker threads) until `done` holds.
pub fn wait_until<C: UiContext>(h: &mut Harness<'_, C>, what: &str, mut done: impl FnMut(&mut Harness<'_, C>) -> bool) {
    for _ in 0..400 {
        h.settle();
        if done(h) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
        h.advance(Duration::from_millis(250));
    }
    panic!("timed out waiting for {what}");
}

/// Edits definitions the way the editors do, through the config store the views read.
pub fn edit_store<C: UiContext>(h: &mut Harness<'_, C>, edit: impl FnOnce(&mut DocumentConfig)) {
    let store = h.read_shell(|s, _| s.store.clone());
    h.cx.update(|cx| store.update(cx, |s, cx| s.update("Test edit", edit, cx)))
        .expect("update config");
    h.settle();
}

pub fn add_query<C: UiContext>(h: &mut Harness<'_, C>, id: &str, name: &str, sql: &str) {
    let query = SavedQuery { id: id.into(), name: name.into(), sql: sql.into(), ..Default::default() };
    edit_store(h, |c| c.saved_queries.push(query));
}

/// 60 orders over three regions; amounts 1..=60.
pub fn seed_orders<C: UiContext>(h: &mut Harness<'_, C>) {
    h.create_table(
        "orders",
        &[("id", "INTEGER", 1), ("region", "TEXT", 0), ("customer", "TEXT", 0), ("amount", "REAL", 0)],
    );
    let regions = ["East", "North", "West"];
    for i in 0..60usize {
        h.insert_row(
            "orders",
            &[
                ("id", DataValue::Integer(i as i64 + 1)),
                ("region", DataValue::Text(regions[i % 3].into())),
                ("customer", DataValue::Text(format!("Customer {}", i + 1))),
                ("amount", DataValue::Real(i as f64 + 1.0)),
            ],
        );
    }
}

pub fn mode<C: UiContext>(h: &mut Harness<'_, C>) -> Entity<ReportsMode> {
    h.read_shell(|s, _| s.mode_view())
        .expect("a mode is showing")
        .downcast::<ReportsMode>()
        .ok()
        .expect("Reports mode is showing")
}

pub fn preview<C: UiContext>(h: &mut Harness<'_, C>) -> Entity<ReportPreview> {
    let mode = mode(h);
    h.cx.read(|cx| mode.read(cx).preview()).expect("the preview is showing")
}

/// The text of every line of the page the preview shows.
pub fn page_texts<C: UiContext>(h: &mut Harness<'_, C>) -> Vec<String> {
    let preview = preview(h);
    h.cx.read(|cx| preview.read(cx).page_texts())
}

/// Waits for the preview page labeled `label` ("Page 1 of 2").
pub fn wait_for_page<C: UiContext>(h: &mut Harness<'_, C>, label: &str) {
    wait_until(h, label, |h| h.try_label("report-page").as_deref() == Some(label));
}

pub fn open_reports<C: UiContext>(h: &mut Harness<'_, C>) {
    h.switch_mode(ModeId::Reports);
    assert_eq!(h.mode(), ModeId::Reports);
}

/// The first report of the document.
pub fn report<C: UiContext>(h: &Harness<'_, C>) -> Report {
    h.config().reports.into_iter().next().expect("a report")
}

/// The last component added to a band of the first report.
pub fn last_component<C: UiContext>(h: &Harness<'_, C>, band: impl Fn(&Report) -> &Band) -> Component {
    let report = report(h);
    band(&report).components.last().cloned().expect("a component")
}

/// Selects a band, then adds a component of `kind` to it.
pub fn add_component<C: UiContext>(h: &mut Harness<'_, C>, band: &str, kind: &str) {
    h.click(id(&format!("band-{band}")));
    h.click(id(&format!("add-{kind}")));
}

pub fn new_report<C: UiContext>(h: &mut Harness<'_, C>) {
    open_reports(h);
    h.click("new-report");
    assert_eq!(h.label("mode-title"), "Report 1");
}

/// Picks the dataset, adds a group and waits until the dataset's columns reach the pickers.
pub fn pick_orders_dataset<C: UiContext>(h: &mut Harness<'_, C>) {
    choose(h, "report-dataset", "Query: Orders");
    h.click("add-group");
    wait_until(h, "the dataset columns", |h| h.exists("group-1-field"));
}

pub fn validation_issues_for_reports<C: UiContext>(h: &Harness<'_, C>) -> Vec<String> {
    crate::validation::validate_document(h.label.clone())
        .expect("validate")
        .into_iter()
        .filter(|i| i.object_kind == "report")
        .map(|i| i.message)
        .collect()
}
