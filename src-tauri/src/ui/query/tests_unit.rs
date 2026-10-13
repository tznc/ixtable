//! Unit tests of the logic moved from TypeScript: the builder compiler and model
//! (tests/unit/query-builder.test.ts) and the action-query wording
//! (tests/unit/action-queries.test.ts, `describeRun`). The record-hook parts of that
//! file live in `records_tests.rs`.

use crate::forms::values::to_data_value;
use super::{
    action_text::describe_run,
    compile::{compile_builder, quote_ident, quote_literal, try_compile},
    model::{
        Aggregate, BuilderCondition, BuilderField, BuilderGroup, BuilderJoin, BuilderModel,
        BuilderSort, BuilderSource, Combinator, Direction, FilterOperator, FilterValue,
        GroupColumn, Item, JoinCondition, JoinKind, Literal, alias_for, builder_parameter_names,
        empty_model, normalize_model,
    },
    sql::{placeholder_names, to_named_values},
};
use crate::{archive::ActionKind, data::DataValue};
use serde_json::json;
use std::collections::BTreeMap;

fn model(patch: impl FnOnce(&mut BuilderModel)) -> BuilderModel {
    let mut m = empty_model();
    patch(&mut m);
    m
}

fn source(id: &str, table: &str, alias: &str) -> BuilderSource {
    BuilderSource {
        id: id.into(),
        table: table.into(),
        alias: alias.into(),
    }
}

fn orders() -> BuilderSource {
    source("s1", "orders", "o")
}

fn customers() -> BuilderSource {
    source("s2", "customers", "c")
}

fn field(id: &str, source: &str, column: &str, selected: bool) -> BuilderField {
    BuilderField {
        id: id.into(),
        source: source.into(),
        column: column.into(),
        alias: None,
        aggregate: None,
        selected,
    }
}

fn agg(mut f: BuilderField, aggregate: Aggregate) -> BuilderField {
    f.aggregate = Some(aggregate);
    f
}

fn aliased(mut f: BuilderField, alias: &str) -> BuilderField {
    f.alias = Some(alias.into());
    f
}

fn cond(id: &str, column: &str, operator: FilterOperator, value: Option<FilterValue>) -> BuilderCondition {
    BuilderCondition {
        id: id.into(),
        source: "o".into(),
        column: column.into(),
        aggregate: None,
        operator,
        value,
    }
}

fn text(value: &str) -> Option<FilterValue> {
    Some(FilterValue::Value {
        value: Literal::Text(value.into()),
    })
}

fn param(name: &str) -> Option<FilterValue> {
    Some(FilterValue::Param { name: name.into() })
}

fn group(id: &str, combinator: Combinator, items: Vec<Item>) -> BuilderGroup {
    BuilderGroup {
        id: id.into(),
        combinator,
        items,
    }
}

fn join(id: &str, kind: JoinKind, source: &str, conditions: Vec<JoinCondition>) -> BuilderJoin {
    BuilderJoin {
        id: id.into(),
        kind,
        source: source.into(),
        conditions,
    }
}

fn on(left_source: &str, left_column: &str, right_column: &str) -> JoinCondition {
    JoinCondition {
        left_source: left_source.into(),
        left_column: left_column.into(),
        right_column: right_column.into(),
    }
}

fn sql(m: &BuilderModel) -> String {
    compile_builder(m).expect("model compiles")
}

fn error(m: &BuilderModel) -> String {
    compile_builder(m).expect_err("model fails").0
}

#[test]
fn requires_a_source() {
    assert!(error(&empty_model()).contains("source"));
    assert!(try_compile(&empty_model()).1.contains("source"));
}

#[test]
fn selects_every_column_when_no_field_is_chosen() {
    assert_eq!(
        sql(&model(|m| m.sources = vec![orders()])),
        "SELECT *\nFROM \"orders\" AS \"o\""
    );
}

#[test]
fn selects_grouped_columns_when_group_by_is_set_and_no_field_is_output() {
    let m = model(|m| {
        m.sources = vec![orders()];
        m.group_by = vec![GroupColumn {
            source: "o".into(),
            column: "region".into(),
        }];
    });
    assert_eq!(
        sql(&m),
        "SELECT \"o\".\"region\"\nFROM \"orders\" AS \"o\"\nGROUP BY \"o\".\"region\""
    );
}

#[test]
fn requires_an_output_field_when_fields_exist() {
    let m = model(|m| {
        m.sources = vec![orders()];
        m.fields = vec![field("f", "o", "id", false)];
    });
    assert!(error(&m).contains("at least one output field"));
    assert!(try_compile(&m).1.contains("at least one output field"));
}

