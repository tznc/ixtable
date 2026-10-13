//! Helpers shared by the dashboards tests and the dashboards screenshot spec. They are
//! generic over the harness's context, so they drive both the test and the headless-renderer
//! apps.

use super::DashboardView;

use crate::archive::{DocumentConfig, QueryParameter, SavedQuery};
use crate::dashboards::{Dashboard, DashboardComponent};
use crate::data::DataValue;
pub use crate::ui::reports::test_support::edit_store;
use crate::ui::reports::test_support::{id, wait_until};
use crate::ui::shell::ModeId;
use crate::ui::testing::{Harness, UiContext};
use gpui_kit::Entity;

pub const SALES_BY_REGION: &str = "6d1f8e2a-0c39-4c55-9a4c-2b8f9f6f7a01";

pub fn uid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// The sales table (East 100 and 250, West 400, North 50) and the "Sales by region" query,
/// which takes an optional `$region`.
pub fn seed_sales<C: UiContext>(h: &mut Harness<'_, C>) {
    h.create_table("sales", &[("id", "INTEGER", 1), ("region", "TEXT", 0), ("amount", "REAL", 0)]);
    for (i, (region, amount)) in [("East", 100.0), ("East", 250.0), ("West", 400.0), ("North", 50.0)].into_iter().enumerate() {
        h.insert_row(
            "sales",
            &[
                ("id", DataValue::Integer(i as i64 + 1)),
                ("region", DataValue::Text(region.into())),
                ("amount", DataValue::Real(amount)),
            ],
        );
    }
    let query = SavedQuery {
        id: SALES_BY_REGION.into(),
        name: "Sales by region".into(),
        sql: "SELECT region, SUM(amount) AS total FROM sales WHERE ($region IS NULL OR region = $region) GROUP BY region ORDER BY region"
            .into(),
        parameters: vec![QueryParameter { name: "region".into(), logical_type: "text".into(), default_value: None, required: false }],
        ..Default::default()
    };
    edit_store(h, |c: &mut DocumentConfig| c.saved_queries.push(query));
}

pub fn open_dashboards<C: UiContext>(h: &mut Harness<'_, C>) {
    h.switch_mode(ModeId::Dashboards);
    assert_eq!(h.mode(), ModeId::Dashboards);
}

/// Opens the mode and creates "Dashboard 1".
pub fn new_dashboard<C: UiContext>(h: &mut Harness<'_, C>) {
    open_dashboards(h);
    h.click("new-dashboard");
    assert_eq!(h.label("mode-title"), "Dashboard 1");
}

pub fn dashboard<C: UiContext>(h: &Harness<'_, C>) -> Dashboard {
    h.config().dashboards.into_iter().next().expect("a dashboard")
}

pub fn component<C: UiContext>(h: &Harness<'_, C>, title: &str) -> DashboardComponent {
    dashboard(h).components.into_iter().find(|c| c.title == title).unwrap_or_else(|| panic!("no component {title}"))
}

/// Chooses the saved query `query_id` in the shared picker of the properties panel.
pub fn pick_query<C: UiContext>(h: &mut Harness<'_, C>, query_id: &str) {
    h.click(id("prop-query"));
    h.click(id(&format!("prop-query/{query_id}")));
}

/// Waits until the query's columns reached the pickers of the properties panel.
pub fn wait_for_columns<C: UiContext>(h: &mut Harness<'_, C>, what: &str, probe: &str) {
    wait_until(h, what, |h| h.exists(id(probe)));
}

use super::QueryRunner;
use crate::manager::AppError;
use crate::queries::QueryRun;
use serde_json::{Map, Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI64, Ordering},
};

/// One query run the fake saw.
#[derive(Clone, Debug)]
pub struct Call {
    pub query: String,
    pub params: Map<String, Value>,
    pub run_id: String,
}

/// A query runner double (the React tests mocked `runSavedQuery`): every query answers one
/// row `n`, and the first run can block until its run id is cancelled.
pub struct FakeQueries {
    pub calls: Mutex<Vec<Call>>,
    pub cancelled: Mutex<Vec<String>>,
    pub value: AtomicI64,
    block_first: bool,
}

