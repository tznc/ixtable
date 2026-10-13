//! Helpers shared by the Query mode tests: seeded data, drop-down picking, typing into
//! the SQL editor and waiting for runs (which execute on a worker thread).

use super::QueryMode;
use crate::ui::testing::UiContext;
use crate::{
    data::DataValue,
    ui::{shell::ModeId, testing::Harness},
};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{App, ElementId, Entity, ScrollDelta, SharedString, TestAppContext, point, px};
use serde_json::json;
use std::time::{Duration, Instant};

pub fn column(name: &str, declared: &str, pk: u32) -> serde_json::Value {
    json!({
        "name": name, "declaredType": declared, "nullable": pk == 0, "primaryKeyPosition": pk,
        "unique": false, "defaultExpression": null, "generatedExpression": null,
    })
}

pub fn create_table<C: UiContext>(h: &mut Harness<'_, C>, spec: serde_json::Value) {
    crate::create_database_table(h.label.clone(), serde_json::from_value(spec).unwrap())
        .expect("create table");
    h.database_changed();
}

/// customers(id, name) and orders(id, customer_id, amount) with a foreign key, plus rows.
pub fn seed_sales<C: UiContext>(h: &mut Harness<'_, C>) {
    create_table(
        h,
        json!({
            "name": "customers",
            "columns": [column("id", "INTEGER", 1), column("name", "TEXT", 0)],
            "foreignKeys": [], "checks": [], "withoutRowid": false,
        }),
    );
    create_table(
        h,
        json!({
            "name": "orders",
            "columns": [
                column("id", "INTEGER", 1),
                column("customer_id", "INTEGER", 0),
                column("amount", "REAL", 0),
            ],
            "foreignKeys": [{
                "columns": ["customer_id"], "targetTable": "customers",
                "targetColumns": ["id"], "onUpdate": null, "onDelete": null,
            }],
            "checks": [], "withoutRowid": false,
        }),
    );
    for (id, name) in [(1, "ACME"), (2, "Bolt"), (3, "Core")] {
        h.insert_row(
            "customers",
            &[
                ("id", DataValue::Integer(id)),
                ("name", DataValue::Text(name.into())),
            ],
        );
    }
    for (id, customer, amount) in [(1, 1, 10.0), (2, 1, 30.0), (3, 2, 5.0), (4, 2, 50.0), (5, 3, 1.0)] {
        h.insert_row(
            "orders",
            &[
                ("id", DataValue::Integer(id)),
                ("customer_id", DataValue::Integer(customer)),
                ("amount", DataValue::Real(amount)),
            ],
        );
    }
}

/// Seeds a document's sales data and opens Query mode (for any harness).
pub fn open_query<C: UiContext>(h: &mut Harness<'_, C>) {
    seed_sales(h);
    h.switch_mode(ModeId::Query);
    assert_eq!(h.label("mode-title"), "Query");
}

/// A new document with the sales data, in Query mode.
pub fn open_query_mode(cx: &mut TestAppContext) -> Harness<'_, TestAppContext> {
    let mut h = Harness::new_document(cx);
    seed_sales(&mut h);
    h.switch_mode(ModeId::Query);
    assert_eq!(h.label("mode-title"), "Query");
    h
}

pub fn query_mode<C: UiContext>(h: &mut Harness<'_, C>) -> Entity<QueryMode> {
    let view = h.read_shell(|s, _| s.mode_view()).expect("mode view");
    view.downcast().expect("QueryMode")
}

/// Reads the mode.
pub fn with_mode<C: UiContext, R>(h: &mut Harness<'_, C>, f: impl FnOnce(&QueryMode, &App) -> R) -> R {
    let mode = query_mode(h);
    h.cx.read(|cx| f(mode.read(cx), cx))
}

/// Selects `value` in the drop-down `id` (open it, then click the option).
pub fn pick<C: UiContext>(h: &mut Harness<'_, C>, id: &str, value: &str) {
    click(h, id);
    click(h, &format!("{id}/{value}"));
}

/// Repeats `check` until it holds. Runs execute on a worker thread, so a result arrives
/// in real time after the test executor has been driven.
pub fn wait_until<C: UiContext>(h: &mut Harness<'_, C>, what: &str, mut check: impl FnMut(&mut Harness<'_, C>) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        h.settle();
        if check(h) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
        // Runs report back on the mode's fake-clock tick.
        h.advance(Duration::from_millis(50));
    }
}

/// The result table's cells once a result is showing.
pub fn grid<C: UiContext>(h: &mut Harness<'_, C>) -> Vec<Vec<String>> {
    with_mode(h, |q, cx| q.results.read(cx).cells())
}

/// Waits for the results table to show exactly `expected`.
pub fn expect_grid<C: UiContext>(h: &mut Harness<'_, C>, expected: &[&[&str]]) {
    let want: Vec<Vec<String>> = expected
        .iter()
        .map(|r| r.iter().map(|c| c.to_string()).collect())
        .collect();
    wait_until(h, &format!("grid {want:?}"), |h| grid(h) == want);
}

/// Replaces the SQL editor's text the way an edit does: the mode takes the new text as the
/// editor reports it (programmatic edits of the editor emit no change event) and the view
/// shows it. Keystrokes into the code editor are not reliable in the headless harness.
pub fn type_sql<C: UiContext>(h: &mut Harness<'_, C>, text: &str) {
    h.settle();
    let mode = query_mode(h);
    let text = text.to_string();
    h.cx.update(|cx| mode.update(cx, |q, cx| q.sql_edited(text, cx)));
    h.settle();
}

pub fn editor_text<C: UiContext>(h: &mut Harness<'_, C>) -> String {
    with_mode(h, |q, cx| q.sql_editor.read(cx).value().to_string())
}

/// The id of the builder field `alias.column`.
pub fn field_id<C: UiContext>(h: &mut Harness<'_, C>, name: &str) -> String {
    with_mode(h, |q, _| {
        q.model()
            .expect("a builder model")
            .fields
            .iter()
            .find(|f| super::model::field_name(f) == name)
            .unwrap_or_else(|| panic!("no field {name}"))
            .id
            .clone()
    })
}

/// Scrolls the query pane until `id` lies fully inside it (clicks need a visible target).
pub fn reveal<C: UiContext>(h: &mut Harness<'_, C>, id: &ElementId) {
    h.settle();
    for _ in 0..80 {
        let (Some(el), Some(pane)) = (h.try_find(id.clone()), h.try_find("query-pane")) else {
            // Not rendered: nothing to scroll to.
            return;
        };
        let (e, p) = (el.bounds(), pane.bounds());
        let dy = if e.origin.y < p.origin.y {
            200.0
        } else if e.origin.y + e.size.height > p.origin.y + p.size.height {
            -200.0
        } else {
            return;
        };
        let delta = ScrollDelta::Pixels(point(px(0.), px(dy)));
        h.update(|w, cx| w.scroll("query-pane", delta, cx));
        h.settle();
    }
}

pub fn click<C: UiContext>(h: &mut Harness<'_, C>, id: &str) {
    let id = ElementId::from(SharedString::from(id.to_string()));
    reveal(h, &id);
    h.click(id);
}

pub fn fill<C: UiContext>(h: &mut Harness<'_, C>, id: &str, text: &str) {
    let id = ElementId::from(SharedString::from(id.to_string()));
    reveal(h, &id);
    h.fill(id, text);
}

pub fn exists<C: UiContext>(h: &mut Harness<'_, C>, id: &str) -> bool {
    h.exists(SharedString::from(id.to_string()))
}