#[test]
fn groups_when_an_aggregate_is_used_only_for_sorting() {
    let m = model(|m| {
        m.sources = vec![orders()];
        m.fields = vec![
            field("r", "o", "region", true),
            agg(field("a", "o", "amount", false), Aggregate::Sum),
        ];
        m.order_by = vec![BuilderSort {
            id: "s".into(),
            field_id: "a".into(),
            direction: Direction::Desc,
        }];
    });
    assert_eq!(
        sql(&m),
        "SELECT \"o\".\"region\" AS \"region\"\nFROM \"orders\" AS \"o\"\nGROUP BY \"o\".\"region\"\nORDER BY sum(\"o\".\"amount\") DESC"
    );
}

#[test]
fn quotes_identifiers_and_aliases() {
    let m = model(|m| {
        m.sources = vec![source("s", "we\"ird table", "w")];
        m.fields = vec![aliased(
            field("f", "w", "col\"; DROP TABLE x; --", true),
            "A \"b\"",
        )];
    });
    assert_eq!(
        sql(&m),
        "SELECT \"w\".\"col\"\"; DROP TABLE x; --\" AS \"A \"\"b\"\"\"\nFROM \"we\"\"ird table\" AS \"w\""
    );
}

#[test]
fn compiles_inner_and_left_joins() {
    let m = model(|m| {
        m.sources = vec![orders(), customers(), source("s3", "regions", "r")];
        m.joins = vec![
            join("j1", JoinKind::Inner, "c", vec![on("o", "customer_id", "id")]),
            join("j2", JoinKind::Left, "r", vec![on("c", "region_id", "id")]),
        ];
        m.fields = vec![
            field("f1", "o", "id", true),
            aliased(field("f2", "c", "name", true), "customer"),
            field("f3", "r", "name", false),
        ];
    });
    assert_eq!(
        sql(&m),
        [
            "SELECT \"o\".\"id\" AS \"id\", \"c\".\"name\" AS \"customer\"",
            "FROM \"orders\" AS \"o\"",
            "INNER JOIN \"customers\" AS \"c\" ON \"o\".\"customer_id\" = \"c\".\"id\"",
            "LEFT JOIN \"regions\" AS \"r\" ON \"c\".\"region_id\" = \"r\".\"id\"",
        ]
        .join("\n")
    );
}

#[test]
fn rejects_joins_that_are_missing_or_refer_to_later_sources() {
    assert!(error(&model(|m| m.sources = vec![orders(), customers()])).contains("join"));
    let m = model(|m| {
        m.sources = vec![orders(), customers()];
        m.joins = vec![join("j", JoinKind::Inner, "c", vec![on("x", "a", "b")])];
    });
    assert!(error(&m).contains("refers to x"));
}

#[test]
fn groups_non_aggregated_fields_aggregates_and_filters_with_having_and_params() {
    let m = model(|m| {
        m.sources = vec![orders()];
        m.fields = vec![
            field("f1", "o", "customer", true),
            agg(field("f2", "o", "amount", true), Aggregate::Sum),
            aliased(agg(field("f3", "o", "*", true), Aggregate::Count), "orders"),
            agg(field("f4", "o", "sku", true), Aggregate::CountDistinct),
        ];
        m.filters = group(
            "g",
            Combinator::And,
            vec![
                Item::Condition(cond("c1", "status", FilterOperator::Eq, text("paid"))),
                Item::Group(group(
                    "g2",
                    Combinator::Or,
                    vec![
                        Item::Condition(cond("c2", "placed", FilterOperator::Ge, param("since"))),
                        Item::Condition(cond("c3", "placed", FilterOperator::IsNull, None)),
                    ],
                )),
            ],
        );
        let mut having = cond("h1", "amount", FilterOperator::Gt, param("min_total"));
        having.aggregate = Some(Aggregate::Sum);
        m.having = group("h", Combinator::And, vec![Item::Condition(having)]);
        m.order_by = vec![BuilderSort {
            id: "s".into(),
            field_id: "f2".into(),
            direction: Direction::Desc,
        }];
        m.limit = Some(10.0);
    });
    assert_eq!(
        sql(&m),
        [
            "SELECT \"o\".\"customer\" AS \"customer\", sum(\"o\".\"amount\") AS \"sum_amount\", count(*) AS \"orders\", count(DISTINCT \"o\".\"sku\") AS \"count_distinct_sku\"",
            "FROM \"orders\" AS \"o\"",
            "WHERE (\"o\".\"status\" = 'paid') AND ((\"o\".\"placed\" >= $since) OR (\"o\".\"placed\" IS NULL))",
            "GROUP BY \"o\".\"customer\"",
            "HAVING sum(\"o\".\"amount\") > $min_total",
            "ORDER BY sum(\"o\".\"amount\") DESC",
            "LIMIT 10",
        ]
        .join("\n")
    );
}

