//! Query builder sections: collapsed optional sections, derived grouping, duplicate
//! join warnings and run values prefilled from parameter defaults.

use super::{
    inputs::Field,
    tests_support::{click, exists, expect_grid, fill, open_query_mode, open_section, pick, type_sql, with_mode},
};
use crate::ui::testing::Harness;
use gpui_kit::{SharedString, TestAppContext};

#[gpui_kit::test]
fn collapses_empty_optional_sections_with_a_summary_and_opens_them_on_demand(cx: &mut TestAppContext) {
    let mut h = open_query_mode(cx);
    click(&mut h, "query-new");
    pick(&mut h, "source-table", "orders");
    // Sources and Fields are always open.
    assert!(exists(&mut h, "col:o.amount"));
    for (id, summary) in [
        ("query-filters", "No filters, all rows"),
        ("query-group", "No grouping"),
        ("query-having", "No conditions on aggregates"),
        ("query-sort", "Unsorted, all rows"),
        ("query-parameters", "No parameters"),
    ] {
        assert_eq!(h.label(SharedString::from(format!("{id}-summary"))), summary, "{id}");
    }
    assert!(!exists(&mut h, "add-filter"));
    assert!(!exists(&mut h, "add-sort"));
    click(&mut h, "query-filters-toggle");
    assert!(exists(&mut h, "add-filter"));
    assert!(!exists(&mut h, "query-filters-summary"));

    // A section with content collapses to a summary of it.
    click(&mut h, "col:o.amount");
    open_section(&mut h, "query-sort");
    click(&mut h, "add-sort");
    click(&mut h, "query-sort-toggle");
    assert_eq!(h.label("query-sort-summary"), "Sorted by o.amount ↑");
}

#[gpui_kit::test]
fn the_group_by_section_follows_the_aggregates_chosen_on_the_fields(cx: &mut TestAppContext) {
    let mut h = open_query_mode(cx);
    click(&mut h, "query-new");
    pick(&mut h, "source-table", "orders");
    click(&mut h, "col:o.customer_id");
    click(&mut h, "col:o.amount");
    assert_eq!(h.label("query-group-summary"), "No grouping");

    pick(&mut h, "aggregate:o.amount", "sum");
    // Grouping content opens the section.
    assert!(!exists(&mut h, "query-group-summary"));
    assert_eq!(h.label("group-by:o.customer_id"), "o.customer_id");
    assert_eq!(h.label("group-by-source:o.customer_id"), "From Fields");
    assert!(!exists(&mut h, "remove-group-by:o.customer_id"));
    assert!(!exists(&mut h, "group-by:o.amount"));
    assert_eq!(h.label("query-group"), "Group by");

    // Taking the aggregate off empties the list again.
    pick(&mut h, "aggregate:o.amount", "");
    assert!(!exists(&mut h, "group-by:o.customer_id"));
    assert!(exists(&mut h, "group-by-none"));

    // An extra column is listed as added there, and can be removed.
    pick(&mut h, "group-by-column", "o.id");
    click(&mut h, "add-group-by");
    assert_eq!(h.label("group-by-source:o.id"), "Added here");
    assert_eq!(h.label("group-by:o.customer_id"), "o.customer_id");
    assert_eq!(h.label("group-by-source:o.customer_id"), "From Fields");
    click(&mut h, "remove-group-by:o.id");
    assert!(!exists(&mut h, "group-by:o.id"));
}

#[gpui_kit::test]
fn warns_next_to_a_join_that_repeats_a_condition(cx: &mut TestAppContext) {
    let mut h = open_query_mode(cx);
    click(&mut h, "query-new");
    pick(&mut h, "source-table", "orders");
    click(&mut h, "suggest-join:Join customers on o.customer_id = customers.id");
    assert!(!exists(&mut h, "join-duplicate:c:2"));
    click(&mut h, "join-add-condition:c");
    assert!(!exists(&mut h, "join-duplicate:c:2"));
    pick(&mut h, "join-left:c:2", "o.customer_id");
    pick(&mut h, "join-right:c:2", "id");
    assert_eq!(
        h.label("join-duplicate:c:2"),
        "Condition 2 repeats condition 1. Remove one."
    );
    click(&mut h, "join-condition-remove:c:2");
    assert!(!exists(&mut h, "join-duplicate:c:2"));
}

#[gpui_kit::test]
fn run_with_values_start_as_the_parameter_defaults_and_show_the_type(cx: &mut TestAppContext) {
    let mut h = open_query_mode(cx);
    click(&mut h, "query-new");
    click(&mut h, "tab-sql");
    type_sql(&mut h, "SELECT id FROM orders WHERE amount >= $min ORDER BY id");
    open_section(&mut h, "query-parameters");
    click(&mut h, "declare:min");
    let value = |h: &mut Harness<'_, TestAppContext>| with_mode(h, |q, cx| q.input_text(&Field::ParamValue("min".into()), cx));
    assert_eq!(h.label("param-hint:min"), "text");
    assert_eq!(value(&mut h), "");
    pick(&mut h, "param-type:min", "number");
    assert_eq!(h.label("param-hint:min"), "number");
    fill(&mut h, "param-default:min", "6");
    h.settle();
    assert_eq!(value(&mut h), "6");

    // The prefilled value is the one that runs.
    click(&mut h, "query-run");
    expect_grid(&mut h, &[&["1"], &["2"], &["4"]]);

    // A typed value replaces it and stays when the default changes.
    fill(&mut h, "param-value:min", "40");
    fill(&mut h, "param-default:min", "7");
    h.settle();
    assert_eq!(value(&mut h), "40");
    click(&mut h, "query-run");
    expect_grid(&mut h, &[&["4"]]);
}
