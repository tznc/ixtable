//! Dashboards as Runtime pages (port of `tests/integration/dashboard-rbac.test.tsx` and the
//! navigation cases of `dashboard-view.test.tsx`). Imports are explicit.

use super::test_support::{
    FakeQueries, define, install_fake, nav_home, open_in_runtime, runtime_page, seed_ops, uid,
};
use crate::archive::DocumentConfig;
use crate::data::{DataValue, NamedValue};
use crate::manager::AppError;
use crate::roles::{ObjectPermission, Permissions, Role};
use crate::ui::reports::test_support::{choose, id, wait_until};
use crate::ui::runtime::tests::{run_body, seed, wait, with_real_runner};
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;
use serde_json::json;

fn code<T>(result: Result<T, AppError>) -> String {
    match result {
        Ok(_) => "OK".into(),
        Err(error) => error.code,
    }
}

fn run_code(h: &Harness, query: &str) -> String {
    let run = crate::queries::run_saved_query(h.label.clone(), query.into(), vec![], None, None);
    code(tauri::async_runtime::block_on(run))
}

fn at(column: u32, row: u32, span: u32) -> serde_json::Value {
    json!({"column": column, "row": row, "columnSpan": span, "rowSpan": 2})
}

#[gpui_kit::test]
fn is_the_runtime_page_for_dashboard_navigation_items(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    seed_ops(&mut h);
    let fake = FakeQueries::new(5, false);
    install_fake(&mut h, &fake);
    open_in_runtime(&mut h, "d1");
    wait_until(&mut h, "the KPI", |h| h.try_label(id("kpi-value:Open orders")).as_deref() == Some("5"));
    assert_eq!(h.label(id("text:Notes:0")), "First.");
}

#[gpui_kit::test]
fn routes_dashboard_buttons_through_runtime_navigation(cx: &mut TestAppContext) {
    let _runner = with_real_runner();
    let mut h = Harness::new_document(cx);
    seed_ops(&mut h);
    let fake = FakeQueries::new(5, false);
    install_fake(&mut h, &fake);
    open_in_runtime(&mut h, "d1");
    wait_until(&mut h, "the KPI", |h| h.try_label(id("kpi-value:Open orders")).as_deref() == Some("5"));
    let visit = runtime_visit(&mut h);
    h.click(id("dash-button:Open dashboard"));
    wait(&mut h);
    assert!(runtime_visit(&mut h) > visit, "the action navigated the Runtime");
    wait_until(&mut h, "the dashboard again", |h| h.try_label(id("kpi-value:Open orders")).as_deref() == Some("5"));
}

fn runtime_visit(h: &mut Harness) -> u64 {
    let body = run_body(h);
    let runtime = h.cx.read(|cx| body.read(cx).runtime().clone());
    h.cx.read(|cx| runtime.read(cx).visit())
}

#[gpui_kit::test]
fn lets_a_role_granted_only_a_dashboard_read_the_dashboards_queries(cx: &mut TestAppContext) {
    let mut h = Harness::start_sized(cx, (1500.0, 1800.0));
    h.click("new-document");
    seed_sales_east_west(&mut h);
    let [total, regions, secret, summary, stray, form, home] = std::array::from_fn(|_| uid());
    let region = |required| crate::archive::QueryParameter {
        name: "region".into(),
        logical_type: "text".into(),
        default_value: None,
        required,
    };
    let query = |id: &String, name: &str, sql: &str, parameters| crate::archive::SavedQuery {
        id: id.clone(),
        name: name.into(),
        sql: sql.into(),
        parameters,
        ..Default::default()
    };
    let queries = vec![
        query(&total, "Total", "SELECT SUM(amount) AS total FROM sales WHERE ($region IS NULL OR region = $region)", vec![region(false)]),
        query(&regions, "Regions", "SELECT DISTINCT region FROM sales ORDER BY region", vec![]),
        query(&secret, "Secret", "SELECT * FROM sales", vec![]),
        query(&summary, "Summary", "SELECT id, region, amount FROM sales", vec![]),
        query(&stray, "Stray", "SELECT id FROM sales", vec![]),
    ];
    seed(&mut h, move |c: &mut DocumentConfig| c.saved_queries = queries);
    define(
        &mut h,
        vec![json!({
            "id": form, "name": "Summary", "source": {"kind": "query", "queryId": summary},
            "modes": ["list"], "listColumns": ["id", "region", "amount"], "pageSize": 25,
        })],
        vec![json!({
            "id": home, "name": "Home",
            "filters": [{"id": uid(), "name": "Region", "param": "region", "control": "select", "optionsQueryId": regions}],
            "components": [
                {"id": uid(), "kind": "kpi", "title": "Revenue", "queryId": total, "valueField": "total", "placement": at(1, 1, 6)},
                {"id": uid(), "kind": "table", "title": "Totals", "queryId": total, "placement": at(7, 1, 6)},
                {"id": uid(), "kind": "form", "title": "Summary form", "formId": form, "mode": "list", "placement": at(1, 3, 12)},
                {"id": uid(), "kind": "text", "title": "Note", "text": "Hi", "queryId": stray, "placement": at(1, 5, 6)},
            ],
        })],
    );
    let nav = nav_home(&mut h, &home);
    let nav_item = nav.trim_start_matches("nav:").to_string();
    let grant = ObjectPermission { kind: "dashboard".into(), id: home.clone(), read: true, ..Default::default() };
    let role = Role {
        id: "viewer".into(),
        name: "Viewer".into(),
        permissions: Permissions { navigation: vec![nav_item], objects: vec![grant], ..Default::default() },
    };
    seed(&mut h, move |c: &mut DocumentConfig| c.roles = vec![role]);

    h.switch_mode(ModeId::Run);
    wait(&mut h);
    h.click("role:viewer");
    wait(&mut h);
    h.click(nav);
    wait_until(&mut h, "the revenue", |h| h.try_label(id("kpi-value:Revenue")).as_deref() == Some("500"));
    wait_until(&mut h, "the totals", |h| h.try_label(id("cell:Totals:0:0")).as_deref() == Some("500"));
    for name in ["Revenue", "Totals", "Note"] {
        assert!(!h.exists(id(&format!("gate:{name}"))), "{name} is not gated");
    }
    wait_until(&mut h, "the denied form", |h| {
        h.try_label_in(id("widget:Summary form"), id("form-denied")).as_deref() == Some("You do not have access to Summary.")
    });
    let view = runtime_page(&mut h);
    wait_until(&mut h, "the region choices", |h| h.cx.read(|cx| !view.read(cx).options.is_empty()));
    choose(&mut h, "filter:Region", "East");
    wait_until(&mut h, "East revenue", |h| h.try_label(id("kpi-value:Revenue")).as_deref() == Some("100"));

    // The backend enforces the same grants.
    assert_eq!(run_code(&h, &secret), "FORBIDDEN");
    assert_eq!(run_code(&h, &total), "OK");
    assert_eq!(run_code(&h, &regions), "OK");
    assert_eq!(run_code(&h, &summary), "FORBIDDEN");
    assert_eq!(run_code(&h, &stray), "FORBIDDEN");
    let label = h.label.clone();
    assert_eq!(code(crate::read_table_page(label.clone(), "sales".into(), 0, 10, vec![], vec![], None)), "FORBIDDEN");
    let row = vec![NamedValue { column: "id".into(), value: DataValue::Integer(9) }];
    assert_eq!(code(crate::recordstore::commands::insert_row(label, "sales".into(), row, None)), "FORBIDDEN");
}