#[test]
fn uses_explicit_group_by_without_aggregates() {
    let m = model(|m| {
        m.sources = vec![orders()];
        m.fields = vec![field("f", "o", "customer", true)];
        m.group_by = vec![GroupColumn {
            source: "o".into(),
            column: "customer".into(),
        }];
    });
    assert!(sql(&m).contains("GROUP BY \"o\".\"customer\""));
}

#[test]
fn groups_by_a_column_that_is_not_output_and_groups_the_output_fields_with_it() {
    let m = model(|m| {
        m.sources = vec![orders()];
        m.fields = vec![field("f", "o", "status", true)];
        m.group_by = vec![GroupColumn {
            source: "o".into(),
            column: "region".into(),
        }];
    });
    assert_eq!(
        sql(&m),
        "SELECT \"o\".\"status\" AS \"status\"\nFROM \"orders\" AS \"o\"\nGROUP BY \"o\".\"region\", \"o\".\"status\""
    );
}

#[test]
fn sorts_by_a_field_that_is_not_output() {
    let m = model(|m| {
        m.sources = vec![orders()];
        m.fields = vec![field("a", "o", "id", true), field("b", "o", "placed", false)];
        m.order_by = vec![BuilderSort {
            id: "s".into(),
            field_id: "b".into(),
            direction: Direction::Desc,
        }];
    });
    assert_eq!(
        sql(&m),
        "SELECT \"o\".\"id\" AS \"id\"\nFROM \"orders\" AS \"o\"\nORDER BY \"o\".\"placed\" DESC"
    );
}

#[test]
fn asks_to_group_a_hidden_sort_field_when_the_query_groups() {
    let m = model(|m| {
        m.sources = vec![orders()];
        m.fields = vec![
            agg(field("a", "o", "id", true), Aggregate::Count),
            field("b", "o", "placed", false),
        ];
        m.order_by = vec![BuilderSort {
            id: "s".into(),
            field_id: "b".into(),
            direction: Direction::Asc,
        }];
    });
    assert!(try_compile(&m).1.contains("placed needs it in Group by"));
}

#[test]
fn compiles_right_and_full_outer_joins_with_several_conditions() {
    let joined = |kind| {
        sql(&model(|m| {
            m.sources = vec![orders(), customers()];
            m.joins = vec![join(
                "j",
                kind,
                "c",
                vec![on("o", "customer_id", "id"), on("o", "region", "region")],
            )];
        }))
    };
    assert!(joined(JoinKind::Right).contains(
        "RIGHT JOIN \"customers\" AS \"c\" ON \"o\".\"customer_id\" = \"c\".\"id\" AND \"o\".\"region\" = \"c\".\"region\""
    ));
    assert!(joined(JoinKind::Full).contains("FULL OUTER JOIN \"customers\" AS \"c\" ON"));
}

#[test]
fn escapes_literal_injection_attempts_and_rejects_unsafe_parameter_names() {
    let where_ = |value: FilterValue| {
        compile_builder(&model(|m| {
            m.sources = vec![orders()];
            m.filters = group(
                "g",
                Combinator::And,
                vec![Item::Condition(cond("c", "name", FilterOperator::Eq, Some(value)))],
            );
        }))
    };
    let literal = |value: Literal| FilterValue::Value { value };
    assert!(
        where_(literal(Literal::Text("x' OR '1'='1".into())))
            .unwrap()
            .contains("WHERE \"o\".\"name\" = 'x'' OR ''1''=''1'")
    );
    assert!(
        where_(literal(Literal::Number(5.0)))
            .unwrap()
            .contains("\"o\".\"name\" = 5")
    );
    assert!(
        where_(literal(Literal::Bool(true)))
            .unwrap()
            .contains("\"o\".\"name\" = TRUE")
    );
    for bad in ["x; DROP TABLE o", "1abc"] {
        let e = where_(FilterValue::Param { name: bad.into() }).unwrap_err();
        assert!(e.0.contains("parameter name"), "{e}");
    }
    let e = where_(literal(Literal::Number(f64::NAN))).unwrap_err();
    assert!(e.0.contains("finite"), "{e}");
}

#[test]
fn compiles_contains_and_starts_with_case_insensitively() {
    let m = model(|m| {
        m.sources = vec![orders()];
        m.filters = group(
            "g",
            Combinator::Or,
            vec![
                Item::Condition(cond("a", "name", FilterOperator::Contains, param("q"))),
                Item::Condition(cond("b", "name", FilterOperator::StartsWith, text("Ac"))),
            ],
        );
    });
    assert!(sql(&m).contains(
        "WHERE (contains(lower(CAST(\"o\".\"name\" AS VARCHAR)), lower($q))) OR (starts_with(lower(CAST(\"o\".\"name\" AS VARCHAR)), lower('Ac')))"
    ));
}

