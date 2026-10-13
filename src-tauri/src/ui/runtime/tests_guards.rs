//! Data binding guards (port of the query-form tests of tests/integration/data-binding-guards
//! .test.tsx). The React test hosted the forms on a dashboard; here a detached
//! `FormRenderer` (no Runtime, as a dashboard embeds it) in a window of its own is driven
//! with `set_props`, the call a dashboard makes when its filters change. The cases that need
//! the Dashboards editor or `EmbeddedForm` (first-row pinning, the row filter message, the
//! Record id property, orphaned binding removal in the Design studio) belong to those
//! features. Imports are explicit.

use super::tests::{seed, uid, wait};
use super::{FormRenderer, FormRendererProps, RuntimeEnv};
use crate::data::{DataValue, NamedValue};
use crate::design::FormMode;
use crate::recordstore::WriteOp;
use crate::ui::testing::Harness;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, ElementId, Entity, Point, TestAppContext, WindowBounds,
    WindowOptions, px, size,
};
use serde_json::{Map, Value, json};
use std::time::Duration;

/// A `FormRenderer` in a window of its own, detached from any Runtime.
struct Embedded {
    window: AnyWindowHandle,
    renderer: Entity<FormRenderer>,
}

impl Embedded {
    fn open(h: &mut Harness, props: FormRendererProps) -> Self {
        let (window, renderer) = h.cx.update(|cx| {
            let bounds = Bounds { origin: Point::default(), size: size(px(900.), px(1600.)) };
            gpui_kit::open_window(
                WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
                cx,
                |window, cx| cx.new(|cx| FormRenderer::new(props, window, cx)),
            )
            .expect("open the embedding window")
        });
        let mut embedded = Self { window, renderer };
        embedded.settle(h);
        embedded
    }

    fn settle(&mut self, h: &mut Harness) {
        for _ in 0..16 {
            h.cx.executor().advance_clock(Duration::from_millis(20));
            std::thread::sleep(Duration::from_millis(5));
            h.cx.run_until_parked();
            let _ = h.cx.update_window(self.window, |_, w, cx| w.render_frame(cx));
        }
    }

    fn label(&self, h: &mut Harness, id: &str) -> Option<String> {
        let id = ElementId::from(gpui_kit::SharedString::from(id.to_string()));
        h.cx.update_window(self.window, |_, w, _| w.try_find(id).map(|s| s.label().unwrap_or_default().to_string()))
            .expect("window")
    }

    fn click(&mut self, h: &mut Harness, id: &str) {
        let id = ElementId::from(gpui_kit::SharedString::from(id.to_string()));
        let _ = h.cx.update_window(self.window, |_, w, cx| {
            w.render_frame(cx);
            w.click(id, cx)
        });
        self.settle(h);
    }

    fn set_props(&mut self, h: &mut Harness, props: FormRendererProps) {
        let renderer = self.renderer.clone();
        h.cx.update(|cx| renderer.update(cx, |r, cx| r.set_props(props, cx)));
        self.settle(h);
    }
}

fn env(h: &mut Harness) -> RuntimeEnv {
    let (store, bus) = h.read_shell(|s, _| (s.store.clone(), s.bus.clone()));
    RuntimeEnv { label: h.label.clone().into(), store, bus, runtime: None }
}

fn params(bin: &str) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("bin".into(), json!(bin));
    map
}

fn seed_parts(h: &mut Harness) {
    h.create_table(
        "parts",
        &[("id", "INTEGER", 1), ("name", "TEXT", 0), ("bin", "TEXT", 0), ("qty", "INTEGER", 0)],
    );
    let ops: Vec<WriteOp> = (1..=60i64)
        .map(|n| WriteOp::Insert {
            table: "parts".into(),
            values: vec![
                NamedValue { column: "id".into(), value: DataValue::Integer(n) },
                NamedValue { column: "name".into(), value: DataValue::Text(format!("Part {n:02}")) },
                NamedValue { column: "bin".into(), value: DataValue::Text(if n <= 40 { "A" } else { "B" }.into()) },
                NamedValue { column: "qty".into(), value: DataValue::Integer(1) },
            ],
        })
        .collect();
    crate::recordstore::commands::execute_write_batch(h.label.clone(), ops, None).expect("seed parts");
    h.records_changed();
}

/// The "Bin parts" query form; returns its id.
fn bin_form(h: &mut Harness) -> String {
    let (query, form) = (uid(), uid());
    let id = form.clone();
    seed(h, move |config| {
        config.saved_queries = serde_json::from_value(json!([{
            "id": query, "name": "Parts in bin",
            "sql": "SELECT id, name FROM parts WHERE bin = $bin ORDER BY id",
            "parameters": [{"name": "bin", "logicalType": "text", "required": true}],
        }]))
        .expect("saved query");
        let form = json!({
            "id": form, "name": "Bin parts",
            "source": {"kind": "query", "queryId": query, "params": {"bin": "params.bin"}},
            "modes": ["list", "detail"], "listColumns": ["id", "name"], "pageSize": 25,
            "controls": [], "rules": [],
        });
        config.design.forms.push(serde_json::from_value(form).expect("form"));
    });
    id
}

#[gpui_kit::test]
fn restarts_paging_when_filter_params_change(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_parts(&mut h);
    let form = bin_form(&mut h);
    let mut props = FormRendererProps::new(env(&mut h), form.clone());
    props.mode = Some(FormMode::List);
    props.params = params("A");
    let mut embedded = Embedded::open(&mut h, props.clone());

    assert_eq!(embedded.label(&mut h, "pager-status").as_deref(), Some("1–25 of 40"));
    embedded.click(&mut h, "next-page");
    assert_eq!(embedded.label(&mut h, "pager-status").as_deref(), Some("26–40 of 40"));

    props.params = params("B");
    embedded.set_props(&mut h, props);
    assert_eq!(embedded.label(&mut h, "pager-status").as_deref(), Some("1–20 of 20"));
    let list = h.cx.read(|cx| embedded.renderer.read(cx).list()).expect("a list");
    assert!(h.cx.read(|cx| !list.read(cx).can_prev()));
    wait(&mut h);
}

#[gpui_kit::test]
fn keeps_query_forms_off_record_ids(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_parts(&mut h);
    let form = bin_form(&mut h);
    let mut props = FormRendererProps::new(env(&mut h), form);
    props.mode = Some(FormMode::Detail);
    props.record_id = Some(json!(3));
    props.params = params("A");
    let mut embedded = Embedded::open(&mut h, props);
    assert_eq!(
        embedded.label(&mut h, "status:error").as_deref(),
        Some("This form shows query rows, so it cannot open a record by id.")
    );
}