impl FakeQueries {
    pub fn new(value: i64, block_first: bool) -> Arc<Self> {
        Arc::new(Self { calls: Mutex::default(), cancelled: Mutex::default(), value: AtomicI64::new(value), block_first })
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn last(&self) -> Option<Call> {
        self.calls().last().cloned()
    }
}

pub fn install_fake<C: UiContext>(h: &mut Harness<'_, C>, fake: &Arc<FakeQueries>) {
    let (runner, canceller) = (fake.clone(), fake.clone());
    let runner = QueryRunner {
        run: Arc::new(move |_, query, params, run_id| {
            let index = {
                let mut calls = runner.calls.lock().unwrap_or_else(|e| e.into_inner());
                calls.push(Call { query: query.into(), params: params.clone(), run_id: run_id.into() });
                calls.len() - 1
            };
            if runner.block_first && index == 0 {
                for _ in 0..4000 {
                    if runner.cancelled.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|r| r == run_id) {
                        return Err(AppError::new("CANCELLED", "Query cancelled"));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
            Ok(QueryRun {
                columns: vec!["n".into()],
                rows: vec![vec![DataValue::Integer(runner.value.load(Ordering::SeqCst))]],
                truncated: false,
                row_limit: 10_000,
                elapsed_ms: 1,
            })
        }),
        cancel: Arc::new(move |_, run_id| canceller.cancelled.lock().unwrap_or_else(|e| e.into_inner()).push(run_id.into())),
    };
    h.cx.update(|cx| cx.set_global(runner));
}

fn at3(column: u32) -> Value {
    json!({"column": column, "row": 1, "columnSpan": 3, "rowSpan": 1, "region": null})
}

/// The "Ops" dashboard of the React view tests: a KPI on query q1, a button running action a1
/// and a two-paragraph note, with a Region select filter.
pub fn seed_ops<C: UiContext>(h: &mut Harness<'_, C>) {
    let dashboard: Dashboard = serde_json::from_value(json!({
        "id": "d1", "name": "Ops",
        "filters": [{"id": "f1", "name": "Region", "param": "region", "logicalType": "text", "control": "select", "options": ["East"]}],
        "components": [
            {"id": "k", "kind": "kpi", "title": "Open orders", "placement": at3(1), "queryId": "q1", "valueField": "n"},
            {"id": "b", "kind": "button", "title": "Go", "label": "Open dashboard", "placement": at3(4), "actionId": "a1"},
            {"id": "t", "kind": "text", "title": "Notes", "placement": at3(7), "text": "First.\n\nSecond."},
        ],
    }))
    .expect("dashboard");
    let action = serde_json::from_value(json!({
        "id": "a1", "name": "Go", "onError": "stop",
        "steps": [{"id": "s", "kind": "openDashboard", "dashboardId": "d1"}],
    }))
    .expect("action");
    let query = SavedQuery { id: "q1".into(), name: "Orders".into(), sql: "SELECT 1 AS n".into(), ..Default::default() };
    edit_store(h, move |c: &mut DocumentConfig| {
        c.dashboards = vec![dashboard];
        c.saved_queries = vec![query];
        c.actions = vec![action];
    });
}

/// Steps the test clock in the 50 ms ticks the loaders run on.
pub fn advance_by<C: UiContext>(h: &mut Harness<'_, C>, ms: u64) {
    for _ in 0..ms / 50 {
        h.advance(std::time::Duration::from_millis(50));
    }
}

use crate::design::{Form, NavKind, NavigationItem};
use crate::recordstore::WriteOp;
use crate::data::NamedValue;
use crate::ui::runtime::tests::{seed, wait};

/// 60 parts: ids 1-40 in bin A and 41-60 in bin B, quantity 1.
pub fn seed_parts<C: UiContext>(h: &mut Harness<'_, C>) {
    h.create_table("parts", &[("id", "INTEGER", 1), ("name", "TEXT", 0), ("bin", "TEXT", 0), ("qty", "INTEGER", 0)]);
    let ops: Vec<WriteOp> = (1..=60i64).map(|n| insert_part(n, if n <= 40 { "A" } else { "B" })).collect();
    crate::recordstore::commands::execute_write_batch(h.label.clone(), ops, None).expect("seed parts");
    h.records_changed();
}

pub fn insert_part(n: i64, bin: &str) -> WriteOp {
    let value = |column: &str, value: DataValue| NamedValue { column: column.into(), value };
    WriteOp::Insert {
        table: "parts".into(),
        values: vec![
            value("id", DataValue::Integer(n)),
            value("name", DataValue::Text(format!("Part {n:02}"))),
            value("bin", DataValue::Text(bin.into())),
            value("qty", DataValue::Integer(1)),
        ],
    }
}

/// "Parts in bin" with a required `$bin`.
pub fn bin_query(id: &str) -> SavedQuery {
    SavedQuery {
        id: id.into(),
        name: "Parts in bin".into(),
        sql: "SELECT id, name FROM parts WHERE bin = $bin ORDER BY id".into(),
        parameters: vec![QueryParameter { name: "bin".into(), logical_type: "text".into(), default_value: None, required: true }],
        ..Default::default()
    }
}

/// A text or number control of a form, bound to `column`.
pub fn field(label: &str, column: &str, kind: &str, row: u32) -> Value {
    json!({
        "id": uid(), "kind": kind, "label": label, "binding": {"column": column},
        "placement": {"column": 1, "row": row, "columnSpan": 6, "rowSpan": 1},
    })
}

pub fn at(column: u32, row: u32, column_span: u32) -> Value {
    json!({"column": column, "row": row, "columnSpan": column_span, "rowSpan": 2})
}

/// Adds forms and dashboards from JSON (the way the React tests seeded `config`).
pub fn define<C: UiContext>(h: &mut Harness<'_, C>, forms: Vec<Value>, dashboards: Vec<Value>) {
    seed(h, move |c: &mut DocumentConfig| {
        c.design.forms.extend(forms.into_iter().map(|f| serde_json::from_value::<Form>(f).expect("form")));
        c.dashboards.extend(dashboards.into_iter().map(|d| serde_json::from_value::<Dashboard>(d).expect("dashboard")));
    });
}

/// A navigation item "Home" for the dashboard `target`, which becomes the app's only page;
/// returns its element id.
pub fn nav_home<C: UiContext>(h: &mut Harness<'_, C>, target: &str) -> String {
    let (item_id, target) = (uid(), target.to_string());
    let item = NavigationItem { id: item_id.clone(), label: "Home".into(), kind: NavKind::Dashboard, target_id: Some(target), ..Default::default() };
    seed(h, move |c: &mut DocumentConfig| {
        c.design.navigation = vec![item];
        c.design.start_page = None;
    });
    format!("nav:{item_id}")
}

/// Opens the Runtime on the dashboard `target` through a "Home" navigation item.
pub fn open_in_runtime<C: UiContext>(h: &mut Harness<'_, C>, target: &str) {
    let nav = nav_home(h, target);
    h.switch_mode(ModeId::Run);
    wait(h);
    h.click(nav);
    wait(h);
}

/// The text of the input `id` inside `scope`.
pub fn value_in<C: UiContext>(h: &mut Harness<'_, C>, scope: &str, id: &str) -> Option<String> {
    h.try_find_in(id_of(scope), id_of(id)).and_then(|s| s.value().map(String::from))
}

fn id_of(text: &str) -> gpui_kit::SharedString {
    text.to_string().into()
}

pub fn edit_store_query<C: UiContext>(h: &mut Harness<'_, C>, query: SavedQuery) {
    edit_store(h, move |c: &mut DocumentConfig| c.saved_queries.push(query));
}

/// The dashboard page the Runtime shows.
pub fn runtime_page<C: UiContext>(h: &mut Harness<'_, C>) -> Entity<DashboardView> {
    let view = h.read_shell(|s, _| s.mode_view()).expect("a mode view");
    let mode: Entity<crate::ui::runtime::RunMode> = view.downcast().ok().expect("RunMode");
    let body = h.cx.read(|cx| mode.read(cx).body().cloned()).expect("the Run body is rendered");
    let page = h.cx.read(|cx| body.read(cx).page_view_entity()).expect("a page");
    page.downcast().ok().expect("a dashboard page")
}

/// Whether the list form embedded as the component titled `title` can go to a previous
/// page (the Runtime page's dashboard).
pub fn list_can_prev<C: UiContext>(h: &mut Harness<'_, C>, title: &str) -> bool {
    let id = component(h, title).id;
    let view = runtime_page(h);
    h.cx.read(|cx| {
        let embedded = view.read(cx).embedded(&id).expect("an embedded form");
        let renderer = embedded.read(cx).renderer().expect("a form renderer");
        let list = renderer.read(cx).list().expect("a list");
        list.read(cx).can_prev()
    })
}