#[test]
fn rejects_aggregates_in_where_star_without_count_and_bad_limits() {
    let base = model(|m| m.sources = vec![orders()]);
    let mut with_aggregate = base.clone();
    let mut c = cond(
        "c",
        "a",
        FilterOperator::Gt,
        Some(FilterValue::Value {
            value: Literal::Number(1.0),
        }),
    );
    c.aggregate = Some(Aggregate::Sum);
    with_aggregate.filters = group("g", Combinator::And, vec![Item::Condition(c)]);
    assert!(error(&with_aggregate).contains("Having"));

    let mut star = base.clone();
    star.fields = vec![agg(field("f", "o", "*", true), Aggregate::Sum)];
    assert!(error(&star).contains("Count"));

    let mut limit = base.clone();
    limit.limit = Some(1.5);
    assert!(error(&limit).contains("Limit"));

    let mut sort = base;
    sort.order_by = vec![BuilderSort {
        id: "s".into(),
        field_id: "gone".into(),
        direction: Direction::Asc,
    }];
    assert!(error(&sort).contains("Sort"));
}

#[test]
fn quotes() {
    assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
    assert_eq!(quote_literal(&Literal::Text("it's".into())).unwrap(), "'it''s'");
    assert_eq!(quote_literal(&Literal::Null).unwrap(), "NULL");
}

#[test]
fn normalizes_partial_models_and_lists_parameter_names() {
    let mut m = normalize_model(&json!({ "sources": [{ "id": "s1", "table": "orders", "alias": "o" }] }));
    assert!(m.joins.is_empty());
    assert!(m.filters.items.is_empty());
    m.filters
        .items
        .push(Item::Condition(cond("c", "a", FilterOperator::Eq, param("p"))));
    let mut having = cond("d", "a", FilterOperator::Gt, param("p"));
    having.aggregate = Some(Aggregate::Sum);
    m.having.items.push(Item::Condition(having));
    assert_eq!(builder_parameter_names(&m), vec!["p".to_string()]);
}

#[test]
fn picks_unused_aliases() {
    assert_eq!(alias_for("orders", &[]), "o");
    assert_eq!(alias_for("orders", &["o".to_string()]), "o2");
    assert_eq!(alias_for("123", &[]), "t");
}

#[test]
fn converts_run_parameters_to_data_values() {
    assert_eq!(to_data_value(&json!(3)), DataValue::Integer(3));
    assert_eq!(to_data_value(&json!(1.5)), DataValue::Real(1.5));
    assert_eq!(to_data_value(&json!(null)), DataValue::Null);
    assert_eq!(
        to_data_value(&json!({ "type": "date", "value": "2024-01-01" })),
        DataValue::Date("2024-01-01".into())
    );
    let params: BTreeMap<String, String> =
        [("a".to_string(), "x".to_string()), ("b".to_string(), "false".to_string())].into();
    let named = to_named_values(&params);
    assert_eq!(named[0].column, "a");
    assert_eq!(named[0].value, DataValue::Text("x".into()));
    assert_eq!(to_data_value(&json!(false)), DataValue::Boolean(false));
}

#[test]
fn round_trips_a_model_through_the_stored_json() {
    let m = model(|m| {
        m.sources = vec![orders()];
        m.filters = group(
            "g",
            Combinator::And,
            vec![Item::Condition(cond(
                "c",
                "amount",
                FilterOperator::Ge,
                Some(FilterValue::Value {
                    value: Literal::Number(6.0),
                }),
            ))],
        );
    });
    let stored = m.to_value();
    assert_eq!(stored["filters"]["kind"], "group");
    assert_eq!(stored["filters"]["items"][0]["kind"], "condition");
    assert_eq!(stored["filters"]["items"][0]["value"], json!({ "kind": "value", "value": 6 }));
    assert_eq!(stored["limit"], json!(null));
    assert_eq!(normalize_model(&stored), m);
}

#[test]
fn finds_placeholders_outside_strings_comments_and_dollar_quotes() {
    let sql = "SELECT $a, '$no', \"$no2\" FROM t -- $skip\nWHERE x = $b /* $skip2 */ AND y = $a AND z = $$ $skip3 $$ AND q = $1";
    assert_eq!(placeholder_names(sql), vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn describes_runs_in_words() {
    assert_eq!(
        describe_run(2, 0, true, "orders", ActionKind::Update),
        "Would update 2 rows in orders. Nothing was changed."
    );
    assert_eq!(
        describe_run(1, 0, false, "orders", ActionKind::Delete),
        "Deleted 1 row in orders."
    );
    assert_eq!(
        describe_run(2, 5, false, "orders", ActionKind::Replace),
        "Inserted 2 rows in orders after removing 5 rows."
    );
    assert_eq!(
        describe_run(1234, 0, false, "orders", ActionKind::Insert),
        "Inserted 1,234 rows in orders."
    );
}
