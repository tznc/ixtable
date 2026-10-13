//! Dashboards mode (`dashboards-NN-*`): designing a KPI, a bar chart and a filter, the live
//! view with and without a filter value, an expression error, and table styles and
//! conditions. Port of `scripts/screenshot/specs/dashboards.spec.tsx`.
//!
//! The window is tall so the whole properties panel is visible: the harness only clicks
//! what is on screen. The React spec generated the Orders list from the Design studio; here
//! the list is a query form that shows customer names, which keeps the spec on this mode.

use crate::archive::{DocumentConfig, QueryParameter, SavedQuery};
use crate::data::DataValue;
use crate::ui::pages::test_support::{
    component, dashboard, define, new_dashboard, pick_query, uid, wait_for_columns,
};
use crate::ui::reports::test_support::{choose, edit_store, id, wait_until};
use crate::ui::screenshot::context;
use crate::ui::testing::Harness;
use gpui_kit::HeadlessAppContext;
use serde_json::json;

const WINDOW: (f32, f32) = (1400.0, 1700.0);
const REVENUE: &str = "q-revenue";
const AMOUNTS: &str = "q-amounts";
const ORDERS: &str = "q-orders";

fn start(cx: &mut HeadlessAppContext) -> Harness<'_, HeadlessAppContext> {
    let mut h = Harness::start_sized(cx, WINDOW);
    h.click("new-document");
    assert!(h.has_shell());
    h
}

fn seed_sales(h: &mut Harness<'_, HeadlessAppContext>) {
    h.create_table("customers", &[("id", "INTEGER", 1), ("name", "TEXT", 0), ("city", "TEXT", 0)]);
    h.create_table(
        "orders",
        &[("id", "INTEGER", 1), ("customer_id", "INTEGER", 0), ("status", "TEXT", 0), ("amount", "REAL", 0)],
    );
    for (n, name, city) in [(1, "Northstar Goods", "Leeds"), (2, "Juniper Supply", "Bristol"), (3, "Harbor & Pine", "York")] {
        h.insert_row(
            "customers",
            &[("id", DataValue::Integer(n)), ("name", DataValue::Text(name.into())), ("city", DataValue::Text(city.into()))],
        );
    }
    let orders = [(1, 1, "open", 120.0), (2, 1, "shipped", 340.0), (3, 2, "open", 75.5), (4, 3, "shipped", 410.0), (5, 2, "cancelled", 20.0)];
    for (n, customer, status, amount) in orders {
        h.insert_row(
            "orders",
            &[
                ("id", DataValue::Integer(n)),
                ("customer_id", DataValue::Integer(customer)),
                ("status", DataValue::Text(status.into())),
                ("amount", DataValue::Real(amount)),
            ],
        );
    }
}

fn status_param() -> Vec<QueryParameter> {
    vec![QueryParameter { name: "status".into(), logical_type: "text".into(), default_value: None, required: false }]
}

