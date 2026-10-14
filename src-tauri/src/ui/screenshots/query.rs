//! Query mode: the visual builder, the generated SQL, a multi-condition outer join, and a
//! typed SQL query (`query-*`, `app-qa-05-query`). Port of
//! scripts/screenshot/specs/query.spec.tsx and the query capture of app-shell.spec.tsx.

use crate::ui::query::tests_support::{
    click, editor_text, exists, expect_grid, field_id, fill, open_query, open_section, pick, type_sql,
    with_mode,
};
use crate::ui::screenshot::context;
use crate::ui::testing::Harness;
use std::time::Duration;

/// A tall window, so the whole builder shows without a scroll fold.
const TALL: (f32, f32) = (1280.0, 1900.0);

fn open_new_document(cx: &mut gpui_kit::HeadlessAppContext) -> Harness<'_, gpui_kit::HeadlessAppContext> {
    let mut h = Harness::start_sized(cx, TALL);
    h.click("new-document");
    assert!(h.has_shell());
    open_query(&mut h);
    h
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn query_builder() {
    let mut cx = context();
    let mut h = open_new_document(&mut cx);
    click(&mut h, "query-new");
    pick(&mut h, "source-table", "orders");
    click(&mut h, "suggest-join:Join customers on o.customer_id = customers.id");
    click(&mut h, "col:c.name");
    click(&mut h, "col:o.amount");
    pick(&mut h, "aggregate:o.amount", "sum");
    fill(&mut h, "alias:o.amount", "total");
    click(&mut h, "add-row-count");
    fill(&mut h, "alias:Row count", "orders");

    open_section(&mut h, "query-parameters");
    fill(&mut h, "new-param-name", "min_amount");
    click(&mut h, "add-param");
    pick(&mut h, "param-type:min_amount", "number");
    fill(&mut h, "param-default:min_amount", "6");
    open_section(&mut h, "query-filters");
    click(&mut h, "add-filter");
    pick(&mut h, "filter-1-column", "o.amount");
    pick(&mut h, "filter-1-operator", ">=");
    pick(&mut h, "filter-1-compares-with", "param");
    pick(&mut h, "filter-1-parameter", "min_amount");
    open_section(&mut h, "query-sort");
    click(&mut h, "add-sort");
    let sum = field_id(&mut h, "o.amount");
    pick(&mut h, "sort-1-field", &sum);
    pick(&mut h, "sort-1-direction", "desc");
    fill(&mut h, "query-name", "Customer totals");

    // Orders of at least 6, summed per customer, largest first.
    h.advance(Duration::from_millis(500));
    expect_grid(&mut h, &[&["Bolt", "50", "1"], &["ACME", "40", "2"]]);
    h.capture(
        "query-01-builder",
        &[
            "The builder shows orders joined to customers with the selected fields, sum, and row count.",
            "The min_amount parameter, the amount filter that uses it, and the sort are listed.",
            "The live preview below lists one row per customer.",
        ],
    );

    click(&mut h, "tab-sql");
    assert!(h.label("generated-sql").contains("\"o\".\"amount\" >= $min_amount"));
    h.capture(
        "query-02-generated-sql",
        &["The SQL tab shows the generated SELECT with the join, GROUP BY, and $min_amount filter."],
    );
    click(&mut h, "tab-builder");
    click(&mut h, "query-save");
    assert_eq!(h.config().saved_queries.len(), 1);
    assert!(!exists(&mut h, "query-unsaved"));
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn query_group_by_multi_join() {
    let mut cx = context();
    let mut h = open_new_document(&mut cx);
    click(&mut h, "query-new");
    pick(&mut h, "source-table", "orders");
    click(&mut h, "suggest-join:Join customers on o.customer_id = customers.id");
    pick(&mut h, "join-kind:c", "left");
    click(&mut h, "join-add-condition:c");
    pick(&mut h, "join-left:c:2", "o.customer_id");
    pick(&mut h, "join-right:c:2", "id");
    click(&mut h, "col:c.name");
    click(&mut h, "col:o.amount");
    pick(&mut h, "aggregate:o.amount", "sum");
    fill(&mut h, "alias:o.amount", "total");
    click(&mut h, "col:c.id");
    click(&mut h, "output:c.id");
    open_section(&mut h, "query-group");
    pick(&mut h, "group-by-column", "c.id");
    click(&mut h, "add-group-by");
    assert_eq!(h.label("group-by:c.id"), "c.id");
    open_section(&mut h, "query-sort");
    click(&mut h, "add-sort");
    let id = field_id(&mut h, "c.id");
    pick(&mut h, "sort-1-field", &id);
    pick(&mut h, "sort-1-direction", "desc");

    h.advance(Duration::from_millis(500));
    expect_grid(&mut h, &[&["Core", "1"], &["Bolt", "55"], &["ACME", "40"]]);
    h.capture(
        "query-03-group-by-multi-join",
        &[
            "The customers join is a Left join with two ON conditions listed under it.",
            "The Group by section lists c.id as \"Added here\" (selected but not output) and o.amount is summed as total.",
            "The preview lists Core, Bolt, then ACME with their totals.",
        ],
    );

    click(&mut h, "tab-sql");
    let sql = h.label("generated-sql");
    assert!(sql.contains(
        "LEFT JOIN \"customers\" AS \"c\" ON \"o\".\"customer_id\" = \"c\".\"id\" AND \"o\".\"customer_id\" = \"c\".\"id\""
    ));
    assert!(sql.contains("GROUP BY \"c\".\"id\", \"c\".\"name\""));
    h.capture(
        "query-04-group-by-sql",
        &["The SQL shows a LEFT JOIN with two AND-ed conditions, GROUP BY c.id, c.name, and ORDER BY c.id DESC."],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn query_sql_editor() {
    let mut cx = context();
    let mut h = Harness::start(&mut cx);
    h.click("new-document");
    open_query(&mut h);
    click(&mut h, "query-new");
    click(&mut h, "tab-sql");
    type_sql(&mut h, "SELECT name FROM customers");
    assert_eq!(editor_text(&mut h), "SELECT name FROM customers");
    click(&mut h, "query-run");
    expect_grid(&mut h, &[&["ACME"], &["Bolt"], &["Core"]]);
    click(&mut h, "query-save");
    let saved = h.config().saved_queries;
    assert_eq!(saved.len(), 1);
    assert!(exists(&mut h, &format!("query-open:{}", saved[0].id)));
    assert!(with_mode(&mut h, |q, _| q.draft().is_some_and(|d| !d.is_new)));
    h.capture(
        "app-qa-05-query",
        &[
            "The SQL editor has query naming, Run, Save, and Delete actions.",
            "Typed query results appear below the editor.",
            "The saved query appears selected in the sidebar without replacing table metadata.",
        ],
    );
}
