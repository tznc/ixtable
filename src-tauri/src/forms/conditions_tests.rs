//! Port of tests/unit/conditions.test.ts.

use super::{
    SCAN_LIMIT, TRUNCATED_NOTICE, filter_inputs, filter_inputs_key, filter_names, page_label,
    paged_scan, row_filter, scan_filtered, tone_class, tone_for,
};
use crate::design::{ConditionalStyle, Tone};
use crate::expr::{Scope, check};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};

fn rule(when: &str, tone: Tone, column: Option<&str>) -> ConditionalStyle {
    ConditionalStyle {
        id: when.into(),
        when: when.into(),
        tone,
        column: column.map(Into::into),
    }
}

fn scope(value: Value) -> Scope {
    value.as_object().cloned().unwrap_or_default()
}

fn rec(value: Value) -> serde_json::Map<String, Value> {
    value.as_object().cloned().unwrap()
}

#[test]
fn tone_for_returns_the_first_matching_rules_tone() {
    let rules = vec![
        rule("value < 0", Tone::Negative, None),
        rule("value > 100", Tone::Positive, None),
        rule("true", Tone::Muted, None),
    ];
    assert_eq!(tone_for(&rules, &scope(json!({"value": -5})), None), Some(Tone::Negative));
    assert_eq!(tone_for(&rules, &scope(json!({"value": 500})), None), Some(Tone::Positive));
    assert_eq!(tone_for(&rules, &scope(json!({"value": 50})), None), Some(Tone::Muted));
    assert_eq!(tone_for(&[], &scope(json!({"value": 1})), None), None);
}

#[test]
fn tone_for_skips_blank_invalid_null_and_non_boolean_conditions() {
    let odd = vec![
        rule("", Tone::Warning, None),
        rule("value >", Tone::Warning, None),
        rule("value", Tone::Warning, None),
    ];
    assert_eq!(tone_for(&odd, &scope(json!({"value": 3})), None), None);
    let gt = vec![rule("value > 1", Tone::Warning, None)];
    assert_eq!(tone_for(&gt, &scope(json!({"value": null})), None), None);
}

#[test]
fn tone_for_limits_rules_to_a_column_when_one_is_given() {
    let table = vec![
        rule("value > 1", Tone::Warning, Some("a")),
        rule("value > 1", Tone::Emphasis, Some("b")),
    ];
    let s = scope(json!({"value": 5}));
    assert_eq!(tone_for(&table, &s, Some("b")), Some(Tone::Emphasis));
    assert_eq!(tone_for(&table, &s, Some("c")), None);
}

#[test]
fn tone_for_can_read_the_whole_row_and_maps_tones_to_classes() {
    let s = scope(json!({"record": {"status": "late"}, "value": 3}));
    assert_eq!(
        tone_for(&[rule("record.status = 'late'", Tone::Negative, None)], &s, None),
        Some(Tone::Negative)
    );
    assert_eq!(tone_class(Some(&Tone::Negative)), "tone tone-negative");
    assert_eq!(tone_class(None), "");
}

#[test]
fn row_filter_is_none_for_a_blank_filter_and_errors_on_a_syntax_error() {
    let empty = Scope::new();
    assert!(row_filter(Some(""), &empty).unwrap().is_none());
    assert!(row_filter(Some("  "), &empty).unwrap().is_none());
    assert!(row_filter(Some("record.qty >"), &empty).is_err());
}

#[test]
fn row_filter_evaluates_against_the_row_and_the_other_scope_roots() {
    let keep = row_filter(
        Some("record.qty > params.min and record.region = parent.region"),
        &scope(json!({"params": {"min": 1}, "parent": {"region": "East"}})),
    )
    .unwrap()
    .unwrap();
    assert!(keep(&rec(json!({"qty": 2, "region": "East"}))));
    assert!(!keep(&rec(json!({"qty": 2, "region": "West"}))));
    assert!(!keep(&rec(json!({"qty": 0, "region": "East"}))));
}

#[test]
fn row_filter_excludes_rows_whose_evaluation_is_null_or_fails() {
    let empty = Scope::new();
    let keep = row_filter(Some("record.qty > 1"), &empty).unwrap().unwrap();
    assert!(!keep(&rec(json!({"qty": null}))));
    let bad = row_filter(Some("record.name + 1 > 0"), &empty).unwrap().unwrap();
    assert!(!bad(&rec(json!({"name": "x"}))));
}

#[test]
fn filter_scope_names_are_known() {
    let names = filter_names(&["qty".to_string()]);
    let known: Vec<&str> = names.iter().map(String::as_str).collect();
    assert!(check("record.qty > 0 and parent.id = app.user.id", Some(&known)).is_empty());
    assert!(check("record.bogus > 0", Some(&known))[0].message.contains("Unknown field"));
    assert!(check("other > 0", Some(&known))[0].message.contains("Unknown name"));
}

