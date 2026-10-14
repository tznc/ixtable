//! Query editor flows on the Data page. Port of tests/integration/query-mode.test.tsx and
//! tests/integration/action-queries.test.tsx. Imports are explicit: `gpui_kit::*`
//! would shadow `#[test]`.

use super::{
    Tab,
    tests_support::{
        click, editor_text, exists, expect_grid, field_id, fill, open_query_mode, pick, type_sql,
        wait_until, with_mode,
    },
};
use crate::{
    archive::{ActionKind, ActionSpec, QueryParameter, SavedQuery},
    data::{DataValue, NamedValue},
    ui::testing::Harness,
};
use gpui_kit::TestAppContext;
use serde_json::json;
use std::time::Duration;

fn text_value(h: &mut Harness, row: usize, column: usize, table: &str) -> String {
    crate::ui::testing::shown(&h.read_page(table).rows[row][column])
}

fn saved(h: &Harness) -> Vec<SavedQuery> {
    h.config().saved_queries
}

fn draft_name(h: &mut Harness) -> String {
    with_mode(h, |q, _| q.draft().map(|d| d.query.name.clone()).unwrap_or_default())
}

#[gpui_kit::test]
fn builds_a_joined_grouped_query_with_a_filter_parameter_previews_saves_reopens_and_runs_it(
    cx: &mut TestAppContext,
) {
    let mut h = open_query_mode(cx);
    click(&mut h, "new-query");
    assert_eq!(with_mode(&mut h, |q, _| q.tab), Tab::Builder);

    pick(&mut h, "source-table", "orders");
    click(&mut h, "suggest-join:Join customers on o.customer_id = customers.id");
    assert_eq!(h.label("join-kind:c"), "Inner join");
    click(&mut h, "col:c.name");
    click(&mut h, "col:o.amount");
    pick(&mut h, "aggregate:o.amount", "sum");
    fill(&mut h, "alias:o.amount", "total");
    click(&mut h, "add-row-count");
    fill(&mut h, "alias:Row count", "orders");

    fill(&mut h, "new-param-name", "min_amount");
    click(&mut h, "add-param");
    pick(&mut h, "param-type:min_amount", "number");
    fill(&mut h, "param-default:min_amount", "6");

    click(&mut h, "add-filter");
    pick(&mut h, "filter-1-column", "o.amount");
    pick(&mut h, "filter-1-operator", ">=");
    pick(&mut h, "filter-1-compares-with", "param");
    pick(&mut h, "filter-1-parameter", "min_amount");

    click(&mut h, "add-sort");
    let amount = field_id(&mut h, "o.amount");
    pick(&mut h, "sort-1-field", &amount);
    pick(&mut h, "sort-1-direction", "desc");

    h.advance(Duration::from_millis(500));
    expect_grid(&mut h, &[&["Bolt", "50", "1"], &["ACME", "40", "2"]]);

    click(&mut h, "tab-sql");
    let generated = h.label("generated-sql");
    assert!(generated.contains("INNER JOIN \"customers\" AS \"c\" ON \"o\".\"customer_id\" = \"c\".\"id\""));
    assert!(generated.contains("\"o\".\"amount\" >= $min_amount"));
    click(&mut h, "tab-builder");

    fill(&mut h, "query-name", "Customer totals");
    click(&mut h, "query-save");
    let queries = saved(&h);
    let saved_query = queries.first().expect("the query is saved").clone();
    assert_eq!(saved_query.name, "Customer totals");
    assert_eq!(
        with_mode(&mut h, |q, _| q.draft().map(|d| (d.query.id.clone(), d.is_new))),
        Some((saved_query.id.clone(), false))
    );
    let parameter = &saved_query.parameters[0];
    assert_eq!(
        (parameter.name.as_str(), parameter.logical_type.as_str(), parameter.default_value.clone()),
        ("min_amount", "number", Some(json!("6")))
    );
    let aliases: Vec<String> = saved_query.builder.as_ref().unwrap()["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["alias"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(aliases, ["o", "c"]);
    assert!(saved_query.sql.contains("GROUP BY \"c\".\"name\""));

    click(&mut h, "new-query");
    assert!(!exists(&mut h, "col:o.amount"));
    click(&mut h, &format!("query:{}", saved_query.id));
    assert!(exists(&mut h, "col:o.amount"));
    let model = with_mode(&mut h, |q, _| q.model()).unwrap();
    let amount_field = model.fields.iter().find(|f| f.column == "amount").unwrap();
    assert_eq!(amount_field.aggregate, Some(super::model::Aggregate::Sum));
    assert_eq!(h.label("aggregate:o.amount"), "Sum");

    fill(&mut h, "param-value:min_amount", "20");
    click(&mut h, "query-run");
    expect_grid(&mut h, &[&["Bolt", "50", "1"], &["ACME", "30", "1"]]);

    let result = tauri::async_runtime::block_on(crate::queries::run_saved_query(
        h.label.clone(),
        saved_query.id.clone(),
        vec![NamedValue {
            column: "min_amount".into(),
            value: DataValue::Real(40.0),
        }],
        None,
        None,
    ))
    .unwrap();
    assert_eq!(result.columns, ["name", "total", "orders"]);
    assert_eq!(
        result.rows,
        vec![vec![
            DataValue::Text("Bolt".into()),
            DataValue::Real(50.0),
            DataValue::Integer(1)
        ]]
    );
    assert!(!result.truncated);
}

#[gpui_kit::test]
fn saves_sql_with_a_required_parameter_and_rejects_mutating_sql_on_save(cx: &mut TestAppContext) {
    let mut h = open_query_mode(cx);
    click(&mut h, "new-query");
    click(&mut h, "tab-sql");
    type_sql(&mut h, "SELECT name FROM customers WHERE id >= $min ORDER BY id");
    assert_eq!(h.label("query-mode-badge"), "SQL");
    click(&mut h, "declare:min");
    pick(&mut h, "param-type:min", "integer");
    click(&mut h, "param-required:min");
    fill(&mut h, "query-name", "Customers from");
    click(&mut h, "query-save");
    let first = saved(&h).into_iter().next().expect("the query is saved");
    assert_eq!(first.sql, "SELECT name FROM customers WHERE id >= $min ORDER BY id");
    assert!(first.builder.is_none());
    assert_eq!(
        (first.parameters[0].name.as_str(), first.parameters[0].logical_type.as_str(), first.parameters[0].required),
        ("min", "integer", true)
    );

    click(&mut h, "query-run");
    wait_until(&mut h, "the required-parameter error", |h| h.exists("run-error"));
    assert!(h.label("run-error").contains("$min is required"), "{}", h.label("run-error"));
    fill(&mut h, "param-value:min", "2");
    click(&mut h, "query-run");
    expect_grid(&mut h, &[&["Bolt"], &["Core"]]);

    type_sql(&mut h, "DELETE FROM customers");
    click(&mut h, "query-save");
    let error = h.label("query-error");
    assert!(error.contains("read-only"), "{error}");
    assert!(error.contains("migration"), "{error}");
    assert!(saved(&h)[0].sql.contains("SELECT"));

    h.dialogs.queue_confirm(true);
    click(&mut h, "query-duplicate");
    assert_eq!(draft_name(&mut h), "Customers from copy");
    let copy = saved(&h).into_iter().find(|q| q.name == "Customers from copy").expect("the copy is saved");
    assert_eq!(h.read_shell(|s, _| s.selection.clone()).map(|s| s.id), Some(copy.id.clone()));
    // Renaming is editing the name and saving.
    fill(&mut h, "query-name", "Second");
    click(&mut h, "query-save");
    assert!(saved(&h).iter().any(|q| q.name == "Second"));
    h.dialogs.queue_confirm(true);
    click(&mut h, "query-delete");
    let names: Vec<String> = saved(&h).into_iter().map(|q| q.name).collect();
    assert_eq!(names, ["Customers from"]);
    assert!(h.read_shell(|s, _| s.selection.is_none()), "the Data page goes back to the diagram");
}

#[gpui_kit::test]
fn saves_parameterized_sql_through_save_query_without_parameter_values(cx: &mut TestAppContext) {
    let h = open_query_mode(cx);
    let config = crate::save_query(
        h.label.clone(),
        None,
        "Since".into(),
        "SELECT name FROM customers WHERE id >= $min AND name <> 'Delete; me';".into(),
        None,
    )
    .unwrap();
    assert!(config.saved_queries.iter().any(|q| q.name == "Since"));
}

#[gpui_kit::test]
fn shows_progress_and_a_cancel_button_for_a_long_query_and_cancels_it(cx: &mut TestAppContext) {
    let mut h = open_query_mode(cx);
    click(&mut h, "new-query");
    click(&mut h, "tab-sql");
    type_sql(
        &mut h,
        "SELECT count(*) FROM range(100000000) a CROSS JOIN range(100000) b WHERE a.range + b.range < 0",
    );
    click(&mut h, "query-run");
    assert!(h.label("run-status").starts_with("Running query"));
    assert!(!exists(&mut h, "cancel-query"));
    for _ in 0..10 {
        h.advance(Duration::from_millis(250));
    }
    assert!(exists(&mut h, "cancel-query"));
    assert!(exists(&mut h, "run-progress"));
    click(&mut h, "cancel-query");
    wait_until(&mut h, "the cancelled run", |h| h.exists("run-error"));
    assert_eq!(h.label("run-error"), "Query cancelled.");
}

#[gpui_kit::test]
fn joins_on_several_conditions_with_an_outer_join_groups_by_and_sorts_on_fields_that_are_not_output(
    cx: &mut TestAppContext,
) {
    let mut h = open_query_mode(cx);
    click(&mut h, "new-query");
    pick(&mut h, "source-table", "orders");
    click(&mut h, "suggest-join:Join customers on o.customer_id = customers.id");
    click(&mut h, "join-kind:c");
    for kind in ["inner", "left", "right", "full"] {
        assert!(exists(&mut h, &format!("join-kind:c/{kind}")), "{kind}");
    }
    click(&mut h, "join-kind:c/right");
    click(&mut h, "join-add-condition:c");
    pick(&mut h, "join-left:c:2", "o.customer_id");
    pick(&mut h, "join-right:c:2", "id");
    click(&mut h, "join-add-condition:c");
    click(&mut h, "join-condition-remove:c:3");
    assert!(!exists(&mut h, "join-left:c:3"));

    click(&mut h, "col:c.name");
    click(&mut h, "col:o.amount");
    pick(&mut h, "aggregate:o.amount", "sum");
    click(&mut h, "col:c.id");
    click(&mut h, "output:c.id");
    pick(&mut h, "group-by-column", "c.id");
    click(&mut h, "add-group-by");
    assert_eq!(h.label("group-by:c.id"), "c.id");
    click(&mut h, "add-sort");
    let id = field_id(&mut h, "c.id");
    pick(&mut h, "sort-1-field", &id);
    pick(&mut h, "sort-1-direction", "desc");

    h.advance(Duration::from_millis(500));
    expect_grid(&mut h, &[&["Core", "1"], &["Bolt", "55"], &["ACME", "40"]]);
    click(&mut h, "tab-sql");
    let sql = h.label("generated-sql");
    assert!(sql.contains(
        "RIGHT JOIN \"customers\" AS \"c\" ON \"o\".\"customer_id\" = \"c\".\"id\" AND \"o\".\"customer_id\" = \"c\".\"id\""
    ), "{sql}");
    assert!(sql.contains("GROUP BY \"c\".\"id\", \"c\".\"name\""), "{sql}");
    assert!(sql.contains("ORDER BY \"c\".\"id\" DESC"), "{sql}");
    assert!(!sql.contains("\"c\".\"id\" AS"), "{sql}");
}

// ---- tests/integration/action-queries.test.tsx -----------------------------------------

fn orders(h: &mut Harness) {
    let column = |name: &str, ty: &str, pk: u32| super::tests_support::column(name, ty, pk);
    super::tests_support::create_table(
        h,
        json!({
            "name": "orders",
            "columns": [
                column("id", "INTEGER", 1), column("total", "REAL", 0),
                column("due", "DATE", 0), column("status", "TEXT", 0),
            ],
            "foreignKeys": [], "checks": [], "withoutRowid": false,
        }),
    );
    for (id, total, due) in [(1, 10.0, "2024-01-31"), (2, 20.0, "2024-02-29"), (3, 30.0, "2024-03-31")] {
        h.insert_row(
            "orders",
            &[
                ("id", DataValue::Integer(id)),
                ("total", DataValue::Real(total)),
                ("due", DataValue::Date(due.into())),
                ("status", DataValue::Text("open".into())),
            ],
        );
    }
}

fn author_update(h: &mut Harness) {
    click(h, "new-query");
    fill(h, "query-name", "Extend due dates");
    pick(h, "query-type", "update");
    pick(h, "target-table", "orders");
    assert_eq!(editor_text(h), "UPDATE orders\nSET ... \nWHERE ...");
    type_sql(h, "UPDATE orders SET due = due + 7, status = 'extended' WHERE total >= 20");
    assert_eq!(editor_text(h), "UPDATE orders SET due = due + 7, status = 'extended' WHERE total >= 20");
    assert_eq!(with_mode(h, |q, _| q.draft().unwrap().query.sql.clone()), editor_text(h));
    click(h, "query-save");
    assert_eq!(with_mode(h, |q, _| q.action_blocked()), None);
}

#[gpui_kit::test]
fn authors_previews_and_runs_an_action_query_in_query_mode(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    orders(&mut h);
    author_update(&mut h);
    assert_eq!(h.label("query-mode-badge"), "Action query");

    click(&mut h, "preview-changes");
    wait_until(&mut h, "the preview", |h| h.exists("action-result") || h.exists("action-error"));
    assert!(!h.exists("action-error"), "{}", h.label("action-error"));
    assert_eq!(h.label("action-result"), "Would update 2 rows in orders. Nothing was changed.");
    let statuses: Vec<String> = (0..3).map(|i| text_value(&mut h, i, 3, "orders")).collect();
    assert_eq!(statuses, ["open", "open", "open"]);

    click(&mut h, "run-action");
    wait_until(&mut h, "the run", |h| {
        h.try_label("action-result").is_some_and(|t| t.starts_with("Updated"))
    });
    assert_eq!(h.label("action-result"), "Updated 2 rows in orders.");
    assert!(h.dialogs.calls.lock().unwrap().iter().any(|c| c.contains(
        "Run Extend due dates? It changes rows in orders and cannot be undone."
    )));
    let page = h.read_page("orders");
    let rows: Vec<(String, String)> = page
        .rows
        .iter()
        .map(|r| (crate::ui::testing::shown(&r[2]), crate::ui::testing::shown(&r[3])))
        .collect();
    assert_eq!(
        rows,
        [
            ("2024-01-31".to_string(), "open".to_string()),
            ("2024-03-07".to_string(), "extended".to_string()),
            ("2024-04-07".to_string(), "extended".to_string()),
        ]
    );
}

#[gpui_kit::test]
fn refuses_sql_that_writes_another_table_and_action_queries_as_read_sources(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    orders(&mut h);
    click(&mut h, "new-query");
    pick(&mut h, "query-type", "delete");
    pick(&mut h, "target-table", "orders");
    type_sql(&mut h, "DROP TABLE orders");
    click(&mut h, "query-save");
    assert!(h.label("query-error").contains("cannot use DROP"), "{}", h.label("query-error"));

    let shell = h.shell();
    h.cx.update(|cx| {
        shell.update(cx, |s, cx| {
            s.store.update(cx, |store, cx| {
                store
                    .update(
                        "Add purge",
                        |c| {
                            c.saved_queries = vec![SavedQuery {
                                id: "q-del".into(),
                                name: "Purge".into(),
                                sql: "DELETE FROM orders".into(),
                                action: Some(ActionSpec {
                                    kind: ActionKind::Delete,
                                    table: "orders".into(),
                                }),
                                ..Default::default()
                            }]
                        },
                        cx,
                    )
                    .unwrap();
            })
        })
    });
    let error = tauri::async_runtime::block_on(crate::queries::run_saved_query(
        h.label.clone(),
        "q-del".into(),
        vec![],
        None,
        None,
    ))
    .unwrap_err();
    assert!(error.message.contains("is an action query"), "{}", error.message);
    assert_eq!(h.read_page("orders").total, 3);
}

#[gpui_kit::test]
fn runs_action_queries_with_parameters_and_reports_changed_rows(cx: &mut TestAppContext) {
    // The trigger half of this flow (a sync trigger per changed row) needs the record-hook
    // runtime of the Automation port; here the run counts and the dry run are checked.
    let mut h = Harness::new_document(cx);
    orders(&mut h);
    let shell = h.shell();
    h.cx.update(|cx| {
        shell.update(cx, |s, cx| {
            s.store.update(cx, |store, cx| {
                store
                    .update(
                        "Add queries",
                        |c| {
                            c.saved_queries = vec![
                                SavedQuery {
                                    id: "q-close".into(),
                                    name: "Close big orders".into(),
                                    sql: "UPDATE orders SET status = 'closed' WHERE total > $min".into(),
                                    parameters: vec![QueryParameter {
                                        name: "min".into(),
                                        logical_type: "number".into(),
                                        default_value: None,
                                        required: false,
                                    }],
                                    action: Some(ActionSpec {
                                        kind: ActionKind::Update,
                                        table: "orders".into(),
                                    }),
                                    ..Default::default()
                                },
                                SavedQuery {
                                    id: "q-copy".into(),
                                    name: "Copy closed".into(),
                                    sql: "INSERT INTO orders (id, total, status) SELECT id + 100, total, 'copy' FROM orders WHERE status = 'closed'".into(),
                                    action: Some(ActionSpec {
                                        kind: ActionKind::Insert,
                                        table: "orders".into(),
                                    }),
                                    ..Default::default()
                                },
                            ]
                        },
                        cx,
                    )
                    .unwrap();
            })
        })
    });
    let min = |n: i64| {
        vec![NamedValue {
            column: "min".into(),
            value: DataValue::Integer(n),
        }]
    };
    let closed = crate::records::run_action_query(&h.label, "q-close", min(15), false, 0).unwrap();
    // Changed rows are listed only for tables with triggers; the count is always there.
    assert_eq!(closed.changed, 2);
    let copied = crate::records::run_action_query(&h.label, "q-copy", vec![], false, 0).unwrap();
    assert_eq!(copied.changed, 2);
    let dry = crate::records::run_action_query(&h.label, "q-close", min(0), true, 0).unwrap();
    assert_eq!(dry.changed, 5);
    assert_eq!(h.read_page("orders").total, 5);
}
