//! Unit tests of the query builder's derived grouping and duplicate join conditions.

use super::{
    model::{Aggregate, GroupColumn, JoinCondition},
    tests_unit::{agg, field, model, orders, sql},
};

#[test]
fn lists_the_columns_a_query_groups_by_like_the_compiler() {
    use super::sortgroup::grouping;
    let plain = model(|m| {
        m.sources = vec![orders()];
        m.fields = vec![field("a", "o", "status", true), field("b", "o", "id", true)];
    });
    assert!(grouping(&plain).is_empty());

    let aggregated = model(|m| {
        m.sources = vec![orders()];
        m.fields = vec![
            field("a", "o", "status", true),
            agg(field("b", "o", "id", true), Aggregate::Count),
            field("c", "o", "region", false),
        ];
        m.group_by = vec![GroupColumn { source: "o".into(), column: "placed".into() }];
    });
    assert_eq!(
        grouping(&aggregated),
        [("o.placed".to_string(), true), ("o.status".to_string(), false)]
    );
    // Every column listed is in the compiled GROUP BY.
    let compiled = sql(&aggregated);
    assert!(compiled.contains("GROUP BY \"o\".\"placed\", \"o\".\"status\""), "{compiled}");
}

#[test]
fn finds_join_conditions_that_repeat_an_earlier_one() {
    use super::joins::duplicate_conditions;
    let c = |left: &str, right: &str| JoinCondition {
        left_source: "o".into(),
        left_column: left.into(),
        right_column: right.into(),
    };
    assert!(duplicate_conditions(&[c("a", "id"), c("b", "id")]).is_empty());
    assert_eq!(duplicate_conditions(&[c("a", "id"), c("b", "id"), c("a", "id")]), [(3, 1)]);
    // Unfinished blank conditions are not duplicates of each other.
    assert!(duplicate_conditions(&[c("", ""), c("", "")]).is_empty());
}