fn query(id: &str, name: &str, sql: &str, parameters: Vec<QueryParameter>) -> SavedQuery {
    SavedQuery { id: id.into(), name: name.into(), sql: sql.into(), parameters, ..Default::default() }
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn dashboards_designer_view_and_filter() {
    let mut cx = context();
    let mut h = start(&mut cx);
    seed_sales(&mut h);
    let revenue = query(
        REVENUE,
        "Revenue by customer",
        "SELECT c.name AS customer, SUM(o.amount) AS revenue FROM orders o JOIN customers c ON c.id = o.customer_id WHERE ($status IS NULL OR o.status = $status) GROUP BY c.name ORDER BY c.name",
        status_param(),
    );
    edit_store(&mut h, move |c: &mut DocumentConfig| c.saved_queries.push(revenue));

    new_dashboard(&mut h);
    h.click("add-kpi");
    h.fill("prop-title", "Revenue");
    pick_query(&mut h, REVENUE);
    h.fill("prop-expression", "sum(rows.revenue)");
    h.fill("prop-format", "#,##0.00");
    h.click("add-chart");
    h.fill("prop-title", "Revenue by customer");
    pick_query(&mut h, REVENUE);
    wait_for_columns(&mut h, "the query columns", "prop-y:revenue");
    choose(&mut h, "prop-x", "customer");
    h.click(id("prop-y:revenue"));
    h.click("dashboard-settings");
    h.click("add-filter");
    let filter = dashboard(&h).filters[0].clone();
    assert_eq!(filter.param, "status");
    h.fill(id(&format!("filter-choices:{}", filter.id)), "open, shipped, cancelled");
    assert_eq!(dashboard(&h).filters[0].options, ["open", "shipped", "cancelled"]);
    h.capture(
        "dashboards-01-designer",
        &[
            "The dashboard canvas shows the Revenue KPI and the chart as grid items in design mode.",
            "Page settings list the status filter with its three choices.",
        ],
    );

    h.click("tab-view");
    wait_until(&mut h, "the revenue", |h| h.try_label(id("kpi-value:Revenue")).as_deref() == Some("965.50"));
    wait_until(&mut h, "the chart", |h| {
        h.try_label(id("chart:Revenue by customer")).is_some_and(|l| l.starts_with("Revenue by customer. Bar chart"))
    });
    assert!(h.exists("filter:Status"));
    h.capture(
        "dashboards-02-view",
        &[
            "The Revenue KPI reads 965.50 above a bar chart with one bar per customer.",
            "The status filter is shown above the components with no value selected.",
        ],
    );

    choose(&mut h, "filter:Status", "open");
    wait_until(&mut h, "the open revenue", |h| h.try_label(id("kpi-value:Revenue")).as_deref() == Some("195.50"));
    wait_until(&mut h, "the open chart", |h| h.try_label(id("chart-row:Revenue by customer:3")).is_none());
    h.capture(
        "dashboards-03-filtered",
        &["With status = open the KPI reads 195.50 and the chart shows only Juniper Supply and Northstar Goods."],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn dashboards_conditions_and_expression_error() {
    let mut cx = context();
    let mut h = start(&mut cx);
    seed_sales(&mut h);
    let amounts = query(AMOUNTS, "Order amounts", "SELECT id, status, amount FROM orders ORDER BY id", vec![]);
    let orders = query(
        ORDERS,
        "Orders with customers",
        "SELECT o.id, c.name AS customer, o.status, o.amount FROM orders o JOIN customers c ON c.id = o.customer_id WHERE ($status IS NULL OR o.status = $status) ORDER BY o.id",
        status_param(),
    );
    edit_store(&mut h, move |c: &mut DocumentConfig| c.saved_queries.extend([amounts, orders]));
    let form = uid();
    define(
        &mut h,
        vec![json!({
            "id": form, "name": "Orders list", "source": {"kind": "query", "queryId": ORDERS},
            "modes": ["list"], "listColumns": ["id", "customer", "status", "amount"], "pageSize": 25,
        })],
        vec![],
    );

    new_dashboard(&mut h);
    h.click("add-table");
    pick_query(&mut h, AMOUNTS);
    wait_for_columns(&mut h, "the query columns", "prop-column:amount");
    h.fill("prop-row-filter", "record.total > 0");
    wait_until(&mut h, "the filter problem", |h| h.exists(id("prop-row-filter-problem")));
    h.capture(
        "dashboards-04-expression-error",
        &["The table's Row filter 'record.total > 0' is marked invalid with an inline error naming the unknown field."],
    );
    h.fill("prop-row-filter", "record.amount > 0");
    wait_until(&mut h, "the problem to clear", |h| !h.exists(id("prop-row-filter-problem")));
    h.click("prop-add-style");
    choose(&mut h, "prop-style-0-column", "amount");
    h.fill("prop-style-0-when", "value > 300");
    choose(&mut h, "prop-style-0-tone", "Warning");

    h.click("add-form");
    choose(&mut h, "prop-form", "Orders list");
    choose(&mut h, "prop-form-mode", "list");
    h.fill("prop-enabled-when", "isnull(params.status)");
    assert!(!h.exists(id("prop-enabled-when-problem")));

    h.click("add-text");
    h.fill("prop-text", "Showing one status only");
    h.fill("prop-visible-when", "not(isnull(params.status))");
    assert!(!h.exists(id("prop-visible-when-problem")));

    h.click("dashboard-settings");
    h.click("add-filter");
    let filter = dashboard(&h).filters[0].clone();
    // No component's query declares $status, so nothing is picked automatically.
    h.fill(id(&format!("filter-param:{}", filter.id)), "status");
    h.fill(id(&format!("filter-name:{}", filter.id)), "Status");
    h.fill(id(&format!("filter-choices:{}", filter.id)), "open, shipped");
    let (table, list, note) = (component(&h, "Table 1"), component(&h, "Form 1"), component(&h, "Text 1"));
    assert_eq!(note.text.as_deref(), Some("Showing one status only"));
    assert_eq!(table.styles.len(), 1);
    assert_eq!(list.enabled_when.as_deref(), Some("isnull(params.status)"));
    assert_eq!(note.visible_when.as_deref(), Some("not(isnull(params.status))"));

    h.click("tab-view");
    wait_until(&mut h, "the table", |h| h.try_label(id("cell:Table 1:3:2")).as_deref() == Some("410"));
    assert_eq!(h.label(id("cell:Table 1:0:2")), "120");
    assert!(!h.exists(id("widget:Text 1")), "the note is hidden until a status is chosen");
    assert!(!h.exists(id("widget-disabled:Form 1")));
    choose(&mut h, "filter:Status", "open");
    wait_until(&mut h, "the note", |h| h.exists(id("widget:Text 1")));
    wait_until(&mut h, "the disabled form", |h| h.exists(id("widget-disabled:Form 1")));
    assert_eq!(h.label(id("widget-disabled:Form 1")), "Not available right now.");
    h.capture(
        "dashboards-05-conditions",
        &[
            "In the table, amounts above 300 (340, 410) carry a warning tone; smaller amounts are plain.",
            "With Status = open the 'Showing one status only' text appears and the embedded Orders list is greyed out as disabled, with a 'Not available right now.' note.",
            "The embedded Orders list shows customer names, not ids, and its New orders button fits inside the card.",
        ],
    );
}