fn seed_sales_east_west(h: &mut Harness) {
    h.create_table("sales", &[("id", "INTEGER", 1), ("region", "TEXT", 0), ("amount", "REAL", 0)]);
    for (i, (region, amount)) in [("East", 100.0), ("West", 400.0)].into_iter().enumerate() {
        h.insert_row(
            "sales",
            &[
                ("id", DataValue::Integer(i as i64 + 1)),
                ("region", DataValue::Text(region.into())),
                ("amount", DataValue::Real(amount)),
            ],
        );
    }
}

#[gpui_kit::test]
fn marks_the_queries_of_an_unreadable_dashboard_denied_in_a_hosted_view(cx: &mut TestAppContext) {
    use super::{DashboardView, QueryState};
    use crate::ui::runtime::{Runtime, RuntimeEnv};
    use gpui_kit::{AppContext as _, Bounds, Point, WindowBounds, WindowOptions, px, size};

    let mut h = Harness::start_sized(cx, (1200.0, 900.0));
    h.click("new-document");
    seed_sales_east_west(&mut h);
    let (total, home) = (uid(), uid());
    let query = crate::archive::SavedQuery {
        id: total.clone(),
        name: "Total".into(),
        sql: "SELECT SUM(amount) AS total FROM sales".into(),
        ..Default::default()
    };
    seed(&mut h, move |c: &mut DocumentConfig| c.saved_queries = vec![query]);
    define(
        &mut h,
        vec![],
        vec![json!({"id": home, "name": "Home", "components": [
            {"id": uid(), "kind": "kpi", "title": "Revenue", "queryId": total, "valueField": "total", "placement": at(1, 1, 6)}]})],
    );
    let role = Role { id: "nobody".into(), name: "Nobody".into(), permissions: Permissions::default() };
    seed(&mut h, move |c: &mut DocumentConfig| c.roles = vec![role]);

    // Host the dashboard in a window of its own with a Runtime previewing the role.
    let shell = h.shell();
    let (label, store, bus) = h.read_shell(|s, _| (s.label().to_string(), s.store.clone(), s.bus.clone()));
    let (_window, view) = h.cx.update(|cx| {
        let runtime = cx.new(|_| Runtime::new(&label));
        runtime.update(cx, |r, cx| r.set_role_id(Some("nobody".into()), cx));
        let env = RuntimeEnv { label: label.clone().into(), store, bus, runtime: Some(runtime) };
        let bounds = Bounds { origin: Point::default(), size: size(px(900.), px(700.)) };
        gpui_kit::open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
            cx,
            |_, cx| cx.new(|cx| DashboardView::new(shell, env, &home, serde_json::Map::new(), None, cx)),
        )
        .expect("open the hosting window")
    });
    wait(&mut h);
    let dashboard = h.config().dashboards[0].clone();
    h.cx.read(|cx| {
        let view = view.read(cx);
        assert!(view.runnable_queries(&dashboard, cx).is_empty());
        let state = view.component_state(&dashboard, &dashboard.components[0], cx);
        assert!(matches!(state, Some(QueryState::Denied)), "denied, got a different state");
    });
    assert_eq!(run_code(&h, &total), "FORBIDDEN", "the backend enforces the previewed role");
}