/// A source of `size` rows `{n}`; `reads` records the size of every chunk read.
fn source(
    size: usize,
) -> (
    impl FnMut(usize, usize) -> Result<Vec<usize>, ()>,
    Rc<RefCell<Vec<usize>>>,
) {
    let reads = Rc::new(RefCell::new(vec![]));
    let reads_ref = reads.clone();
    let fetch = move |offset: usize, limit: usize| {
        let count = limit.min(size.saturating_sub(offset));
        reads_ref.borrow_mut().push(count);
        Ok((offset..offset + count).collect())
    };
    (fetch, reads)
}

#[test]
fn scan_filtered_scans_a_small_source_to_the_end() {
    let (fetch, _) = source(1200);
    let scan = scan_filtered(fetch, |n| n % 3 == 0, 1000, 500).unwrap();
    assert!(scan.exhausted);
    assert_eq!(scan.matches.len(), 400);
}

#[test]
fn scan_filtered_stops_when_enough_matches_are_found_but_never_past_the_hard_limit() {
    let (fetch, reads) = source(100_000);
    let scan = scan_filtered(fetch, |_| true, 50, 500).unwrap();
    assert_eq!(*reads.borrow(), [500]);
    assert!(scan.matches.len() >= 50);
    let (sparse, sparse_reads) = source(1_000_000);
    let none = scan_filtered(sparse, |_| false, 1, 500).unwrap();
    assert!(none.matches.is_empty() && !none.exhausted);
    assert_eq!(sparse_reads.borrow().iter().sum::<usize>(), SCAN_LIMIT);
}

#[test]
fn paged_scan_keeps_every_match_of_a_source_below_the_limit() {
    let (fetch, _) = source(12_000);
    let scan = paged_scan(fetch, |n| n % 1000 == 7).unwrap();
    assert!(!scan.truncated);
    assert_eq!(scan.matches, (0..12).map(|i| i * 1000 + 7).collect::<Vec<_>>());
}

#[test]
fn paged_scan_reports_truncation_when_the_source_is_larger_than_the_limit() {
    let (fetch, reads) = source(SCAN_LIMIT * 2);
    let scan = paged_scan(fetch, |n| n % 10_000 == 0).unwrap();
    assert!(scan.truncated);
    assert_eq!(scan.matches.len(), SCAN_LIMIT / 10_000);
    assert_eq!(reads.borrow().iter().sum::<usize>(), SCAN_LIMIT);
    assert_eq!(
        TRUNCATED_NOTICE,
        "Filter applied to the first 50,000 rows; some matches may be missing."
    );
}

#[test]
fn page_label_shows_the_range_and_the_total() {
    assert_eq!(page_label(0, 25, 40), "1\u{2013}25 of 40");
    assert_eq!(page_label(25, 15, 40), "26\u{2013}40 of 40");
    assert_eq!(page_label(0, 0, 0), "0 records");
}

fn inputs_scope() -> Scope {
    scope(json!({
        "parent": {"id": 4, "status": "open", "notes": "long text"},
        "form": {"min": 3, "other": "x"},
        "app": {"user": {"id": 9, "name": "Ann"}},
        "params": {},
    }))
}

#[test]
fn filter_inputs_keeps_only_the_referenced_values_never_the_row() {
    let picked = filter_inputs(
        Some("record.qty > form.min and record.owner = app.user.id and parent.status = 'open'"),
        &inputs_scope(),
    );
    assert_eq!(
        Value::Object(picked),
        json!({"form": {"min": 3}, "app": {"user": {"id": 9}}, "parent": {"status": "open"}})
    );
}

#[test]
fn filter_inputs_copies_a_whole_root_when_the_filter_uses_it_whole() {
    let s = inputs_scope();
    let picked = filter_inputs(Some("not isnull(parent) and parent.id > 0"), &s);
    assert_eq!(Value::Object(picked), json!({"parent": s["parent"]}));
}

#[test]
fn filter_inputs_copies_a_null_or_missing_value_where_the_path_stops() {
    assert_eq!(
        Value::Object(filter_inputs(Some("parent.id > 0"), &scope(json!({"parent": null})))),
        json!({"parent": null})
    );
    assert_eq!(
        Value::Object(filter_inputs(Some("form.nope > 0"), &inputs_scope())),
        json!({"form": {}})
    );
}

#[test]
fn filter_inputs_key_changes_only_with_referenced_values() {
    let src = Some("record.qty > form.min");
    let s = inputs_scope();
    let key = filter_inputs_key(src, &s);
    let mut other = s.clone();
    other.insert("form".into(), json!({"min": 3, "other": "y"}));
    assert_eq!(filter_inputs_key(src, &other), key);
    other.insert("form".into(), json!({"min": 4, "other": "x"}));
    assert_ne!(filter_inputs_key(src, &other), key);
    assert_eq!(filter_inputs_key(Some(""), &s), "");
    assert_eq!(
        filter_inputs_key(Some("record.qty >"), &s),
        r#"["record.qty >",{}]"#
    );
}
